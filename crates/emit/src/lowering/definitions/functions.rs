use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::queries::go_name_for_binding;
use crate::lookup::facts::reusable_bindings::ReusableBindings;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::return_abi;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::interface_adapter::{MethodParameters, has_tagged_counterpart};
use crate::lowering::patterns::sites::PatternSubject;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::asserts::test_context_call;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::output::finish::cleanup::{clean_up, declare_zero_values, drop_unreachable_statements};
use crate::output::finish::function_literals::function_literal;
use crate::output::finish::hygiene::{
    apply_go_hygiene, blank_unused_parameters, drop_unused_locals,
};
use crate::output::finish::locals::identify_body_locals;
use crate::output::finish::verify::verify_local_scopes;
use crate::output::finish::verify::{BodyResult, verify_final_function_body, verify_go_names};
use crate::target::ir::bodies::AsyncKind;
use crate::target::ir::bodies::async_statement;
use crate::target::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, inherit_line_directives, rename_generated_locals,
};
use crate::target::ir::declarations::{DeclarationKind, FunctionSignature, GoDeclaration};
use crate::target::ir::go_expression::{
    FunctionLiteralLayout, GoExpressionNode, GoParameter, verbatim_identifiers,
};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name;
use crate::target::names::receiver::fresh_receiver_name;
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi};
use rustc_hash::{FxHashMap, FxHashSet};
use std::iter::once;
use syntax::EcoString;
use syntax::ast::{
    Binding, Expression, FunctionDefinitionView, Pattern, SelectArm, collect_pattern_bindings,
};
use syntax::types::Type;

type ParamDestructure<'a> = (GoIdentifier, &'a Pattern, &'a Type);

struct LoweredParams<'a> {
    pairs: Vec<(GoIdentifier, GoType)>,
    destructures: Vec<ParamDestructure<'a>>,
    test_handle: Option<GoIdentifier>,
}

pub(super) enum BodyOrigin {
    Source,
    Adapter,
}

pub(in crate::lowering) struct LambdaReturnInfo {
    signature: Option<GoType>,
    ctx: ReturnContext,
}

impl LambdaReturnInfo {
    fn body_result(&self) -> BodyResult {
        match self.signature {
            Some(_) => BodyResult::Value,
            None => BodyResult::Void,
        }
    }

    pub(in crate::lowering) fn signature(&self) -> GoType {
        self.signature.clone().unwrap_or_default()
    }
}

