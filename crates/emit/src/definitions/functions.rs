use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::names::go_name;
use crate::types::emitter::Position;
use crate::types::native::NativeGoType;
use crate::utils::{group_params, optimize_function_body, receiver_name, requires_temp_var};
use crate::write_line;
use syntax::ast::{
    Annotation, Binding, Expression, FunctionDefinition, Generic, Pattern, Span, TypedPattern,
};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn emit_function_body(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
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

        let is_statement_only = matches!(
            last,
            Expression::Assignment { .. } | Expression::Let { .. } | Expression::Const { .. }
        );

        let needs_return = should_return
            && !matches!(last, Expression::Return { .. })
            && !is_statement_only
            && !last.get_type().is_unit()
            && !last.get_type().is_never();

        if !needs_return {
            self.emit_non_returning_tail(output, last, should_return, is_statement_only);
            return;
        }

        if self.try_emit_lowered_tail_return(output, last) {
            return;
        }

        if self.emit_wrapped_return(output, last) {
            return;
        }

        self.emit_returning_tail(output, last);
    }

    /// Tail that doesn't itself produce a returned value: statement-only tails
    /// (`let`/`const`/assignment), unit/never-typed tails, or explicit `Return`.
    /// Emits the tail as a statement, then appends a `panic("unreachable")` for
    /// non-Go never tails and a zero-value `return` when the function needs a
    /// typed return value but the tail couldn't provide one.
    fn emit_non_returning_tail(
        &mut self,
        output: &mut String,
        last: &Expression,
        should_return: bool,
        is_statement_only: bool,
    ) {
        self.emit_statement(output, last);
        if should_return && last.get_type().is_never() && !Self::is_go_never(last) {
            output.push_str("panic(\"unreachable\")\n");
        }
        let last_is_unit_expr = !is_statement_only
            && !matches!(last, Expression::Return { .. })
            && last.get_type().is_unit();
        if should_return
            && (is_statement_only || last_is_unit_expr)
            && self
                .current_return_context
                .as_ref()
                .is_some_and(|ctx| !ctx.ty.is_unit())
        {
            let return_ty = self.current_return_context.as_ref().unwrap().ty.clone();
            let zero = self.zero_value(&return_ty);
            write_line!(output, "return {}", zero);
        }
    }

    /// Tail that produces the function's return value. Value-shaped tails
    /// flow through `emit_value` + coercion; branching/block/loop shapes emit
    /// into a tail position that writes `return` at the leaves.
    fn emit_returning_tail(&mut self, output: &mut String, last: &Expression) {
        self.with_position(Position::Tail, |this| {
            if !requires_temp_var(last) {
                let expression = this.emit_value(output, last);
                let return_ty = this
                    .current_return_context
                    .as_ref()
                    .map(|ctx| ctx.ty.clone());
                let expression =
                    this.apply_type_coercion(output, return_ty.as_ref(), last, expression);
                output.push_str(&this.wrap_value(&expression));
                return;
            }
            match last {
                Expression::If { .. } | Expression::Match { .. } | Expression::Select { .. } => {
                    this.emit_branching_directly(output, last);
                }
                Expression::IfLet { .. } => {
                    unreachable!("IfLet should be desugared to Match before emit")
                }
                Expression::Block { .. }
                | Expression::Loop { .. }
                | Expression::Propagate { .. } => {
                    let expression = this.emit_operand(output, last);
                    output.push_str(&this.wrap_value(&expression));
                }
                _ => unreachable!("requires_temp_var returned true for unexpected expression"),
            }
        });
    }

    pub(crate) fn emit_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
    ) -> String {
        let saved_declared = std::mem::take(&mut self.scope.declared);
        let saved_scope_depth = self.scope.scope_depth;
        self.scope.declared = vec![HashSet::default()];
        self.scope.scope_depth = 0;

        self.scope.bindings.save();

        let mut destructure_bindings: Vec<(String, &Pattern, Option<&TypedPattern>)> = vec![];

        let param_pairs: Vec<(String, String)> = params
            .iter()
            .map(|p| {
                let name = if let Pattern::Identifier { identifier, .. } = &p.pattern {
                    if let Some(go_name) = self.go_name_for_binding(&p.pattern) {
                        let go_id = self.scope.bindings.add(identifier, go_name);
                        self.declare(&go_id);
                        go_id
                    } else {
                        self.scope.bindings.add(identifier, "_");
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
                    ));
                    temp_name
                };
                (name, self.go_type_as_string(&p.ty))
            })
            .collect();

        let has_return = matches!(ty, Type::Function { return_type, .. }
            if !return_type.is_unit() && !return_type.is_variable());

        // When the lambda flows into a Go-prelude generic callback that
        // expects the unlowered single-return form, suppress the lambda's
        // own return-type lowering so signature and body match.
        let suppress_lowering = self.suppress_go_fn_short_circuit;
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

        let saved_return_context = self.current_return_context.clone();
        if let Type::Function { return_type, .. } = ty {
            self.current_return_context = Some(if suppress_lowering {
                crate::ReturnContext::tagged(return_type.as_ref().clone())
            } else {
                crate::ReturnContext::new(return_type.as_ref().clone())
            });
        }
        let saved_suppress = std::mem::replace(&mut self.suppress_go_fn_short_circuit, false);

        let mut body_string = String::new();

        for (temp_name, pattern, typed) in &destructure_bindings {
            self.emit_pattern_bindings(&mut body_string, temp_name, pattern, *typed);
        }

        self.emit_function_body(&mut body_string, body, should_return);
        optimize_function_body(&mut body_string);

        self.scope.declared = saved_declared;
        self.scope.scope_depth = saved_scope_depth;
        self.scope.bindings.restore();

        self.current_return_context = saved_return_context;
        self.suppress_go_fn_short_circuit = saved_suppress;

        format!(
            "func({}){} {{\n{}}}",
            group_params(&param_pairs),
            return_ty_string,
            body_string
        )
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

        let saved_return_context = self.current_return_context.clone();
        self.current_return_context = Some(crate::ReturnContext::new(
            function_definition.return_type.clone(),
        ));

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

        let saved_absorbed =
            self.detect_absorbed_ref_generics(params_to_process, &function_definition.generics);

        let (params_string, deferred_patterns) = self.emit_function_params(params_to_process);
        parts.push(params_string);

        let return_ty = if function_definition.return_type.is_unit() {
            String::new()
        } else if let Some(shape) = self.classify_direct_emission(&function_definition.return_type)
        {
            self.render_lowered_return_ty(&shape, &function_definition.return_type)
        } else {
            self.go_type_as_string(&function_definition.return_type)
        };

        if !return_ty.is_empty() {
            parts.push(return_ty);
        }

        let signature = parts.join(" ");

        let mut body = String::new();

        for (var_name, pattern, typed) in deferred_patterns {
            self.emit_pattern_bindings(&mut body, &var_name, &pattern, typed.as_ref());
        }

        self.emit_function_body(
            &mut body,
            &function_definition.body,
            !function_definition.return_type.is_unit(),
        );
        optimize_function_body(&mut body);

        self.current_return_context = saved_return_context;
        self.module.absorbed_ref_generics = saved_absorbed;

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

        self.scope.bindings.add("self", receiver_var.clone());
        self.declare(&receiver_var);

        (Some(receiver_var), Some(receiver_part))
    }

    /// Detect Ref<T> parameters where T is a bounded generic and populate
    /// absorbed_ref_generics. Returns the previous value for restoration.
    fn detect_absorbed_ref_generics(
        &mut self,
        params: &[Binding],
        generics: &[Generic],
    ) -> HashSet<String> {
        let saved = self.module.absorbed_ref_generics.clone();
        self.module.absorbed_ref_generics.clear();
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
                self.module.absorbed_ref_generics.insert(name.to_string());
            }
        }
        saved
    }

    fn emit_function_params(
        &mut self,
        params_to_process: &[Binding],
    ) -> (String, Vec<(String, Pattern, Option<TypedPattern>)>) {
        let mut deferred_patterns = Vec::new();
        let mut params = Vec::new();
        for param in params_to_process {
            let name = match &param.pattern {
                Pattern::Identifier { identifier, .. } => {
                    if let Some(go_name) = self.go_name_for_binding(&param.pattern) {
                        let go_id = self.scope.bindings.add(identifier, go_name);
                        self.declare(&go_id);
                        go_id
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
                    ));
                    var
                }
            };

            let param_type = {
                if param.ty.is_ref()
                    && let Some(inner) = param.ty.inner()
                    && let Type::Parameter(name) = &inner
                    && self.module.absorbed_ref_generics.contains(name.as_ref())
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
