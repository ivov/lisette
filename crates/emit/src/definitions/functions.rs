use crate::Planner;
use crate::Renderer;
use crate::ReturnContext;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi};
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use crate::patterns::sites::PatternSubject;
use crate::plan::bodies::AsyncKind;
use crate::plan::bodies::{
    LoweredBlock, LoweredStatement, Statement, inherit_line_directives, rename_generated_locals,
};
use crate::plan::cleanup::{clean_up, declare_zero_values};
use crate::plan::go_expression::{
    FunctionLiteralLayout, GoExpressionNode, GoParameter, verbatim_identifiers,
};
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::values::GoExpression;
#[cfg(debug_assertions)]
use crate::plan::verify::verify_final_function_body;
use crate::plan::verify::verify_local_scopes;
use crate::plan::visit::{VisitorMut, identify_body_locals, visit_statements_mut};
use crate::statements::testing::test_context_call;
use crate::types::go_type::GoType;
use crate::utils::{fresh_receiver_name, group_params};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::EcoString;
use syntax::ast::{
    Binding, Expression, FunctionDefinitionView, Pattern, SelectArm, collect_pattern_bindings,
};
use syntax::types::{Type, build_substitution_map, substitute};

pub(crate) fn is_test_context_ty(ty: &Type) -> bool {
    let stripped = ty.strip_refs();
    stripped.get_qualified_id().is_some_and(|id| {
        id.strip_suffix(".TestContext")
            .is_some_and(|package| package == go_name::TEST_PRELUDE_PACKAGE)
    })
}

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

pub(crate) struct LambdaReturnInfo {
    signature: Option<GoType>,
    ctx: ReturnContext,
}

impl LambdaReturnInfo {
    fn should_return(&self) -> bool {
        self.signature.is_some()
    }

    pub(crate) fn signature(&self) -> GoType {
        self.signature.clone().unwrap_or_default()
    }
}