impl Planner<'_> {
    fn lower_function_body_inner(
        &mut self,
        body: &Expression,
        result: BodyResult,
        mut prefix: Vec<Statement>,
        parameters: &mut [GoIdentifier],
    ) -> LoweredBlock {
        let mut lowered = self.lower_function_body(body, result);
        prefix.append(&mut lowered.statements);
        lowered.statements = prefix;

        self.finish_function_body(lowered, parameters, result, BodyOrigin::Source)
    }

    pub(super) fn finish_function_body(
        &mut self,
        mut body: LoweredBlock,
        parameters: &mut [GoIdentifier],
        result: BodyResult,
        origin: BodyOrigin,
    ) -> LoweredBlock {
        let sourcemap = self.facts.sourcemap();
        if sourcemap {
            inherit_line_directives(&mut body.statements, None);
        }
        drop_unreachable_statements(&mut body.statements);
        let shadowing = identify_body_locals(&mut body.statements, parameters, &mut self.scope);
        drop_unused_locals(&mut body.statements);
        clean_up(&mut body.statements, &shadowing, parameters, &self.scope);
        declare_zero_values(&mut body.statements, &mut self.scope, self.names);
        apply_go_hygiene(&mut body, result);
        if sourcemap {
            inherit_line_directives(&mut body.statements, None);
        }
        if matches!(origin, BodyOrigin::Source) {
            self.queries()
                .settle_generated_names(&mut body.statements, parameters);
        }
        if cfg!(debug_assertions) {
            verify_final_function_body(&mut body, result).unwrap_or_else(|error| panic!("{error}"));
            verify_go_names(&mut body.statements, parameters)
                .unwrap_or_else(|error| panic!("{error}"));
        }
        verify_local_scopes(&mut body.statements, parameters)
            .unwrap_or_else(|error| panic!("{error}"));
        self.collect_imports(&body.statements);
        body
    }

    fn reserve_source_binder_names(&mut self, body: &Expression) {
        let mut names: Vec<SourceName> = Vec::new();
        collect_binder_names(body, &mut names);
        for name in names {
            self.scope.reserve_go_name(&name.preferred());
        }
    }

    pub(in crate::lowering) fn plan_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let return_info = self.queries().lambda_return_info(ty, ctx);
        self.with_isolated_function(return_info.ctx.clone(), |this| {
            let LoweredParams {
                pairs: mut param_pairs,
                destructures,
                test_handle,
                ..
            } = this.lower_parameters(params);

            let recover = this.lambda_test_recover(params, &mut param_pairs, test_handle, body);

            let mut statements = this.lower_lambda_body_with_deferred(
                body,
                &destructures,
                return_info.body_result(),
            );
            if let Some(recover) = recover {
                statements.insert(0, recover);
            }
            let mut body = LoweredBlock { statements };
            let unit_result = return_info.body_result() == BodyResult::Value
                && ty
                    .get_function_ret()
                    .is_some_and(|return_ty| this.facts.resolves_to_unit(return_ty));
            if unit_result && !body.go_terminates() {
                let unit = this.unit_value(None, ty.get_function_ret());
                body.statements.push(plain_return(unit));
            }

            function_literal(
                param_pairs
                    .into_iter()
                    .map(|(name, go_type)| GoParameter::new(name, go_type))
                    .collect(),
                return_info.signature(),
                body,
                FunctionLiteralLayout::MultiLine,
            )
        })
    }

    fn lambda_test_recover(
        &mut self,
        params: &[Binding],
        param_pairs: &mut [(GoIdentifier, GoType)],
        test_handle: Option<GoIdentifier>,
        body: &Expression,
    ) -> Option<Statement> {
        let handle = test_handle.or_else(|| {
            let index = params
                .iter()
                .position(|param| param.ty.is_test_context())
                .filter(|&index| param_pairs[index].0.spelling() == "_")?;
            let handle = self.fresh_declared("lisetteSub");
            param_pairs[index].0 = handle.clone();
            Some(handle)
        })?;
        self.scope.set_test_handle(handle.clone());
        self.require_testkit();
        Some(async_statement(
            AsyncKind::Defer,
            test_context_call(
                GoExpression::identifier(handle),
                "Recover",
                body.get_span(),
                Vec::new(),
            ),
        ))
    }

    fn lower_lambda_body_with_deferred(
        &mut self,
        body: &Expression,
        destructures: &[ParamDestructure<'_>],
        result: BodyResult,
    ) -> Vec<Statement> {
        let mut statements = self.lower_param_destructures(destructures);
        let body = self.lower_function_body(body, result);
        statements.extend(body.statements);
        statements
    }

    pub(in crate::lowering) fn emit_function(
        &mut self,
        function_definition: FunctionDefinitionView<'_>,
        receiver_ty: Option<&Type>,
        go_name: &str,
        owner_generics: Option<&[(EcoString, Vec<Type>)]>,
    ) -> Vec<GoDeclaration> {
        let Some(function_body) = function_definition.body else {
            return Vec::new();
        };
        let (generic_context, signature_generics_start) =
            function_generic_context(owner_generics, receiver_ty, function_definition);
        let return_ctx = self.function_return_context(function_definition, receiver_ty);
        let return_shape = return_ctx.lowered_shape();

        let (method_parameters, params_to_process) =
            split_method_parameters(function_definition.params, receiver_ty);

        self.enter_function_scope(
            &generic_context,
            return_ctx,
            function_definition,
            function_body,
        );

        let receiver =
            method_parameters.map(|method| self.receiver_parameter(method.rest, method.receiver));
        let generics = self
            .queries()
            .plan_generics(&generic_context[signature_generics_start..]);
        let (signature_parameters, deferred_patterns) =
            self.lower_signature_params(params_to_process);
        let return_ty = self
            .queries()
            .render_return_ty(function_definition, return_shape.as_ref());
        let (body, parameters) = self.lower_and_exit_function_body(
            function_body,
            &deferred_patterns,
            &signature_parameters,
            receiver.as_ref(),
        );

        let signature = FunctionSignature {
            receiver: receiver.map(|receiver| declared_parameter(&parameters, receiver)),
            name: go_name.to_string(),
            generics,
            parameters: declared_parameters(&parameters, signature_parameters),
            result: return_ty,
        };

        let mut declaration = GoDeclaration::new(DeclarationKind::Function { signature, body });
        declaration.position = self.facts.source_position(&function_definition.name_span);
        let counterpart = self.tagged_counterpart(
            function_definition,
            receiver_ty,
            method_parameters,
            go_name,
            &generic_context[..signature_generics_start],
        );
        once(declaration).chain(counterpart).collect()
    }

    fn lower_and_exit_function_body(
        &mut self,
        function_body: &Expression,
        deferred_patterns: &[ParamDestructure<'_>],
        signature_parameters: &[(GoIdentifier, GoType)],
        receiver: Option<&GoParameter>,
    ) -> (LoweredBlock, Vec<GoIdentifier>) {
        let mut parameters = named_parameters(signature_parameters);
        if let Some(receiver) = self.scope.bound_go_identifier("self") {
            parameters.push(receiver.clone());
        }
        let mut body = self.lower_function_body_with_deferred_patterns(
            function_body,
            deferred_patterns,
            &mut parameters,
        );
        self.scope.exit_isolated_function();
        let receiver_name = receiver.map(|receiver| &receiver.name);
        blank_unused_parameters(&mut body, &mut parameters, receiver_name);
        (body, parameters)
    }

    fn enter_function_scope(
        &mut self,
        generic_context: &[(EcoString, Vec<Type>)],
        return_ctx: ReturnContext,
        function_definition: FunctionDefinitionView<'_>,
        function_body: &Expression,
    ) {
        self.set_type_params(generic_context);
        self.scope.enter_isolated_function(return_ctx);
        self.reserve_source_binder_names(function_body);
        self.scope
            .set_reusable_bindings(ReusableBindings::of_function(
                function_definition.params,
                function_body,
                &self.facts,
            ));
    }

    fn function_return_context(
        &self,
        function_definition: FunctionDefinitionView<'_>,
        receiver_ty: Option<&Type>,
    ) -> ReturnContext {
        let return_type = function_definition.return_type.clone();
        match receiver_ty {
            Some(_) => self.return_context_for_method(return_type),
            None => self.return_context_for_type(return_type),
        }
    }

    fn tagged_counterpart(
        &mut self,
        function_definition: FunctionDefinitionView<'_>,
        receiver_ty: Option<&Type>,
        method_parameters: Option<MethodParameters<'_>>,
        go_name: &str,
        owner_generics: &[(EcoString, Vec<Type>)],
    ) -> Option<GoDeclaration> {
        let method_parameters = method_parameters?;
        let owner = receiver_ty.and_then(Type::get_qualified_id)?;
        if !has_tagged_counterpart(&self.facts, owner, function_definition.name) {
            return None;
        }
        Some(self.emit_tagged_counterpart(
            function_definition,
            method_parameters,
            go_name,
            owner_generics,
        ))
    }

    fn lower_signature_params<'a>(
        &mut self,
        params_to_process: &'a [Binding],
    ) -> (Vec<(GoIdentifier, GoType)>, Vec<ParamDestructure<'a>>) {
        let LoweredParams {
            pairs,
            destructures,
            test_handle,
        } = self.lower_parameters(params_to_process);
        if let Some(handle) = test_handle {
            self.scope.set_test_handle(handle);
        }
        (pairs, destructures)
    }

    fn lower_function_body_with_deferred_patterns(
        &mut self,
        body: &Expression,
        deferred_patterns: &[ParamDestructure<'_>],
        parameters: &mut [GoIdentifier],
    ) -> LoweredBlock {
        let result = match self.scope.current_return_ctx() {
            ReturnContext::Void(_) => BodyResult::Void,
            _ => BodyResult::Value,
        };
        let prefix = self.lower_param_destructures(deferred_patterns);
        self.lower_function_body_inner(body, result, prefix, parameters)
    }

    fn receiver_parameter(
        &mut self,
        params_to_process: &[Binding],
        self_param: &Binding,
    ) -> GoParameter {
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

        let go_type = self.queries().go_type(&self_param.ty);
        let receiver_var = fresh_receiver_name(go_type.leading_name(), |name| {
            param_names.iter().any(|param| param == name)
                || self.queries().shadows_declaration(name)
        });

        let self_id = self_param.pattern.binding_id();
        let receiver = self
            .scope
            .bind_new_local("self", self_id.as_slice(), receiver_var.clone());
        self.scope.declare_go_name(&receiver_var);

        GoParameter::new(receiver, go_type)
    }

    fn lower_parameters<'a>(&mut self, params: &'a [Binding]) -> LoweredParams<'a> {
        let mut lowered = LoweredParams {
            pairs: Vec::with_capacity(params.len()),
            destructures: Vec::new(),
            test_handle: None,
        };
        for param in params {
            let name = match &param.pattern {
                Pattern::Identifier {
                    identifier,
                    binding: None,
                    ..
                } if identifier == go_name::TEST_CTX_PARAM => {
                    let go_name = self.scope.fresh_binding_go_name("ctx", self.names);
                    let name = self.scope.new_local_identifier(go_name);
                    self.scope.declare_go_name(&name);
                    let name = self.scope.bind_new_local(identifier.as_str(), &[], name);
                    lowered.test_handle.get_or_insert_with(|| name.clone());
                    name
                }
                Pattern::Identifier {
                    identifier,
                    binding: id,
                    ..
                } => {
                    if let Some(go_name) = go_name_for_binding(&param.pattern) {
                        let name = self.claim_declared_binding(identifier, id.as_slice(), &go_name);
                        lowered.test_handle = lowered
                            .test_handle
                            .take()
                            .or_else(|| param.ty.is_test_context().then(|| name.clone()));
                        name
                    } else {
                        self.scope
                            .bind_new_local(identifier.as_str(), id.as_slice(), "_")
                    }
                }
                Pattern::WildCard { .. } => GoIdentifier::blank(),
                _ => {
                    let var = self.fresh_declared("arg");
                    lowered
                        .destructures
                        .push((var.clone(), &param.pattern, &param.ty));
                    var.clone()
                }
            };

            lowered
                .pairs
                .push((name, self.queries().go_type(&param.ty)));
        }
        lowered
    }

    fn lower_param_destructures(
        &mut self,
        destructures: &[ParamDestructure<'_>],
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        for (temp_name, pattern, param_ty) in destructures {
            statements.extend(self.lower_irrefutable_pattern_site(
                PatternSubject::for_identifier(temp_name.clone()),
                pattern,
                param_ty,
            ));
        }
        statements
    }
}

