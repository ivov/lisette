use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::UnusedExpressionKind;
use syntax::ast::{Expression, Generic, Pattern, Span, StructKind, UnaryOperator};
use syntax::program::{Definition, ReceiverCoercion};
use syntax::types::{Bound, Type, TypeVariableState};

use crate::checker::PostInferenceCheck;

use super::super::Checker;
use super::expressions::patterns::collect_pattern_bindings;

impl Checker<'_, '_> {
    pub(crate) fn check_unused_expression(
        &mut self,
        span: Span,
        ty: &Type,
        is_literal: bool,
        allowed_lints: &[String],
    ) {
        let kind = if is_literal {
            Some(UnusedExpressionKind::Literal)
        } else if ty.is_result() {
            Some(UnusedExpressionKind::Result)
        } else if ty.is_option() {
            Some(UnusedExpressionKind::Option)
        } else if ty.is_partial() {
            Some(UnusedExpressionKind::Partial)
        } else if !ty.is_unit() && !ty.is_variable() && !ty.is_never() {
            Some(UnusedExpressionKind::Value)
        } else {
            None
        };

        if let Some(kind) = kind
            && !allowed_lints.contains(&kind.lint_name().to_string())
        {
            self.facts.add_unused_expression(span, kind);
        }
    }

    pub(crate) fn callee_allowed_lints(
        &mut self,
        callee_name: &str,
        inferred: &Expression,
    ) -> Vec<String> {
        if let Some(qualified) = self.lookup_qualified_name(callee_name)
            && let Some(definition) = self.store.get_definition(&qualified)
        {
            return definition.allowed_lints().to_vec();
        }

        if let Expression::Call { expression, .. } = inferred
            && let Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } = expression.as_ref()
        {
            let receiver_ty = receiver.get_type().resolve().strip_refs();
            if let Type::Constructor { id, .. } = &receiver_ty {
                let method_key = format!("{}.{}", id, member);
                if let Some(definition) = self.store.get_definition(&method_key) {
                    return definition.allowed_lints().to_vec();
                }
            }
        }