impl Planner<'_> {
    fn emit_function_body_inner(
        &mut self,
        output: &mut String,
        body: &Expression,
        should_return: bool,
        mut prefix: Vec<Statement>,
        parameters: &mut [GoIdentifier],
    ) {
        let mut lowered = self.lower_function_body(body, should_return);
        prefix.append(&mut lowered.statements);
        lowered.statements = prefix;
        let lowered =
            self.finish_function_body(lowered, parameters, should_return, BodyOrigin::Source);
        Renderer.render_lowered_block(output, &lowered);
    }

    /// Resolve locals, clean the body, and verify it before collecting its imports.
    pub(super) fn finish_function_body(
        &mut self,
        mut body: LoweredBlock,
        parameters: &mut [GoIdentifier],
        has_result: bool,
        origin: BodyOrigin,
    ) -> LoweredBlock {
        let sourcemap = self.facts.sourcemap();
        // Cleanup moves statements out of the context they would inherit from.
        if sourcemap {
            inherit_line_directives(&mut body.statements, None);
        }
        let shadowing = identify_body_locals(&mut body.statements, parameters, &mut self.scope);
        clean_up(&mut body.statements, &shadowing, parameters, &self.scope);
        if has_result {
            body.ensure_go_termination();
        }
        declare_zero_values(&mut body.statements, &mut self.scope, self.package);
        if sourcemap {
            inherit_line_directives(&mut body.statements, None);
        }
        if matches!(origin, BodyOrigin::Source) {
            self.settle_generated_names(&mut body.statements, parameters);
        }
        #[cfg(debug_assertions)]
        verify_final_function_body(&mut body, has_result).unwrap_or_else(|error| panic!("{error}"));
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

    fn settle_generated_names(
        &mut self,
        statements: &mut Vec<Statement>,
        parameters: &mut [GoIdentifier],
    ) {
        #[derive(Default)]
        struct Names {
            present: HashMap<LocalId, String>,
            pinned: HashSet<String>,
        }

        impl VisitorMut for Names {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node {
                    if let Some(id) = name.id() {
                        self.present.insert(id, name.to_string());
                    } else {
                        self.pinned.insert(name.to_string());
                    }
                } else if let GoExpressionNode::Verbatim(source) = node {
                    self.pinned
                        .extend(verbatim_identifiers(source).map(str::to_string));
                }
            }

            fn local_binding(&mut self, name: &mut GoIdentifier) {
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

        let mut names = Names::default();
        visit_statements_mut(statements, &mut names);
        let mut by_id =
            self.scope
                .settle_generated_names(&names.present, &names.pinned, self.package);
        self.scope
            .restore_builtin_names(&names.present, &names.pinned, &mut by_id, self.package);
        if by_id.is_empty() {
            return;
        }
        rename_generated_locals(statements, &by_id);
        for parameter in parameters {
            if let Some(name) = parameter.id().and_then(|id| by_id.get(&id)) {
                *parameter.spelling_mut() = name.clone();
            }
        }
    }

    pub(crate) fn emit_lambda(
        &mut self,
        params: &[Binding],
        body: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> GoExpression {
        let return_info = self.lambda_return_info(ty, ctx);
        self.with_isolated_function(return_info.ctx.clone(), |this| {
            let LoweredParams {
                pairs: mut param_pairs,
                destructures,
                test_handle,
                ..
            } = this.lower_parameters(params);

            // The deferred `Recover` needs a real name.
            let test_handle = test_handle.or_else(|| {
                let index = params
                    .iter()
                    .position(|param| is_test_context_ty(&param.ty))
                    .filter(|&index| param_pairs[index].0.spelling() == "_")?;
                let name = this.fresh_declared("lisetteSub");
                let handle = name.clone();
                param_pairs[index].0 = handle.clone();
                Some(handle)
            });

            let recover = test_handle.map(|handle| {
                this.scope.set_test_handle(handle.clone());
                this.require_testkit();
                let span = body.get_span();
                LoweredStatement::Async {
                    kind: AsyncKind::Defer,
                    call: test_context_call(
                        GoExpression::identifier(handle),
                        "Recover",
                        span,
                        Vec::new(),
                    ),
                }
            });

            let mut statements = this.lower_lambda_body_with_deferred(
                body,
                &destructures,
                return_info.should_return(),
            );
            if let Some(recover) = recover {
                statements.insert(0, recover.into());
            }
            let mut body = LoweredBlock { statements };
            let unit_result =
                return_info.should_return() && ty.get_function_ret().is_some_and(Type::is_unit);
            if unit_result && !body.go_terminates() {
                body.statements
                    .push(plain_return(GoExpression::empty_composite(GoType::new(
                        "struct{}",
                    ))));
            }

            GoExpression::function_literal(
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

    /// Lambda Go return-type + `ReturnContext`. Go-prelude generic callbacks
    /// suppress lambda return-type lowering so signature and body agree.
    pub(crate) fn lambda_return_info(
        &mut self,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> LambdaReturnInfo {
        let Type::Function(function) = ty else {
            return LambdaReturnInfo {
                signature: None,
                ctx: ReturnContext::None,
            };
        };

        let return_ty = function.return_type.as_ref();
        let abi = self.function_expression_return_abi(return_ty, ctx);
        let has_return = abi != CallableReturnAbi::Void;
        let return_ctx = match abi {
            CallableReturnAbi::Void => ReturnContext::Void(return_ty.clone()),
            CallableReturnAbi::Value => ReturnContext::Tagged(return_ty.clone()),
            CallableReturnAbi::Lowered(shape) => ReturnContext::Lowered {
                return_ty: return_ty.clone(),
                shape,
            },
        };
        let signature = if has_return {
            match return_ctx.lowered_shape() {
                Some(shape) => Some(
                    self.callable_return_go_type(&CallableReturnAbi::Lowered(shape), return_ty),
                ),
                None => Some(self.go_type(return_ty)),
            }
        } else {
            None
        };

        LambdaReturnInfo {
            signature,
            ctx: return_ctx,
        }
    }

    fn lower_lambda_body_with_deferred(
        &mut self,
        body: &Expression,
        destructures: &[ParamDestructure<'_>],
        should_return: bool,
    ) -> Vec<Statement> {
        let mut statements = self.lower_param_destructures(destructures);
        let body = self.lower_function_body(body, should_return);
        statements.extend(body.statements);
        statements
    }

    pub(crate) fn emit_function(
        &mut self,
        function_definition: FunctionDefinitionView<'_>,
        receiver_ty: Option<&Type>,
        go_name: &str,
        free_function_owner: Option<&Type>,
    ) -> String {
        if function_definition.body.is_none() {
            return String::new();
        }

        let mut generic_context = receiver_ty
            .or(free_function_owner)
            .map(|ty| self.receiver_generic_context(ty))
            .unwrap_or_default();
        let signature_generics_start = if receiver_ty.is_some() {
            generic_context.len()
        } else {
            0
        };
        generic_context.extend(function_definition.generics.iter().map(|generic| {
            let bounds = generic
                .resolved_bounds()
                .expect("generic bounds must be resolved before emission")
                .cloned()
                .collect();
            (generic.name.clone(), bounds)
        }));
        let signature_generics = generic_context[signature_generics_start..].to_vec();
        let directive = self
            .maybe_line_directive(&function_definition.name_span)
            .unwrap_or_default();
        let return_ctx = if receiver_ty.is_some() {
            self.return_context_for_method(function_definition.return_type.clone())
        } else {
            self.return_context_for_type(function_definition.return_type.clone())
        };
        let return_shape = return_ctx.lowered_shape();

        let (self_param, params_to_process) = match receiver_ty {
            Some(_) => {
                let (self_param, rest) = function_definition
                    .params
                    .split_first()
                    .expect("method with a receiver has a self param");
                (Some(self_param), rest)
            }
            None => (None, function_definition.params),
        };

        self.set_type_params(&generic_context);
        self.scope.enter_isolated_function(return_ctx.clone());
        if let Some(body) = function_definition.body {
            self.reserve_source_binder_names(body);
        }

        let mut parts = vec!["func".to_string()];

        if let Some(self_param) = self_param {
            parts.push(self.emit_receiver_part(params_to_process, self_param));
        }

        parts.push(go_name.to_string());

        let generics_str = self.resolved_generics_to_string(&signature_generics);
        if !generics_str.is_empty() {
            parts.push(generics_str);
        }

        let mut body = String::new();
        let (params, deferred_patterns) = self.lower_signature_params(params_to_process);
        let return_ty = self.render_return_ty(function_definition, return_shape.as_ref());
        let mut parameters: Vec<GoIdentifier> = params
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| name.spelling() != "_")
            .collect();

        if let Some(receiver) = self.scope.bound_go_identifier("self") {
            parameters.push(receiver.clone());
        }

        self.emit_function_body_with_deferred_patterns(
            &mut body,
            function_definition,
            &deferred_patterns,
            &mut parameters,
        );
        self.scope.exit_isolated_function();

        let rendered: Vec<(String, String)> = params
            .into_iter()
            .map(|(name, go_type)| {
                let settled = parameters
                    .iter()
                    .find(|parameter| parameter.id().is_some() && parameter.id() == name.id())
                    .unwrap_or(&name);
                (settled.to_string(), go_type)
            })
            .collect();
        parts.push(format!("({})", group_params(&rendered)));
        if !return_ty.is_empty() {
            parts.push(return_ty);
        }
        let signature = parts.join(" ");

        let trimmed_body = body.trim_end();
        let mut function = if trimmed_body.is_empty() {
            format!("{}{} {{}}", directive, signature)
        } else {
            format!("{}{} {{\n{}\n}}", directive, signature, trimmed_body)
        };
        if receiver_ty.is_some()
            && signature_generics.is_empty()
            && self
                .facts
                .method_uses_tagged_return(function_definition.name)
        {
            let twin = self.emit_tagged_twin(
                function_definition,
                go_name,
                &generic_context[..signature_generics_start],
            );
            function.push_str("\n\n");
            function.push_str(twin.trim_end());
        }
        function
    }

    pub(crate) fn pick_go_function_name(
        &self,
        function_definition: FunctionDefinitionView<'_>,
        has_receiver: bool,
        is_public: bool,
    ) -> String {
        if has_receiver {
            self.method_go_name(function_definition.name, is_public)
        } else {
            self.free_function_go_name(function_definition.name, is_public)
        }
    }

    fn lower_signature_params<'a>(
        &mut self,
        params_to_process: &'a [Binding],
    ) -> (Vec<(GoIdentifier, String)>, Vec<ParamDestructure<'a>>) {
        let LoweredParams {
            pairs,
            destructures,
            test_handle,
        } = self.lower_parameters(params_to_process);
        if let Some(handle) = test_handle {
            self.scope.set_test_handle(handle);
        }
        let params = pairs
            .into_iter()
            .map(|(name, go_type)| (name, self.use_rendered_go_type(go_type)))
            .collect();
        (params, destructures)
    }

    fn render_return_ty(
        &mut self,
        function_definition: FunctionDefinitionView<'_>,
        return_shape: Option<&LoweredReturnAbi>,
    ) -> String {
        if matches!(self.return_ctx(), ReturnContext::Void(_)) {
            String::new()
        } else if let Some(shape) = return_shape {
            self.render_callable_return_ty(
                &CallableReturnAbi::Lowered(shape.clone()),
                function_definition.return_type,
            )
        } else {
            self.use_go_type(function_definition.return_type)
        }
    }

    fn emit_function_body_with_deferred_patterns(
        &mut self,
        body: &mut String,
        function_definition: FunctionDefinitionView<'_>,
        deferred_patterns: &[ParamDestructure<'_>],
        parameters: &mut [GoIdentifier],
    ) {
        let should_return = !matches!(self.return_ctx(), ReturnContext::Void(_));
        let prefix = self.lower_param_destructures(deferred_patterns);
        self.emit_function_body_inner(
            body,
            function_definition
                .body
                .expect("declarations return before function body emission"),
            should_return,
            prefix,
            parameters,
        );
    }

    fn emit_receiver_part(
        &mut self,
        params_to_process: &[Binding],
        self_param: &Binding,
    ) -> String {
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

        let ty_string = self.use_go_type(&self_param.ty);
        let receiver_var = fresh_receiver_name(&ty_string, |name| {
            param_names.iter().any(|param| param == name) || self.shadows_declaration(name)
        });

        let receiver_part = format!("({} {})", receiver_var, ty_string);

        let self_id = self_param.pattern.binding_id();
        self.scope
            .bind_source("self", self_id.as_slice(), receiver_var.clone());
        self.declare(&receiver_var);

        receiver_part
    }

    fn receiver_generic_context(&self, receiver_ty: &Type) -> Vec<(EcoString, Vec<Type>)> {
        let stripped = receiver_ty.strip_refs();
        let Type::Nominal { id, params, .. } = &stripped else {
            return Vec::new();
        };
        let Some(generics) = self
            .facts
            .definition(id)
            .and_then(|definition| definition.body.generics())
        else {
            return Vec::new();
        };
        let substitution = build_substitution_map(generics, params);
        generics
            .iter()
            .zip(params)
            .filter_map(|(generic, param)| {
                let Type::Parameter(name) = param else {
                    return None;
                };
                let bounds = generic
                    .resolved_bounds()
                    .expect("generic bounds must be resolved before emission")
                    .map(|bound| substitute(bound, &substitution))
                    .collect();
                Some((name.clone(), bounds))
            })
            .collect()
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
                    let go_name = self.scope.fresh_binding_go_name("ctx", self.package);
                    let name = self.scope.new_local_identifier(go_name);
                    self.declare(&name);
                    let name = self.scope.bind_source(identifier.as_str(), &[], name);
                    lowered.test_handle.get_or_insert_with(|| name.clone());
                    name
                }
                Pattern::Identifier {
                    identifier,
                    binding: id,
                    ..
                } => {
                    if let Some(go_name) = self.go_name_for_binding(&param.pattern) {
                        let name = self.claim_declared_binding(identifier, id.as_slice(), &go_name);
                        lowered.test_handle = lowered
                            .test_handle
                            .take()
                            .or_else(|| is_test_context_ty(&param.ty).then(|| name.clone()));
                        name
                    } else {
                        self.scope
                            .bind_source(identifier.as_str(), id.as_slice(), "_")
                    }
                }
                Pattern::WildCard { .. } => GoIdentifier::external("_".to_string()),
                _ => {
                    let var = self.fresh_declared("arg");
                    lowered
                        .destructures
                        .push((var.clone(), &param.pattern, &param.ty));
                    var.clone()
                }
            };
            lowered.pairs.push((name, self.go_type(&param.ty)));
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

pub(crate) fn is_go_never(expression: &Expression) -> bool {
    match expression {
        Expression::Return { .. } => true,
        Expression::Call { expression, .. } => {
            matches!(&**expression, Expression::Identifier { value, .. } if value == "panic")
        }
        _ => false,
    }
}

fn collect_binder_names(expression: &Expression, out: &mut Vec<SourceName>) {
    match expression {
        Expression::Let { binding, .. } => push_binder_names(&binding.pattern, out),
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
        Expression::For { binding, .. } => push_binder_names(&binding.pattern, out),
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