fn collect_binder_names(expression: &Expression, out: &mut Vec<SourceName>) {
    match expression {
        Expression::Lambda { params, .. } => {
            for param in params {
                push_binder_names(&param.pattern, out);
            }
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            push_binder_names(pattern, out)
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive { binding, .. } => push_binder_names(binding, out),
                    SelectArm::MatchReceive { arms, .. } => {
                        for arm in arms {
                            push_binder_names(&arm.pattern, out);
                        }
                    }
                    SelectArm::Send { .. } | SelectArm::WildCard { .. } => {}
                }
            }
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                push_binder_names(&arm.pattern, out);
            }
        }
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            push_binder_names(&binding.pattern, out)
        }
        _ => {}
    }
    for child in expression.children() {
        collect_binder_names(child, out);
    }
}

fn push_binder_names(pattern: &Pattern, out: &mut Vec<SourceName>) {
    out.extend(
        collect_pattern_bindings(pattern)
            .into_iter()
            .map(|(name, _)| SourceName::new(&name.into())),
    );
}

pub(in crate::lowering) fn is_go_never(facts: &EmitFacts<'_>, expression: &Expression) -> bool {
    match expression {
        Expression::Return { .. } => true,
        Expression::Call { expression, .. } => {
            facts.checked(expression).callee().definition.as_deref() == Some("prelude.panic")
        }
        _ => false,
    }
}