        vec![]
    }

    pub(crate) fn get_call_return_type(expression: &Expression) -> Option<Type> {
        if let Expression::Call { expression, .. } = expression {
            let callee_ty = expression.get_type().resolve();
            match callee_ty {
                Type::Function { return_type, .. } => Some(return_type.as_ref().clone()),
                _ => None,
            }
        } else {
            None
        }
    }

    pub(crate) fn is_channel_send(expression: &Expression) -> bool {
        if let Expression::Call { expression, .. } = expression
            && let Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } = expression.as_ref()
            && member == "send"
        {
            return receiver
                .get_type()
                .resolve()
                .get_name()
                .map(|n| n == "Channel" || n == "Sender")
                .unwrap_or(false);
        }
        false
    }

    pub(crate) fn check_unused_type_parameters(&mut self, generics: &[Generic], fn_ty: &Type) {
        if generics.is_empty() {
            return;
        }

        let mut remaining: HashSet<_> = generics.iter().map(|g| g.name.clone()).collect();
        fn_ty.remove_found_type_names(&mut remaining);

        let is_typedef = self.is_d_lis();
        for generic in generics {
            if generic.name.starts_with('_') {
                continue;
            }

            if remaining.contains(&generic.name) {
                self.facts.add_unused_type_param(
                    generic.name.to_string(),
                    generic.span,
                    is_typedef,
                );
            }
        }
    }

    pub(crate) fn check_type_params_only_in_bound(&mut self, generics: &[Generic], fn_ty: &Type) {
        if generics.is_empty() {
            return;
        }
        let bounds = fn_ty.get_bounds();
        if bounds.is_empty() {
            return;
        }
        let (Some(params), Some(return_type)) =
            (fn_ty.get_function_params(), fn_ty.get_function_ret())
        else {
            return;
        };

        let only_in_bound =
            collect_type_params_only_in_bound(generics, params, return_type, bounds);
        if only_in_bound.is_empty() {
            return;
        }

        let is_typedef = self.is_d_lis();
        for generic in generics {
            if generic.name.starts_with('_') || !only_in_bound.contains(&generic.name) {
                continue;
            }
            self.facts.add_type_param_only_in_bound(
                generic.name.to_string(),
                generic.span,
                is_typedef,
            );
        }
    }

    pub(crate) fn check_prelude_shadowing(&mut self, name: &str, span: Span) {
        if self.is_d_lis() {
            return;
        }
        let prelude_qualified_name = format!("prelude.{}", name);
        if let Some(prelude_module) = self.store.get_module("prelude")
            && prelude_module
                .definitions
                .contains_key(prelude_qualified_name.as_str())
        {
            self.sink
                .push(diagnostics::infer::prelude_type_shadowed(name, span));
        }
    }

    /// Returns `true` if valid (no error emitted), `false` if an error was emitted.
    pub(crate) fn check_slice_index_type(
        &mut self,
        type_name: &str,
        index_ty: &Type,
        span: Span,
    ) -> bool {
        if type_name != "Slice" || index_ty.is_variable() {
            return true;
        }

        if index_ty.get_name().is_some_and(|n| n == "int") {
            return true;
        }

        self.sink
            .push(diagnostics::infer::slice_index_type_mismatch(
                index_ty, span,
            ));
        false
    }

    pub(crate) fn check_call_arity(
        &mut self,
        param_types: &[Type],
        args: &[Expression],
        callee_expression: &Expression,
        span: &Span,
    ) {
        if param_types.len() != args.len() {
            let actual_types: Vec<Type> = args.iter().map(|e| e.get_type()).collect();
            let generic_params = self.get_generic_param_names(callee_expression);
            let is_constructor = callee_expression
                .get_var_name()
                .map(|name| name.chars().next().is_some_and(|c| c.is_uppercase()))
                .unwrap_or(false);
            self.sink.push(diagnostics::infer::arity_mismatch(
                param_types,
                &actual_types,
                &generic_params,
                is_constructor,
                *span,
            ));
        }
    }

    fn get_generic_param_names(&self, expression: &Expression) -> Vec<String> {
        if let Expression::Identifier { value, .. } = expression
            && let Some(ty) = self.scopes.lookup_value(value)
        {
            return match ty {
                Type::Forall { vars, .. } => vars.iter().map(|s| s.to_string()).collect(),
                _ => vec![],
            };
        }
        vec![]
    }

    pub(crate) fn check_unconstrained_bounded_type_params(
        &mut self,
        bounds: &[Bound],
        span: &Span,
    ) {
        for bound in bounds {
            if let Type::Variable(var) = &bound.generic.resolve()
                && let TypeVariableState::Unbound { .. } = &*var.borrow()
            {
                self.sink.push(diagnostics::infer::unconstrained_type_param(
                    &bound.param_name,
                    *span,
                ));
            }
        }
    }

    /// Runs all post-inference checks for unresolved type variables.
    /// Called after inference completes for a file, when type variables have had
    /// a chance to be constrained by later usage.
    pub fn run_post_inference_checks(&mut self) {
        for check in std::mem::take(&mut self.post_inference_checks) {
            match check {
                PostInferenceCheck::GenericCall { return_ty, span } => {
                    if return_ty.resolve().has_unbound_variables() {
                        self.sink
                            .push(diagnostics::infer::cannot_infer_type_argument(span));
                    }
                }
                PostInferenceCheck::EmptyCollection { name, ty, span } => {
                    if ty.resolve().has_unbound_variables() {
                        self.sink
                            .push(diagnostics::infer::uninferred_binding(&name, span));
                    }
                }
                PostInferenceCheck::StatementTail { expected_ty, span } => {
                    let resolved = expected_ty.resolve();
                    if !resolved.is_unit()
                        && !matches!(resolved, Type::Variable(_))
                        && !expected_ty.is_ignored()
                    {
                        self.sink.push(diagnostics::infer::statement_as_tail(span));
                    }
                }
            }
        }
    }

    pub(crate) fn check_constrained_return_type(
        &mut self,
        return_ty: &Type,
        generics: &[Generic],
        span: &Span,
        fn_name: &str,
    ) {
        let Type::Constructor { id, params, .. } = return_ty else {
            return;
        };

        if params.is_empty() {
            return;
        }

        let qualified_id = self
            .lookup_qualified_name(id)
            .unwrap_or_else(|| id.as_ref().to_string());
        let Some(definition) = self.store.get_definition(&qualified_id) else {
            return;
        };

        let methods = match definition {
            syntax::program::Definition::Struct { methods, .. } => methods,
            syntax::program::Definition::Enum { methods, .. } => methods,
            _ => return,
        };

        let mut required_bounds: HashMap<String, Vec<Type>> = HashMap::default();

        for method_ty in methods.values() {
            if let Type::Forall { vars: _, body } = method_ty {
                if let Type::Function { bounds, .. } = body.as_ref() {
                    for bound in bounds {
                        if let Type::Parameter(param_name) = &bound.generic {
                            let entry = required_bounds
                                .entry(param_name.as_ref().to_string())
                                .or_default();
                            if !entry.contains(&bound.ty) {
                                entry.push(bound.ty.clone());
                            }
                        }
                    }
                }
            } else if let Type::Function { bounds, .. } = method_ty {
                for bound in bounds {
                    if let Type::Parameter(param_name) = &bound.generic {
                        let entry = required_bounds
                            .entry(param_name.as_ref().to_string())
                            .or_default();
                        if !entry.contains(&bound.ty) {
                            entry.push(bound.ty.clone());
                        }
                    }
                }
            }
        }

        for return_param in params.iter() {
            if let Type::Parameter(param_name) = return_param
                && let Some(method_bounds) = required_bounds.get(param_name.as_ref())
            {
                let fn_generic = generics
                    .iter()
                    .find(|g| g.name.as_ref() == param_name.as_ref());

                if let Some(fn_gen) = fn_generic {
                    let fn_bounds: Vec<Type> = fn_gen
                        .bounds
                        .iter()
                        .map(|b| self.convert_to_type(b, span))
                        .collect();

                    for method_bound in method_bounds {
                        if !fn_bounds.iter().any(|fb| fb == method_bound) {
                            self.sink.push(
                                diagnostics::infer::missing_constraint_on_generic_return_type(
                                    fn_name,
                                    param_name.as_ref(),
                                    method_bound,
                                    *span,
                                ),
                            );
                        }
                    }
                } else {
                    // Generic parameter not declared on function — check impl-level bounds
                    let scope_bounds = self.scopes.collect_all_trait_bounds();
                    let qualified = self.qualify_name(param_name.as_ref());
                    let impl_bounds = scope_bounds
                        .get(qualified.as_str())
                        .or_else(|| scope_bounds.get(param_name.as_ref()));

                    let all_covered = impl_bounds
                        .is_some_and(|ib| method_bounds.iter().all(|mb| ib.contains(mb)));

                    if !all_covered {
                        let bound_str = method_bounds
                            .iter()
                            .map(|b| b.to_string())
                            .collect::<Vec<_>>()
                            .join(" + ");

                        self.sink.push(
                            diagnostics::infer::missing_constraint_on_generic_return_type(
                                fn_name,
                                param_name.as_ref(),
                                &Type::Parameter(bound_str.into()),
                                *span,
                            ),
                        );
                    }
                }
            }
        }
    }

    /// Checks whether an assignment target contains `.0` on a newtype
    /// (single-field, non-generic tuple struct). Newtypes compile to Go named
    /// types, so `.0` is read-only; users must reconstruct instead.
    pub(crate) fn check_newtype_field_assignment(&mut self, target: &Expression, span: Span) {
        match target {
            Expression::DotAccess {
                expression, member, ..
            } => {
                if member == "0" {
                    let base_ty = expression.get_type().resolve();
                    let ty = base_ty.strip_refs();
                    if let Type::Constructor { id, params, .. } = ty
                        && params.is_empty()
                        && let Some(Definition::Struct {
                            kind: StructKind::Tuple,
                            fields,
                            generics,
                            ..
                        }) = self.store.get_definition(id.as_str())
                        && fields.len() == 1
                        && generics.is_empty()
                    {
                        let type_name = id.rsplit('.').next().unwrap_or(id.as_str());
                        self.sink.push(diagnostics::infer::newtype_field_assignment(
                            type_name, span,
                        ));
                        return;
                    }
                }
                // Recurse into inner DotAccess to catch chains like w.0.x = 2
                self.check_newtype_field_assignment(expression, span);
            }
            Expression::IndexedAccess { expression, .. } => {
                self.check_newtype_field_assignment(expression, span);
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => {
                self.check_newtype_field_assignment(expression, span);
            }
            _ => {}
        }
    }

    /// Checks whether an assignment target has a DotAccess above a Map IndexedAccess.
    /// Map entries are returned by copy in Go, so `m["key"].field = v` is invalid.
    /// Users must extract, modify, and reinsert.
    pub(crate) fn check_map_field_chain_assignment(&mut self, target: &Expression, span: Span) {
        if self.has_map_field_in_chain(target) {
            self.sink
                .push(diagnostics::infer::map_field_chain_assignment(span));
        }
    }

    pub(crate) fn has_map_field_in_chain(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::DotAccess { expression, .. } => {
                self.is_map_indexed_access(expression) || self.has_map_field_in_chain(expression)
            }
            _ => false,
        }
    }

    fn is_map_indexed_access(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::IndexedAccess { expression, .. } => {
                expression.get_type().resolve().has_name("Map")
            }
            _ => false,
        }
    }

    fn resolves_to_struct_kind(&self, qualified: &str, kind: StructKind) -> bool {
        match self.store.get_definition(qualified) {
            Some(Definition::Struct { kind: k, .. }) => *k == kind,
            Some(Definition::TypeAlias { ty: alias_ty, .. }) => {
                if let Type::Constructor { id, .. } = alias_ty.unwrap_forall() {
                    matches!(
                        self.store.get_definition(id),
                        Some(Definition::Struct { kind: k, .. }) if *k == kind
                    )
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check if an identifier used as a value (not in call position) refers to a
    /// native method, native constructor, or private method expression.
    pub(crate) fn check_native_value_usage(&mut self, value: &str, ty: &Type, span: Span) {
        if matches!(
            value,
            "imaginary" | "assert_type" | "complex" | "real" | "panic"
        ) {
            let qualified = format!("{}.{}", self.cursor.module_id, value);
            if self.store.get_definition(&qualified).is_none() {
                self.sink
                    .push(diagnostics::infer::native_constructor_value(value, span));
                return;
            }
        }

        {
            let qualified = if value.contains('.') {
                value.to_string()
            } else {
                format!("{}.{}", self.cursor.module_id, value)
            };
            if self.resolves_to_struct_kind(&qualified, StructKind::Tuple) {
                self.sink
                    .push(diagnostics::infer::native_constructor_value(value, span));
                return;
            }
            if !self.inference.dot_access_base
                && self.resolves_to_struct_kind(&qualified, StructKind::Record)
            {
                self.sink
                    .push(diagnostics::infer::record_struct_value(value, span));
                return;
            }
        }

        let Some((type_part, method_part)) = value.split_once('.') else {
            return;
        };
        if method_part.contains('.') {
            return;
        }

        let is_native = matches!(
            type_part,
            "Slice" | "EnumeratedSlice" | "Map" | "Channel" | "Sender" | "Receiver" | "string"
        );

        if is_native {
            if matches!(method_part, "new" | "buffered") {
                self.sink
                    .push(diagnostics::infer::native_constructor_value(value, span));
            } else {
                self.sink
                    .push(diagnostics::infer::native_method_value(method_part, span));
            }
            return;
        }

        if matches!(method_part, "new" | "buffered") {
            let ret_ty = match ty {
                Type::Function { return_type, .. } => Some(return_type.as_ref()),
                Type::Forall { body, .. } => match body.as_ref() {
                    Type::Function { return_type, .. } => Some(return_type.as_ref()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(ret) = ret_ty {
                let resolved = ret.resolve();
                let is_native_ret =
                    matches!(resolved.get_name(), Some("Channel" | "Map" | "Slice"));
                if is_native_ret {
                    self.sink
                        .push(diagnostics::infer::native_constructor_value(value, span));
                    return;
                }
            }
        }

        let is_fn = matches!(ty, Type::Function { .. } | Type::Forall { .. });
        if !is_fn {
            return;
        }
        let fn_params = match ty {
            Type::Function { params, .. } => params.as_slice(),
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Function { params, .. } => params.as_slice(),
                _ => return,
            },
            _ => return,
        };
        let Some(first) = fn_params.first() else {
            return;
        };
        let stripped = first.strip_refs();
        let is_self = matches!(&stripped, Type::Constructor { id, .. }
            if id.rsplit('.').next() == Some(type_part));
        if !is_self {
            return;
        }

        // Look up method visibility
        let module_id = &self.cursor.module_id;
        let method_key = format!("{}.{}.{}", module_id, type_part, method_part);
        let is_public = self
            .store
            .get_definition(&method_key)
            .map(|d| d.visibility().is_public())
            .unwrap_or(true);

        if !is_public {
            self.sink
                .push(diagnostics::infer::private_method_expression(span));
        }
    }

    pub(crate) fn has_newtype_dot0_in_chain(&self, expression: &Expression) -> bool {
        let mut current = expression.unwrap_parens();
        while let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = current
        {
            if member.parse::<usize>().is_ok() {
                let ty = inner.get_type().resolve().strip_refs();
                if let Type::Constructor { id, .. } = &ty
                    && let Some(Definition::Struct {
                        kind: StructKind::Tuple,
                        fields,
                        generics,
                        ..
                    }) = self.store.get_definition(id.as_str())
                    && fields.len() == 1
                    && generics.is_empty()
                {
                    return true;
                }
            }
            current = inner.unwrap_parens();
        }
        false
    }

    pub(crate) fn check_not_temp_producing(&mut self, expression: &Expression) {
        if is_temp_producing(expression) || self.has_auto_address_on_call(expression) {
            self.sink.push(diagnostics::infer::complex_sub_expression(
                expression.get_span(),
            ));
        }
    }

    /// Check if this expression will produce pre-statements at emit time
    /// due to auto-address coercion on a function call result receiver.
    /// E.g. `make_box().get()` where `get` takes `Ref<Box>` — the emitter
    /// must hoist `make_box()` into a temp to take its address.
    fn has_auto_address_on_call(&self, expression: &Expression) -> bool {
        let expression = expression.unwrap_parens();
        if let Expression::Call { expression, .. } = expression
            && let Expression::DotAccess {
                expression: receiver,
                ..
            } = expression.unwrap_parens()
        {
            if matches!(receiver.unwrap_parens(), Expression::Call { .. })
                && self.coercions.get_coercion(receiver.get_span())
                    == Some(ReceiverCoercion::AutoAddress)
            {
                return true;
            }
            return self.has_auto_address_on_call(receiver);
        }
        false
    }
}

pub(crate) fn is_temp_producing(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Select { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
    )
}

pub(crate) fn check_is_non_addressable(expression: &Expression) -> Option<&'static str> {
    match expression {
        Expression::Identifier { .. } => None,
        Expression::DotAccess { expression, .. } => {
            let inner = expression.unwrap_parens();
            // Allow &call().x when call returns Ref<T> — pointer fields are addressable.
            let is_non_addressable_origin = matches!(inner, Expression::StructCall { .. })
                || (matches!(inner, Expression::Call { .. })
                    && !expression.get_type().resolve().is_ref());
            if is_non_addressable_origin {
                Some("field access on non-addressable value")
            } else {
                check_is_non_addressable(expression)
            }
        }
        Expression::IndexedAccess { expression, .. } => {
            let expression_ty = expression.get_type().resolve();
            if let Some(name) = expression_ty.get_name() {
                if name == "Map" {
                    return Some("map index expression");
                }
                // Slice elements are always addressable, even from call results.
                if name == "Slice" {
                    return None;
                }
            }
            if matches!(expression.unwrap_parens(), Expression::Call { .. }) {
                Some("index access on function call")
            } else {
                check_is_non_addressable(expression)
            }
        }
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => None,
        Expression::StructCall { .. } => None,
        Expression::Paren { expression, .. } => check_is_non_addressable(expression),
        Expression::Call { .. } => None,
        Expression::Literal { .. } => Some("literal"),
        Expression::Binary { .. } => Some("binary expression"),
        Expression::If { .. } | Expression::IfLet { .. } => Some("conditional expression"),
        Expression::Match { .. } => Some("match expression"),
        Expression::Block { .. } => Some("block expression"),
        Expression::Lambda { .. } => Some("lambda"),
        Expression::Tuple { .. } => Some("tuple"),
        Expression::Range { .. } => Some("range expression"),
        _ => Some("expression"),
    }
}

/// Check if an assignment target roots at a non-addressable expression.
/// Walks DotAccess/IndexedAccess chains to find the root. Call results,
/// struct literals, and tuple literals are not valid assignment roots.
pub(crate) fn check_non_addressable_assignment_target(
    expression: &Expression,
) -> Option<&'static str> {
    match expression.unwrap_parens() {
        Expression::Identifier { .. } => None,
        Expression::DotAccess { expression, .. } => {
            // Allow assignment through pointer: make().x = 5 when make() -> Ref<T>
            if matches!(expression.unwrap_parens(), Expression::Call { .. })
                && expression.get_type().resolve().is_ref()
            {
                None
            } else {
                check_non_addressable_assignment_target(expression)
            }
        }
        Expression::IndexedAccess { .. } => None,
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => None,
        Expression::Call { .. } => Some("function call result"),
        Expression::StructCall { .. } => Some("struct literal"),
        Expression::Tuple { .. } => Some("tuple literal"),
        _ => None,
    }
}

pub(crate) fn check_duplicate_bindings(sink: &diagnostics::DiagnosticSink, pattern: &Pattern) {
    if let Pattern::Or { patterns, .. } = pattern {
        for alternative_pattern in patterns {
            check_duplicate_bindings(sink, alternative_pattern);
        }
        return;
    }

    if matches!(
        pattern,
        Pattern::Identifier { .. }
            | Pattern::WildCard { .. }
            | Pattern::Literal { .. }
            | Pattern::Unit { .. }
    ) {
        return;
    }

    let bindings = collect_pattern_bindings(pattern);
    let mut seen: HashMap<&str, &Span> = HashMap::default();
    for (name, span) in &bindings {
        if let Some(first_span) = seen.get(name.as_str()) {
            sink.push(diagnostics::infer::duplicate_binding_in_pattern(
                name,
                *(*first_span),
                *span,
            ));
        } else {
            seen.insert(name, span);
        }
    }
}

pub(crate) fn reject_as_binding_in_irrefutable_context(
    sink: &diagnostics::DiagnosticSink,
    pattern: &Pattern,
) {
    match pattern {
        Pattern::AsBinding { span, .. } => {
            sink.push(diagnostics::infer::as_binding_in_irrefutable_context(*span));
        }
        Pattern::Tuple { elements, .. } => {
            for elem in elements {
                reject_as_binding_in_irrefutable_context(sink, elem);
            }
        }
        Pattern::Struct { fields, .. } => {
            for field in fields {
                reject_as_binding_in_irrefutable_context(sink, &field.value);
            }
        }
        Pattern::Slice { prefix, .. } => {
            for elem in prefix {
                reject_as_binding_in_irrefutable_context(sink, elem);
            }
        }
        Pattern::EnumVariant { fields, .. } => {
            for field in fields {
                reject_as_binding_in_irrefutable_context(sink, field);
            }
        }
        _ => {}
    }
}

pub(crate) fn check_binding_pattern(sink: &diagnostics::DiagnosticSink, pattern: &Pattern) {
    if let Pattern::AsBinding { pattern, .. } = pattern {
        check_binding_pattern(sink, pattern);
        return;
    }

    if matches!(pattern, Pattern::Literal { .. }) {
        sink.push(diagnostics::infer::literal_pattern_in_binding(
            pattern.get_span(),
        ));
    }

    if matches!(pattern, Pattern::Or { .. }) {
        sink.push(diagnostics::infer::or_pattern_in_irrefutable_context(
            pattern.get_span(),
        ));
    }
}

pub(crate) fn check_receiver(
    sink: &diagnostics::DiagnosticSink,
    method: &Expression,
    impl_ty: &Type,
) {
    let Expression::Function { params, .. } = method else {
        return;
    };
    let Some(first_param) = params.first() else {
        return;
    };
    let Pattern::Identifier { identifier, span } = &first_param.pattern else {
        return;
    };

    let receiver_ty = first_param.ty.strip_refs();
    let types_match = receiver_ty == *impl_ty;

    if types_match && identifier != "self" {
        sink.push(diagnostics::infer::receiver_must_be_named_self(
            identifier, *span,
        ));
    }

    if !types_match && identifier == "self" {
        let annotation_span = first_param
            .annotation
            .as_ref()
            .map(|a| a.get_span())
            .unwrap_or_else(|| *span);
        let impl_type_name = impl_ty.get_name().unwrap_or_default();
        let receiver_type_name = receiver_ty.get_name().unwrap_or_default();
        sink.push(diagnostics::infer::receiver_type_mismatch(
            impl_type_name,
            receiver_type_name,
            annotation_span,
        ));
    }
}

pub fn check_interface_visibility(
    store: &crate::store::Store,
    module_id: &str,
    sink: &diagnostics::DiagnosticSink,
) {
    let module = match store.get_module(module_id) {
        Some(m) => m,
        None => return,
    };

    let non_pub_interfaces: HashMap<String, HashSet<String>> = module
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&format!("{}.", module_id)))
        .filter_map(|(_, definition)| {
            if let syntax::program::Definition::Interface {
                visibility: syntax::program::Visibility::Private,
                definition: interface_data,
                ..
            } = definition
            {
                let method_names = interface_data
                    .methods
                    .keys()
                    .map(|k| k.to_string())
                    .collect();
                Some((interface_data.name.to_string(), method_names))
            } else {
                None
            }
        })
        .collect();

    if non_pub_interfaces.is_empty() {
        return;
    }

    for (_, definition) in module
        .definitions
        .iter()
        .filter(|(key, _)| key.starts_with(&format!("{}.", module_id)))
    {
        if let syntax::program::Definition::Struct {
            methods,
            name,
            name_span,
            ..
        } = definition
        {
            for method_name in methods.keys() {
                for (interface_name, interface_methods) in &non_pub_interfaces {
                    if interface_methods.contains(method_name.as_str()) {
                        let method_key = format!("{}.{}.{}", module_id, name, method_name);
                        let method_is_pub = module
                            .definitions
                            .get(method_key.as_str())
                            .map(|definition| definition.visibility().is_public())
                            .unwrap_or(false);

                        if method_is_pub {
                            sink.push(diagnostics::infer::non_pub_interface_with_pub_impl(
                                interface_name,
                                name,
                                *name_span,
                            ));
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Returns generic names that appear inside some `bound.ty` and nowhere else
/// in the signature — not in a parameter, the return type, or as a bound's
/// subject.
fn collect_type_params_only_in_bound(
    generics: &[Generic],
    params: &[Type],
    return_type: &Type,
    bounds: &[Bound],
) -> HashSet<EcoString> {
    let mut unseen_outside_constraint: HashSet<EcoString> =
        generics.iter().map(|g| g.name.clone()).collect();
    for param in params {
        param.remove_found_type_names(&mut unseen_outside_constraint);
    }
    return_type.remove_found_type_names(&mut unseen_outside_constraint);
    for bound in bounds {
        bound
            .generic
            .remove_found_type_names(&mut unseen_outside_constraint);
    }

    let mut unseen_anywhere = unseen_outside_constraint.clone();
    for bound in bounds {
        bound.ty.remove_found_type_names(&mut unseen_anywhere);
    }

    unseen_outside_constraint
        .into_iter()
        .filter(|name| !unseen_anywhere.contains(name))
        .collect()
}
