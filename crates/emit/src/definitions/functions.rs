use rustc_hash::FxHashSet as HashSet;

use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::ReturnContext;
use crate::abi::AbiShape;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::patterns::sites::PatternSubject;
use crate::types::native::NativeGoType;
use crate::utils::{group_params, receiver_name};
use syntax::ast::{
    Annotation, Binding, Expression, FunctionDefinition, Generic, Pattern, Span, TypedPattern,
};
use syntax::types::Type;

/// Owned param-destructure record: temp var, pattern, typed pattern, param type.
type DeferredParamDestructure = (String, Pattern, Option<TypedPattern>, Type);

fn receiver_type_name(ty: &Type) -> Option<&str> {
    if let Type::Nominal { id, .. } = ty.unwrap_forall() {
        Some(syntax::types::unqualified_name(id.as_str()))
    } else {
        None
    }
}

/// Borrowed lambda param-destructure record. Lambdas keep references to the
/// caller's `params` slice since they cannot outlive emission scope.
type LambdaParamDestructure<'a> = (String, &'a Pattern, Option<&'a TypedPattern>, &'a Type);

struct LambdaReturnInfo {
    ty_string: String,
    ctx: ReturnContext,
    has_return: bool,
}

impl Planner<'_> {
    pub(crate) fn emit_function_body(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) {
        self.push_const_frame();
        self.push_return_ctx(return_ctx.clone());
        self.emit_function_body_inner(output, body, should_return, fx);
        self.pop_return_ctx();
        self.pop_const_frame();
    }

    fn emit_function_body_inner(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
        fx: &mut EmitEffects,
    ) {
        let lowered = self.lower_function_body(body, should_return, fx);
        Renderer.render_lowered_block(output, &lowered);
    }

    pub(crate) fn emit_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        let frame = self.scope.enter_isolated_function();

        let (param_pairs, destructure_bindings) = self.build_lambda_param_pairs(params, fx);
        let return_info = self.lambda_return_info(ty, ctx, fx);
        let body_string = self.emit_lambda_body_with_deferred(
            body,
            &destructure_bindings,
            &return_info.ctx,
            return_info.has_return,
            fx,
        );

        self.scope.exit_isolated_function(frame);

        format!(
            "func({}){} {{\n{}}}",
            group_params(&param_pairs),
            return_info.ty_string,
            body_string
        )
    }

    fn build_lambda_param_pairs<'a>(
        &mut self,
        params: &'a [Binding],
        fx: &mut EmitEffects,
    ) -> (Vec<(String, String)>, Vec<LambdaParamDestructure<'a>>) {
        let mut destructure_bindings: Vec<LambdaParamDestructure<'a>> = vec![];
        let param_pairs: Vec<(String, String)> = params
            .iter()
            .map(|p| {
                let name = if let Pattern::Identifier { identifier, .. } = &p.pattern {
                    if let Some(go_name) = self.go_name_for_binding(&p.pattern) {
                        self.declare_param(identifier, go_name)
                    } else {
                        self.scope.bind(identifier, "_");
                        "_".to_string()
                    }
                } else if matches!(&p.pattern, Pattern::WildCard { .. }) {
                    "_".to_string()
                } else {
                    let temp_name = self.fresh_var(Some("arg"));
                    self.declare(&temp_name);
                    destructure_bindings.push((
                        temp_name.clone(),
                        &p.pattern,
                        p.typed_pattern.as_ref(),
                        &p.ty,
                    ));
                    temp_name
                };
                (name, self.go_type_string(&p.ty, fx))
            })
            .collect();
        (param_pairs, destructure_bindings)
    }

    /// Lambda Go return-type + `ReturnContext`. Go-prelude generic callbacks
    /// suppress lambda return-type lowering so signature and body agree.
    fn lambda_return_info(
        &mut self,
        ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> LambdaReturnInfo {
        let suppress_lowering = ctx.forces_tagged_go_function();
        let argument_flows_to_unknown = ctx.argument_flows_to_unknown();

        let has_return = matches!(ty, Type::Function(f)
            if !(f.return_type.is_unit()
                || f.return_type.is_variable()
                || (argument_flows_to_unknown && f.return_type.is_never())));

        let ctx = match ty {
            Type::Function(f) => {
                let return_ty = f.return_type.as_ref().clone();
                if suppress_lowering {
                    ReturnContext::Tagged(return_ty)
                } else {
                    self.return_context_for_type(return_ty)
                }
            }
            _ => ReturnContext::None,
        };

        let ty_string = if has_return {
            match ty {
                Type::Function(f) => match ctx.lowered_shape() {
                    Some(shape) => {
                        format!(
                            " {}",
                            self.render_lowered_return_ty(&shape, &f.return_type, fx)
                        )
                    }
                    None => format!(" {}", self.go_type_string(&f.return_type, fx)),
                },
                _ => String::new(),
            }
        } else {
            String::new()
        };

        LambdaReturnInfo {
            ty_string,
            ctx,
            has_return,
        }
    }

    fn emit_lambda_body_with_deferred(
        &mut self,
        body: &Expression,
        destructure_bindings: &[LambdaParamDestructure<'_>],
        return_ctx: &ReturnContext,
        should_return: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let mut body_string = String::new();
        for (temp_name, pattern, typed, param_ty) in destructure_bindings {
            self.emit_irrefutable_pattern_site(
                &mut body_string,
                PatternSubject::for_value(temp_name.clone()),
                pattern,
                *typed,
                param_ty,
                fx,
            );
        }
        self.emit_function_body(&mut body_string, body, should_return, return_ctx, fx);
        body_string
    }

    /// Bind and declare a parameter; freshens the Go name on collision.
    fn declare_param(&mut self, lisette_name: &str, raw_go_name: impl Into<String>) -> String {
        let go_id = self.scope.bind(lisette_name, raw_go_name);
        let go_id = if self.is_declared(&go_id) {
            let fresh = self.fresh_var(Some(lisette_name));
            self.scope.bind(lisette_name, fresh)
        } else {
            go_id
        };
        self.declare(&go_id);
        go_id
    }

    pub(crate) fn emit_function(
        &mut self,
        function_definition: &FunctionDefinition,
        receiver: Option<(String, Type)>,
        is_public: bool,
        fx: &mut EmitEffects,
    ) -> String {
        if matches!(*function_definition.body, Expression::NoOp) {
            return String::new();
        }

        let directive = self.maybe_line_directive(&function_definition.name_span);
        let return_ctx = self.return_context_for_type(function_definition.return_type.clone());
        let return_shape = return_ctx.lowered_shape();

        let (function_definition, receiver) =
            change_go_builtin_methods(function_definition, receiver);
        let (params_to_process, receiver_override) =
            self.extract_receiver(&function_definition, receiver.is_some(), fx);

        let mut parts = vec!["func".to_string()];

        let (_, receiver_part) =
            self.emit_receiver_part(params_to_process, &receiver, receiver_override.as_ref(), fx);
        if let Some(part) = receiver_part {
            parts.push(part);
        }

        parts.push(self.pick_go_function_name(&function_definition, receiver.is_some(), is_public));

        let generics_str = self.build_generics_string(
            &function_definition,
            params_to_process,
            receiver.as_ref(),
            fx,
        );
        if !generics_str.is_empty() {
            parts.push(generics_str);
        }

        let mut body = String::new();
        let signature = self.with_absorbed_ref_generics(
            params_to_process,
            &function_definition.generics,
            fx,
            |this, fx| {
                let (params_string, return_ty, deferred_patterns) = this.build_signature_tail(
                    &function_definition,
                    params_to_process,
                    return_shape.as_ref(),
                    fx,
                );
                parts.push(params_string);
                if !return_ty.is_empty() {
                    parts.push(return_ty);
                }
                let signature = parts.join(" ");
                this.emit_function_body_with_deferred_patterns(
                    &mut body,
                    &function_definition,
                    deferred_patterns,
                    &return_ctx,
                    fx,
                );
                signature
            },
        );

        let trimmed_body = body.trim_end();
        if trimmed_body.is_empty() {
            format!("{}{} {{}}", directive, signature)
        } else {
            format!("{}{} {{\n{}\n}}", directive, signature, trimmed_body)
        }
    }

    fn pick_go_function_name(
        &self,
        function_definition: &FunctionDefinition,
        has_receiver: bool,
        is_public: bool,
    ) -> String {
        if is_public {
            go_name::snake_to_camel(&function_definition.name)
        } else if has_receiver {
            go_name::escape_keyword(&function_definition.name).into_owned()
        } else if let Some(remapped) = self.module.escape_remap(function_definition.name.as_str()) {
            remapped.to_string()
        } else {
            go_name::escape_reserved(&function_definition.name).into_owned()
        }
    }

    fn build_generics_string(
        &mut self,
        function_definition: &FunctionDefinition,
        _params_to_process: &[Binding],
        receiver: Option<&(String, Type)>,
        fx: &mut EmitEffects,
    ) -> String {
        let symbol = self.symbol_for_function(&function_definition.name, receiver);
        self.generics_to_string_for_symbol(&symbol, &function_definition.generics, fx)
    }

    fn symbol_for_function(
        &self,
        function_name: &str,
        receiver: Option<&(String, Type)>,
    ) -> String {
        if let Some((_, receiver_ty)) = receiver
            && let Some(name) = receiver_type_name(receiver_ty)
        {
            return self.facts.qualified_current_member(name, function_name);
        }
        if let Some((receiver, method)) = function_name.split_once('.') {
            self.facts.qualified_current_member(receiver, method)
        } else {
            self.facts.qualified_current(function_name)
        }
    }

    fn build_signature_tail(
        &mut self,
        function_definition: &FunctionDefinition,
        params_to_process: &[Binding],
        return_shape: Option<&AbiShape>,
        fx: &mut EmitEffects,
    ) -> (String, String, Vec<DeferredParamDestructure>) {
        let (params_string, deferred_patterns) = self.emit_function_params(params_to_process, fx);

        let return_ty = if function_definition.return_type.is_unit() {
            String::new()
        } else if let Some(shape) = return_shape {
            self.render_lowered_return_ty(shape, &function_definition.return_type, fx)
        } else {
            self.go_type_string(&function_definition.return_type, fx)
        };

        (params_string, return_ty, deferred_patterns)
    }

    fn emit_function_body_with_deferred_patterns(
        &mut self,
        body: &mut String,
        function_definition: &FunctionDefinition,
        deferred_patterns: Vec<DeferredParamDestructure>,
        return_ctx: &ReturnContext,
        fx: &mut EmitEffects,
    ) {
        let should_return = !function_definition.return_type.is_unit();
        for (var_name, pattern, typed, param_ty) in deferred_patterns {
            self.emit_irrefutable_pattern_site(
                body,
                PatternSubject::for_value(var_name),
                &pattern,
                typed.as_ref(),
                &param_ty,
                fx,
            );
        }
        self.emit_function_body(
            body,
            &function_definition.body,
            should_return,
            return_ctx,
            fx,
        );
    }

    fn emit_receiver_part(
        &mut self,
        params_to_process: &[Binding],
        receiver: &Option<(String, Type)>,
        receiver_override: Option<&Type>,
        fx: &mut EmitEffects,
    ) -> (Option<String>, Option<String>) {
        let Some((_, receiver_ty)) = receiver else {
            return (None, None);
        };

        let param_names: Vec<String> = params_to_process
            .iter()
            .filter_map(|param| {
                if let Pattern::Identifier { identifier, .. } = &param.pattern {
                    Some(identifier.to_string())
                } else {
                    None
                }
            })
            .collect();

        let actual_ty = receiver_override.unwrap_or(receiver_ty);
        let ty_string = self.go_type_string(actual_ty, fx);
        let mut receiver_var = receiver_name(&ty_string);

        if param_names.contains(&receiver_var) {
            receiver_var = format!("{}{}", receiver_var, receiver_var);
            let mut counter = 2;
            while param_names.contains(&receiver_var) {
                receiver_var = format!("{}{}", receiver_name(&ty_string), counter);
                counter += 1;
            }
        }

        let receiver_part = format!("({} {})", receiver_var, ty_string);

        self.scope.bind("self", receiver_var.clone());
        self.declare(&receiver_var);

        (Some(receiver_var), Some(receiver_part))
    }

    fn with_absorbed_ref_generics<F, R>(
        &mut self,
        params: &[Binding],
        generics: &[Generic],
        fx: &mut EmitEffects,
        f: F,
    ) -> R
    where
        F: FnOnce(&mut Self, &mut EmitEffects) -> R,
    {
        let saved = std::mem::take(&mut self.function_state);
        let bounded_generics: HashSet<&str> = generics
            .iter()
            .filter(|g| !g.bounds.is_empty())
            .map(|g| g.name.as_ref())
            .collect();
        for param in params.iter() {
            if param.ty.is_ref()
                && let Some(inner) = param.ty.inner()
                && let Type::Parameter(name) = &inner
                && bounded_generics.contains(name.as_str())
            {
                self.function_state
                    .record_absorbed_ref_generic(name.to_string());
            }
        }
        let result = f(self, fx);
        self.function_state = saved;
        result
    }

    fn emit_function_params(
        &mut self,
        params_to_process: &[Binding],
        fx: &mut EmitEffects,
    ) -> (String, Vec<DeferredParamDestructure>) {
        let mut deferred_patterns = Vec::new();
        let mut params = Vec::new();
        for param in params_to_process {
            let name = match &param.pattern {
                Pattern::Identifier { identifier, .. } => {
                    if let Some(go_name) = self.go_name_for_binding(&param.pattern) {
                        self.declare_param(identifier, go_name)
                    } else {
                        self.scope.bind(identifier.as_str(), "_")
                    }
                }
                Pattern::WildCard { .. } => "_".to_string(),
                _ => {
                    let var = self.fresh_var(Some("arg"));
                    self.declare(&var);
                    deferred_patterns.push((
                        var.clone(),
                        param.pattern.clone(),
                        param.typed_pattern.clone(),
                        param.ty.clone(),
                    ));
                    var
                }
            };

            let param_type = {
                if param.ty.is_ref()
                    && let Some(inner) = param.ty.inner()
                    && let Type::Parameter(name) = &inner
                    && self.function_state.is_absorbed_ref_generic(name.as_ref())
                {
                    inner
                } else {
                    param.ty.clone()
                }
            };
            params.push((name, self.go_type_string(&param_type, fx)));
        }
        (format!("({})", group_params(&params)), deferred_patterns)
    }

    fn extract_receiver<'a>(
        &mut self,
        function_definition: &'a FunctionDefinition,
        has_receiver: bool,
        fx: &mut EmitEffects,
    ) -> (&'a [Binding], Option<Type>) {
        let default = (&function_definition.params[..], None);

        if !has_receiver || function_definition.params.is_empty() {
            return default;
        }

        let Pattern::Identifier { identifier, .. } = &function_definition.params[0].pattern else {
            return default;
        };

        if identifier != "self" {
            return default;
        }

        let receiver_ty = &function_definition.params[0].ty;
        let _ty_str = self.go_type_string(receiver_ty, fx);

        (&function_definition.params[1..], Some(receiver_ty.clone()))
    }
}