#[derive(Default)]
struct BodyNames {
    present: FxHashMap<LocalId, String>,
    pinned: FxHashSet<String>,
    qualifiers: FxHashSet<String>,
}

impl VisitorMut for BodyNames {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = node {
            if let Some(id) = name.id() {
                self.present.insert(id, name.to_string());
            } else {
                self.pinned.insert(name.to_string());
            }
        } else if let GoExpressionNode::Qualified { package, .. } = node {
            self.qualifiers.insert(package.qualifier().to_string());
        } else if let GoExpressionNode::Verbatim(source) = node {
            self.pinned
                .extend(verbatim_identifiers(source).map(str::to_string));
        }
    }

    fn local_binding(&mut self, name: &mut GoIdentifier, _site: BindingSite) {
        if let Some(id) = name.id() {
            self.present.insert(id, name.to_string());
        } else {
            self.pinned.insert(name.to_string());
        }
    }

    fn statements(&mut self, statements: &mut Vec<Statement>) {
        if statements
            .iter()
            .any(|statement| matches!(statement.kind, LoweredStatement::UnreachablePanic))
        {
            self.pinned.insert("panic".to_string());
        }
    }
}

impl PlannerQueries<'_, '_> {
    fn settle_generated_names(
        &self,
        statements: &mut Vec<Statement>,
        parameters: &mut [GoIdentifier],
    ) {
        let mut names = BodyNames::default();

        visit_statements_mut(statements, &mut names);
        let mut by_id =
            self.scope
                .settle_generated_names(&names.present, &names.pinned, self.names);
        self.scope
            .restore_builtin_names(&names.present, &names.pinned, &mut by_id, self.names);
        self.scope.avoid_qualifier_capture(
            &names.present,
            &names.pinned,
            &names.qualifiers,
            &mut by_id,
        );
        if by_id.is_empty() {
            return;
        }
        rename_generated_locals(statements, &by_id);
        for parameter in parameters {
            if let Some(name) = parameter.id().and_then(|id| by_id.get(&id)) {
                parameter.spelling_mut().clone_from(name);
            }
        }
    }

