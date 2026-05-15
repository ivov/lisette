use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::expressions::context::ExpressionContext;
use crate::names::go_name;
use crate::placement::ValuePlace;
use crate::types::native::NativeGoType;
use crate::utils::{group_params, receiver_name};
use syntax::ast::{
    Annotation, Binding, Expression, FunctionDefinition, Generic, Pattern, Span, TypedPattern,
};
use syntax::types::Type;

/// Owned param-destructure record: temp var, pattern, typed pattern, param type.
type DeferredParamDestructure = (String, Pattern, Option<TypedPattern>, Type);

impl Emitter<'_> {
    pub(crate) fn emit_function_body(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
        return_ctx: &crate::ReturnContext,
    ) {
        self.push_const_frame();
        self.emit_function_body_inner(output, body, should_return, return_ctx);
        self.pop_const_frame();
    }

    fn emit_function_body_inner(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
        return_ctx: &crate::ReturnContext,
    ) {
        let items: &[Expression] = if let Expression::Block { items, .. } = body {
            items
        } else {
            std::slice::from_ref(body)
        };

        let Some((last, rest)) = items.split_last() else {
            return;
        };

        for item in rest {
            self.emit_statement(output, item);
        }

        if should_return {
            self.emit_to_place(output, last, ValuePlace::Return(return_ctx));
        } else {
            self.emit_statement(output, last);
        }
    }

    pub(crate) fn emit_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> String {
        let frame = self.scope.enter_isolated_function();

        let mut destructure_bindings: Vec<(String, &Pattern, Option<&TypedPattern>, &Type)> =
            vec![];

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
                (name, self.go_type_as_string(&p.ty))
            })
            .collect();

        let argument_flows_to_unknown = ctx.argument_flows_to_unknown();
        let suppress_lowering = ctx.forces_tagged_go_function();

        let has_return = matches!(ty, Type::Function { return_type, .. }
            if !(return_type.is_unit()
                || return_type.is_variable()
                || (argument_flows_to_unknown && return_type.is_never())));

        // When the lambda flows into a Go-prelude generic callback that
        // expects the unlowered single-return form, suppress the lambda's
        // own return-type lowering so signature and body match.
        let return_ty_string = if has_return {
            match ty {
                Type::Function { return_type, .. } => {
                    if !suppress_lowering
                        && let Some(shape) = self.classify_direct_emission(return_type)
                    {
                        format!(" {}", self.render_lowered_return_ty(&shape, return_type))
                    } else {
                        format!(" {}", self.go_type_as_string(return_type))
                    }
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };

        let should_return = has_return;

        let return_ctx = match ty {
            Type::Function { return_type, .. } => {
                let return_ty = return_type.as_ref().clone();
                if suppress_lowering {
                    crate::ReturnContext::Tagged(return_ty)
                } else {
                    self.return_context_for_type(return_ty)
                }
            }
            _ => crate::ReturnContext::None,
        };

        let mut body_string = String::new();
        self.with_scope_return_context_fallback(return_ctx.clone(), |this| {
            for (temp_name, pattern, typed, param_ty) in &destructure_bindings {
                this.emit_irrefutable_pattern_site(
                    &mut body_string,
                    crate::patterns::sites::PatternSubject::for_value(temp_name.clone()),
                    pattern,
                    *typed,
                    param_ty,
                );
            }
            this.emit_function_body(&mut body_string, body, should_return, &return_ctx);
        });

        self.scope.exit_isolated_function(frame);

        format!(
            "func({}){} {{\n{}}}",
            group_params(&param_pairs),
            return_ty_string,
            body_string
        )
    }

    /// Bind and declare a parameter. If the natural post-escape Go name is
    /// already declared in this scope, pick a fresh Go name so later identifier lookups in the body resolve to the renamed slot.
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

    pub(crate) fn is_go_never(expression: &Expression) -> bool {
        match expression {
            Expression::Return { .. } => true,
            Expression::Call { expression, .. } => {
                matches!(&**expression, Expression::Identifier { value, .. } if value == "panic")
            }
            _ => false,
        }
    }

    pub(crate) fn emit_function(
        &mut self,
        function_definition: &FunctionDefinition,
        receiver: Option<(String, Type)>,
        is_public: bool,
    ) -> String {
        if matches!(*function_definition.body, Expression::NoOp) {
            return String::new();
        }

        let directive = self.maybe_line_directive(&function_definition.name_span);

        let return_ctx = self.return_context_for_type(function_definition.return_type.clone());

        let (function_definition, receiver) =
            self.change_go_builtin_methods(function_definition, receiver);

        let (params_to_process, receiver_override) =
            self.extract_receiver(&function_definition, receiver.is_some());

        let mut parts = vec!["func".to_string()];

        let (_, receiver_part) =
            self.emit_receiver_part(params_to_process, &receiver, receiver_override.as_ref());
        if let Some(part) = receiver_part {
            parts.push(part);
        }

        let function_name = if is_public {
            go_name::snake_to_camel(&function_definition.name)
        } else if receiver.is_some() {
            go_name::escape_keyword(&function_definition.name).into_owned()
        } else if let Some(remapped) = self.module.escape_remap(function_definition.name.as_str()) {
            remapped.to_string()
        } else {
            go_name::escape_reserved(&function_definition.name).into_owned()
        };
        parts.push(function_name);

        let generic_names: Vec<&str> = function_definition
            .generics
            .iter()
            .map(|g| g.name.as_ref())
            .collect();
        let sig_types = params_to_process
            .iter()
            .map(|p| &p.ty)
            .chain(std::iter::once(&function_definition.return_type));
        let mut map_key_generics = Self::collect_map_key_generics(sig_types, &generic_names);
        for name in &generic_names {
            if !map_key_generics.contains(*name)
                && Self::body_has_map_key_generic(&function_definition.body, name)
            {
                map_key_generics.insert(name.to_string());
            }
        }

        let generics_str =
            self.generics_to_string_with_map_keys(&function_definition.generics, &map_key_generics);
        if !generics_str.is_empty() {
            parts.push(generics_str);
        }

        let mut body = String::new();
        let signature = self.with_absorbed_ref_generics(
            params_to_process,
            &function_definition.generics,
            |this| {
                let (params_string, deferred_patterns) =
                    this.emit_function_params(params_to_process);
                parts.push(params_string);

                let return_ty = if function_definition.return_type.is_unit() {
                    String::new()
                } else if let Some(shape) =
                    this.classify_direct_emission(&function_definition.return_type)
                {
                    this.render_lowered_return_ty(&shape, &function_definition.return_type)
                } else {
                    this.go_type_as_string(&function_definition.return_type)
                };

                if !return_ty.is_empty() {
                    parts.push(return_ty);
                }

                let signature = parts.join(" ");
                let should_return = !function_definition.return_type.is_unit();

                this.with_scope_return_context_fallback(return_ctx.clone(), |this| {
                    for (var_name, pattern, typed, param_ty) in deferred_patterns {
                        this.emit_irrefutable_pattern_site(
                            &mut body,
                            crate::patterns::sites::PatternSubject::for_value(var_name),
                            &pattern,
                            typed.as_ref(),
                            &param_ty,
                        );
                    }

                    this.emit_function_body(
                        &mut body,
                        &function_definition.body,
                        should_return,
                        &return_ctx,
                    );
                });
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

    fn change_go_builtin_methods(
        &mut self,
        function_definition: &FunctionDefinition,
        receiver: Option<(String, Type)>,
    ) -> (FunctionDefinition, Option<(String, Type)>) {
        let Some((receiver_name, receiver_type)) = receiver else {
            return (function_definition.clone(), None);
        };

        let Some(native) = NativeGoType::from_type(&receiver_type) else {
            return (
                function_definition.clone(),
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
        (new_function_definition, None)
    }

    fn emit_receiver_part(
        &mut self,
        params_to_process: &[Binding],
        receiver: &Option<(String, Type)>,
        receiver_override: Option<&Type>,
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
        let ty_string = self.go_type_as_string(actual_ty);
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
        f: F,
    ) -> R
    where
        F: FnOnce(&mut Self) -> R,
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
                && bounded_generics.contains(name.as_ref())
            {
                self.function_state
                    .record_absorbed_ref_generic(name.to_string());
            }
        }
        let result = f(self);
        self.function_state = saved;
        result
    }

    fn emit_function_params(
        &mut self,
        params_to_process: &[Binding],
    ) -> (String, Vec<DeferredParamDestructure>) {
        let mut deferred_patterns = Vec::new();
        let mut params = Vec::new();
        for param in params_to_process {
            let name = match &param.pattern {
                Pattern::Identifier { identifier, .. } => {
                    if let Some(go_name) = self.go_name_for_binding(&param.pattern) {
                        self.declare_param(identifier, go_name)
                    } else {
                        "_".to_string()
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
            params.push((name, self.go_type_as_string(&param_type)));
        }
        (format!("({})", group_params(&params)), deferred_patterns)
    }

    fn extract_receiver<'a>(
        &mut self,
        function_definition: &'a FunctionDefinition,
        has_receiver: bool,
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
        let _ty_str = self.go_type_as_string(receiver_ty);

        (&function_definition.params[1..], Some(receiver_ty.clone()))
    }
}