pub(crate) fn is_go_never(expression: &Expression) -> bool {
    match expression {
        Expression::Return { .. } => true,
        Expression::Call { expression, .. } => {
            matches!(&**expression, Expression::Identifier { value, .. } if value == "panic")
        }
        _ => false,
    }
}

fn change_go_builtin_methods<'a>(
    function_definition: &'a FunctionDefinition,
    receiver: Option<(String, Type)>,
) -> (
    std::borrow::Cow<'a, FunctionDefinition>,
    Option<(String, Type)>,
) {
    use std::borrow::Cow;
    let Some((receiver_name, receiver_type)) = receiver else {
        return (Cow::Borrowed(function_definition), None);
    };

    let Some(native) = NativeGoType::from_type(&receiver_type) else {
        return (
            Cow::Borrowed(function_definition),
            Some((receiver_name, receiver_type)),
        );
    };

    let mut new_function_definition = function_definition.clone();
    new_function_definition.name =
        format!("{}.{}", native.lisette_name(), function_definition.name).into();

    let self_binding = Binding {
        pattern: Pattern::Identifier {
            identifier: receiver_name.into(),
            span: Span::dummy(),
        },
        annotation: Some(Annotation::Unknown),
        typed_pattern: None,
        ty: receiver_type,
        mutable: false,
    };

    new_function_definition.params.insert(0, self_binding);
    (Cow::Owned(new_function_definition), None)
}