    fn lambda_return_info(&self, ty: &Type, ctx: ExpressionContext<'_>) -> LambdaReturnInfo {
        let Type::Function(function) = ty else {
            return LambdaReturnInfo {
                signature: None,
                ctx: ReturnContext::None,
            };
        };

        let return_ty = function.return_type.as_ref();
        let abi = return_abi(self.facts, return_ty, ReturnSlot::FunctionExpression(ctx));
        let signature =
            (abi != CallableReturnAbi::Void).then(|| self.return_go_type(&abi, return_ty));
        let return_ctx = match abi {
            CallableReturnAbi::Void => ReturnContext::Void(return_ty.clone()),
            CallableReturnAbi::Value => ReturnContext::Tagged(return_ty.clone()),
            CallableReturnAbi::Lowered(shape) => ReturnContext::Lowered {
                return_ty: return_ty.clone(),
                shape,
            },
        };

        LambdaReturnInfo {
            signature,
            ctx: return_ctx,
        }
    }

    fn render_return_ty(
        &self,
        function_definition: FunctionDefinitionView<'_>,
        return_shape: Option<&LoweredReturnAbi>,
    ) -> GoType {
        let abi = match (self.scope.current_return_ctx(), return_shape) {
            (ReturnContext::Void(_), _) => CallableReturnAbi::Void,
            (_, Some(shape)) => CallableReturnAbi::Lowered(shape.clone()),
            (_, None) => CallableReturnAbi::Value,
        };
        self.return_go_type(&abi, function_definition.return_type)
    }
}

fn function_generic_context(
    owner_generics: Option<&[(EcoString, Vec<Type>)]>,
    receiver_ty: Option<&Type>,
    function_definition: FunctionDefinitionView<'_>,
) -> (Vec<(EcoString, Vec<Type>)>, usize) {
    let mut generic_context = owner_generics.map(<[_]>::to_vec).unwrap_or_default();
    let signature_generics_start = match receiver_ty {
        Some(_) => generic_context.len(),
        None => 0,
    };
    generic_context.extend(function_definition.generics.iter().map(generic_bounds));
    (generic_context, signature_generics_start)
}

fn split_method_parameters<'a>(
    params: &'a [Binding],
    receiver_ty: Option<&Type>,
) -> (Option<MethodParameters<'a>>, &'a [Binding]) {
    match receiver_ty {
        Some(_) => {
            let (receiver, rest) = params
                .split_first()
                .expect("method with a receiver has a self param");
            (Some(MethodParameters { receiver, rest }), rest)
        }
        None => (None, params),
    }
}

fn named_parameters(signature_parameters: &[(GoIdentifier, GoType)]) -> Vec<GoIdentifier> {
    signature_parameters
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| name.spelling() != "_")
        .collect()
}

fn declared_parameters(
    parameters: &[GoIdentifier],
    signature_parameters: Vec<(GoIdentifier, GoType)>,
) -> Vec<GoParameter> {
    signature_parameters
        .into_iter()
        .map(|(name, go_type)| declared_parameter(parameters, GoParameter::new(name, go_type)))
        .collect()
}

fn declared_parameter(parameters: &[GoIdentifier], parameter: GoParameter) -> GoParameter {
    let name = parameters
        .iter()
        .find(|declared| declared.id().is_some() && declared.id() == parameter.name.id())
        .unwrap_or(&parameter.name)
        .clone();
    GoParameter::new(name, parameter.go_type)
}
