use super::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, IfPlan, LoopHeader, LoweredStatement,
    SelectArmPlan, SwitchCasePlan, SwitchKind,
};
use super::go_expression::GoExpressionNode;
use super::local::GoIdentifier;
use super::local::LocalId;
use super::values::ValuePlan;
use crate::state::scope::ScopeState;
use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

pub(crate) trait VisitorMut {
    fn expression(&mut self, node: &mut GoExpressionNode);
    fn binding(&mut self, name: &mut String);

    fn enter_scope(&mut self) {}

    fn exit_scope(&mut self) {}

    fn local_binding(&mut self, name: &mut GoIdentifier) {
        self.binding(name.spelling_mut());
    }
}

pub(crate) fn visit_statements_mut(
    statements: &mut [LoweredStatement],
    visitor: &mut impl VisitorMut,
) {
    for statement in statements {
        visit_statement(statement, visitor);
    }
}

fn visit_expression(node: &mut GoExpressionNode, visitor: &mut impl VisitorMut) {
    visitor.expression(node);
    if let GoExpressionNode::FunctionLiteral {
        parameters, body, ..
    } = node
    {
        visitor.enter_scope();
        for parameter in parameters {
            visitor.local_binding(&mut parameter.name);
        }
        visit_statements_mut(&mut body.statements, visitor);
        visitor.exit_scope();
    } else {
        node.visit_children_mut(&mut |child| visit_expression(child, visitor));
    }
}

fn visit_definition(definition: &mut Definition, visitor: &mut impl VisitorMut) {
    visit_expression(definition.value.node_mut(), visitor);
    for name in &mut definition.names {
        visitor.local_binding(name);
    }
}

fn visit_value(value: &mut ValuePlan, visitor: &mut impl VisitorMut) {
    visit_statements_mut(&mut value.setup, visitor);
    visit_expression(value.expression.node_mut(), visitor);
}

fn visit_if(plan: &mut IfPlan, visitor: &mut impl VisitorMut) {
    visit_statements_mut(&mut plan.condition_setup, visitor);
    visitor.enter_scope();
    if let Some(initializer) = &mut plan.initializer {
        visit_definition(initializer, visitor);
    }
    visit_expression(plan.condition.node_mut(), visitor);
    visitor.enter_scope();
    visit_statements_mut(&mut plan.then_body.statements, visitor);
    visitor.exit_scope();
    match &mut plan.else_arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => visit_if(plan, visitor),
        ElseArm::Else {
            body,
            inline: false,
        } => {
            visitor.enter_scope();
            visit_statements_mut(&mut body.statements, visitor);
            visitor.exit_scope();
        }
        ElseArm::Else { body, inline: true } => {
            visitor.exit_scope();
            visit_statements_mut(&mut body.statements, visitor);
            return;
        }
    }
    visitor.exit_scope();
}

fn visit_case(case: &mut SwitchCasePlan, visitor: &mut impl VisitorMut) {
    for label in &mut case.labels {
        visit_expression(label.node_mut(), visitor);
    }
    visit_statements_mut(&mut case.body.statements, visitor);
}

fn visit_statement(statement: &mut LoweredStatement, visitor: &mut impl VisitorMut) {
    match statement {
        LoweredStatement::If(plan) => visit_if(plan, visitor),
        LoweredStatement::Loop(plan) => {
            visit_statements_mut(&mut plan.prologue, visitor);
            visitor.enter_scope();
            match &mut plan.header {
                LoopHeader::Infinite => {}
                LoopHeader::While(condition) => visit_expression(condition.node_mut(), visitor),
                LoopHeader::Range {
                    key,
                    value,
                    iterable,
                } => {
                    visit_expression(iterable.node_mut(), visitor);
                    for name in key.iter_mut().chain(value.iter_mut()) {
                        visitor.local_binding(name);
                    }
                }
                LoopHeader::Counted {
                    variable,
                    start,
                    condition,
                } => {
                    visit_expression(start.node_mut(), visitor);
                    visitor.local_binding(variable);
                    if let Some(condition) = condition {
                        visit_expression(condition.node_mut(), visitor);
                    }
                }
            }
            visitor.enter_scope();
            visit_statements_mut(&mut plan.body.statements, visitor);
            visitor.exit_scope();
            visitor.exit_scope();
        }
        LoweredStatement::Block(body) => {
            visitor.enter_scope();
            visit_statements_mut(&mut body.statements, visitor);
            visitor.exit_scope();
        }
        LoweredStatement::Body(body) | LoweredStatement::WhileLet(body) => {
            visit_statements_mut(&mut body.statements, visitor)
        }
        LoweredStatement::Break(_)
        | LoweredStatement::Continue(_)
        | LoweredStatement::UnreachablePanic => {}
        LoweredStatement::Const(plan) => {
            visit_expression(plan.value.node_mut(), visitor);
            visitor.local_binding(&mut plan.name);
        }
        LoweredStatement::Return(values) => {
            for value in values {
                visit_expression(value.node_mut(), visitor);
            }
        }
        LoweredStatement::Assign(form) => match form {
            AssignForm::Compound {
                target_capture,
                target,
                kind,
            } => {
                visit_statements_mut(target_capture, visitor);
                visit_expression(target.node_mut(), visitor);
                match kind {
                    CompoundKind::OpAssign {
                        rhs, pinned_left, ..
                    } => {
                        visit_value(rhs, visitor);
                        if let Some(left) = pinned_left {
                            visit_expression(left.node_mut(), visitor);
                        }
                    }
                    CompoundKind::Increment | CompoundKind::Decrement => {}
                }
            }
            AssignForm::Simple {
                target_capture,
                target,
                value,
            } => {
                visit_statements_mut(target_capture, visitor);
                visit_expression(target.node_mut(), visitor);
                visit_value(value, visitor);
            }
        },
        LoweredStatement::Async { call, .. } => visit_expression(call.node_mut(), visitor),
        LoweredStatement::Select(plan) => {
            for arm in &mut plan.arms {
                match arm {
                    SelectArmPlan::Receive {
                        receive_vars,
                        channel,
                        body,
                    } => {
                        visit_expression(channel.node_mut(), visitor);
                        visitor.enter_scope();
                        for name in receive_vars {
                            visitor.local_binding(name);
                        }
                        visit_statements_mut(&mut body.statements, visitor);
                        visitor.exit_scope();
                    }
                    SelectArmPlan::Send {
                        channel,
                        value,
                        body,
                    } => {
                        visit_expression(channel.node_mut(), visitor);
                        visit_expression(value.node_mut(), visitor);
                        visitor.enter_scope();
                        visit_statements_mut(&mut body.statements, visitor);
                        visitor.exit_scope();
                    }
                    SelectArmPlan::Default { body } => {
                        visitor.enter_scope();
                        visit_statements_mut(&mut body.statements, visitor);
                        visitor.exit_scope();
                    }
                }
            }
        }
        LoweredStatement::Switch(plan) => {
            visitor.enter_scope();
            match &mut plan.kind {
                SwitchKind::Conditional => {}
                SwitchKind::Value { subject } => visit_expression(subject.node_mut(), visitor),
                SwitchKind::Type { subject, binding } => {
                    visit_expression(subject.node_mut(), visitor);
                    if let Some(name) = binding {
                        visitor.local_binding(name);
                    }
                }
            }
            for case in &mut plan.cases {
                visitor.enter_scope();
                visit_case(case, visitor);
                visitor.exit_scope();
            }
            if let Some(default) = &mut plan.default {
                visitor.enter_scope();
                visit_statements_mut(&mut default.statements, visitor);
                visitor.exit_scope();
            }
            visitor.exit_scope();
            visit_statements_mut(&mut plan.postlude, visitor);
        }
        LoweredStatement::Define(definition) => visit_definition(definition, visitor),
        LoweredStatement::AssignMany { targets, value } => {
            for target in targets {
                visit_expression(target.node_mut(), visitor);
            }
            visit_expression(value.node_mut(), visitor);
        }
        LoweredStatement::VarDecl { name, value, .. } => {
            if let Some(value) = value {
                visit_expression(value.node_mut(), visitor);
            }
            visitor.local_binding(name);
        }
        LoweredStatement::Discard(expression)
        | LoweredStatement::ExpressionStatement { expression, .. } => {
            visit_expression(expression.node_mut(), visitor)
        }
        LoweredStatement::Directed { inner, .. } => visit_statement(inner, visitor),
    }
}

pub(crate) fn identify_body_locals(
    statements: &mut [LoweredStatement],
    parameters: &[GoIdentifier],
    scope: &mut ScopeState,
) -> HashSet<LocalId> {
    #[derive(Clone, Copy)]
    struct Local {
        id: LocalId,
        provisional: bool,
    }

    struct Resolve<'a> {
        scope: &'a mut ScopeState,
        frames: Vec<HashMap<String, Local>>,
        shadowing: HashSet<LocalId>,
        corrections: HashMap<LocalId, LocalId>,
        declared: HashSet<LocalId>,
    }

    impl VisitorMut for Resolve<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            let GoExpressionNode::Identifier(name) = node else {
                return;
            };
            if name.id().is_none() && !name.is_pending() {
                return;
            }
            if let Some((frame_index, local)) =
                self.frames
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(index, frame)| {
                        frame
                            .get(name.spelling())
                            .copied()
                            .map(|local| (index, local))
                    })
            {
                match name.id() {
                    None => name.resolve_to(local.id),
                    Some(id) if id != local.id && local.provisional => {
                        let already_bound = self
                            .frames
                            .iter()
                            .any(|frame| frame.values().any(|candidate| candidate.id == id));
                        if !already_bound {
                            self.corrections.insert(local.id, id);
                            let active = self.frames[frame_index]
                                .get_mut(name.spelling())
                                .expect("the local was found above");
                            active.id = id;
                            active.provisional = false;
                            self.declared.insert(id);
                        }
                    }
                    Some(id) if id != local.id => {
                        let already_bound = self
                            .frames
                            .iter()
                            .any(|frame| frame.values().any(|candidate| candidate.id == id));
                        if !already_bound {
                            name.resolve_to(local.id);
                        }
                    }
                    Some(_) => {}
                }
            } else if name.is_pending()
                && let Some(id) = self.scope.generated_local_id(name.spelling())
            {
                name.resolve_to(id);
            }
        }

        fn binding(&mut self, _name: &mut String) {}

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if name.spelling() == "_" {
                return;
            }
            let current = self
                .frames
                .last()
                .expect("a local scope exists")
                .get(name.spelling())
                .copied();
            let known = name
                .id()
                .or_else(|| self.scope.generated_local_id(name.spelling()));
            let local = current.unwrap_or_else(|| Local {
                id: known.unwrap_or_else(|| self.scope.new_local_id()),
                provisional: known.is_none(),
            });
            let id = local.id;
            if name.id() != Some(id) {
                name.resolve_to(id);
            }
            if self.frames[..self.frames.len() - 1]
                .iter()
                .rev()
                .any(|frame| {
                    frame
                        .get(name.spelling())
                        .is_some_and(|outer| outer.id != id)
                })
            {
                self.shadowing.insert(id);
            }
            self.frames
                .last_mut()
                .expect("a local scope exists")
                .insert(name.spelling().to_string(), local);
            self.declared.insert(id);
        }

        fn enter_scope(&mut self) {
            self.frames.push(HashMap::default());
        }

        fn exit_scope(&mut self) {
            self.frames.pop().expect("a local scope exists");
        }
    }

    let mut resolver = Resolve {
        scope,
        frames: vec![HashMap::default()],
        shadowing: HashSet::default(),
        corrections: HashMap::default(),
        declared: HashSet::default(),
    };
    for name in parameters {
        let mut name = name.clone();
        resolver.local_binding(&mut name);
    }
    visit_statements_mut(statements, &mut resolver);
    let corrections = resolver.corrections;
    let shadowing = resolver
        .shadowing
        .into_iter()
        .map(|id| corrected_id(id, &corrections))
        .collect();
    if !corrections.is_empty() {
        struct Correct<'a>(&'a HashMap<LocalId, LocalId>);
        impl VisitorMut for Correct<'_> {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node
                    && let Some(id) = name.id()
                {
                    name.resolve_to(corrected_id(id, self.0));
                }
            }

            fn binding(&mut self, _name: &mut String) {}

            fn local_binding(&mut self, name: &mut GoIdentifier) {
                if let Some(id) = name.id() {
                    name.resolve_to(corrected_id(id, self.0));
                }
            }
        }
        visit_statements_mut(statements, &mut Correct(&corrections));
    }
    struct Finish;
    impl VisitorMut for Finish {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::Identifier(name) = node {
                name.finish();
            }
        }

        fn binding(&mut self, _name: &mut String) {}

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            name.finish();
        }
    }
    visit_statements_mut(statements, &mut Finish);
    shadowing
}

fn corrected_id(mut id: LocalId, corrections: &HashMap<LocalId, LocalId>) -> LocalId {
    while let Some(corrected) = corrections.get(&id) {
        id = *corrected;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::bodies::{LoweredBlock, define, rename_generated_locals};
    use crate::plan::go_expression::{FunctionLiteralLayout, GoParameter};
    use crate::plan::local::{GoIdentifier, LocalId};
    use crate::plan::values::GoExpression;
    use crate::state::scope::ScopeState;

    fn name(value: &str) -> GoExpression {
        GoExpression::name(value.to_string())
    }

    fn function(parameter: &str, local: &str) -> GoExpression {
        GoExpression::function_literal(
            vec![GoParameter::new(parameter, "int")],
            "int".to_string(),
            LoweredBlock {
                statements: vec![
                    define(local.to_string(), name(parameter)),
                    LoweredStatement::Return(vec![name(local)]),
                ],
            },
            FunctionLiteralLayout::MultiLine,
        )
    }

    fn identifier_id(expression: &GoExpression) -> Option<LocalId> {
        let GoExpressionNode::Identifier(name) = expression.node() else {
            panic!("expected a local reference");
        };
        name.id()
    }

    #[test]
    fn local_resolution_follows_lexical_shadowing() {
        let mut statements = vec![
            define("value".to_string(), GoExpression::literal("1".to_string())),
            LoweredStatement::Block(LoweredBlock {
                statements: vec![
                    define("value".to_string(), name("value")),
                    LoweredStatement::Return(vec![name("value")]),
                ],
            }),
            LoweredStatement::Return(vec![name("value")]),
        ];
        let shadowing = identify_body_locals(&mut statements, &[], &mut ScopeState::new());
        let LoweredStatement::Define(outer) = &statements[0] else {
            panic!("expected outer binding");
        };
        let LoweredStatement::Block(inner) = &statements[1] else {
            panic!("expected inner block");
        };
        let LoweredStatement::Define(inner_binding) = &inner.statements[0] else {
            panic!("expected inner binding");
        };
        let LoweredStatement::Return(inner_return) = &inner.statements[1] else {
            panic!("expected inner return");
        };
        let LoweredStatement::Return(outer_return) = &statements[2] else {
            panic!("expected outer return");
        };
        assert_eq!(identifier_id(&inner_binding.value), outer.names[0].id());
        assert_eq!(identifier_id(&inner_return[0]), inner_binding.names[0].id());
        assert_eq!(identifier_id(&outer_return[0]), outer.names[0].id());
        assert!(shadowing.contains(&inner_binding.names[0].id().unwrap()));
    }

    #[test]
    fn local_resolution_gives_an_unidentified_binding_its_read_id() {
        let mut statements = vec![
            define("result".to_string(), GoExpression::literal("1".to_string())),
            LoweredStatement::Return(vec![GoExpression::identifier(GoIdentifier::local(
                "result".to_string(),
                LocalId(42),
            ))]),
        ];
        let mut scope = ScopeState::new();
        identify_body_locals(&mut statements, &[], &mut scope);
        let LoweredStatement::Define(binding) = &statements[0] else {
            panic!("expected a binding");
        };
        let LoweredStatement::Return(values) = &statements[1] else {
            panic!("expected a return");
        };
        assert_eq!(identifier_id(&values[0]), binding.names[0].id());
        assert_eq!(binding.names[0].id(), Some(LocalId(42)));
    }

    #[test]
    fn local_resolution_keeps_an_outer_id_when_an_inner_name_shadows_it() {
        use crate::plan::verify::{BodyErrorKind, verify_local_scopes};

        let mut scope = ScopeState::new();
        let outer = GoIdentifier::local("value".to_string(), scope.new_local_id());
        let mut statements = vec![
            define(outer.clone(), GoExpression::literal("1".to_string())),
            LoweredStatement::Block(LoweredBlock {
                statements: vec![
                    define("value".to_string(), GoExpression::literal("2".to_string())),
                    LoweredStatement::Return(vec![GoExpression::identifier(outer.clone())]),
                ],
            }),
        ];
        identify_body_locals(&mut statements, &[], &mut scope);
        let LoweredStatement::Block(inner) = &statements[1] else {
            panic!("expected inner block");
        };
        let LoweredStatement::Return(values) = &inner.statements[1] else {
            panic!("expected inner read");
        };
        assert_eq!(identifier_id(&values[0]), outer.id());
        assert_eq!(
            verify_local_scopes(&mut statements, &[]).unwrap_err().kind,
            BodyErrorKind::ShadowedLocalReference
        );
    }

    #[test]
    fn source_alias_uses_the_generated_go_binding_id() {
        let mut scope = ScopeState::new();
        let generated = scope.fresh_go_name(Some("value"));
        let go_binding = scope.generated_identifier(&generated);
        let mut statements = vec![
            define(go_binding.clone(), GoExpression::literal("1".to_string())),
            LoweredStatement::Return(vec![GoExpression::identifier(GoIdentifier::local(
                generated,
                LocalId(42),
            ))]),
        ];
        identify_body_locals(&mut statements, &[], &mut scope);
        let LoweredStatement::Return(values) = &statements[1] else {
            panic!("expected a return");
        };
        assert_eq!(identifier_id(&values[0]), go_binding.id());
    }

    #[test]
    fn external_reference_does_not_take_an_unused_generated_id() {
        use crate::plan::verify::verify_local_scopes;

        let mut scope = ScopeState::new();
        scope.fresh_go_name(Some("check"));
        let mut statements = vec![LoweredStatement::Return(vec![GoExpression::external_name(
            "check".to_string(),
        )])];
        identify_body_locals(&mut statements, &[], &mut scope);
        let LoweredStatement::Return(values) = &statements[0] else {
            panic!("expected a return");
        };
        assert_eq!(identifier_id(&values[0]), None);
        assert!(verify_local_scopes(&mut statements, &[]).is_ok());
    }

    fn rename_suffix(statements: &mut [LoweredStatement]) {
        use rustc_hash::FxHashMap as HashMap;

        let mut bindings = HashMap::default();
        let mut ids = HashMap::default();
        struct Collect<'a> {
            bindings: &'a mut HashMap<String, String>,
            ids: &'a mut HashMap<String, LocalId>,
        }
        impl VisitorMut for Collect<'_> {
            fn expression(&mut self, _node: &mut GoExpressionNode) {}

            fn binding(&mut self, _name: &mut String) {}

            fn local_binding(&mut self, name: &mut GoIdentifier) {
                if let Some(final_name) = name.spelling().strip_suffix("_1").map(str::to_string) {
                    let id = LocalId(self.ids.len() as u32);
                    self.ids.insert(name.to_string(), id);
                    self.bindings.insert(name.to_string(), final_name);
                    name.identify(id);
                }
            }
        }
        visit_statements_mut(
            statements,
            &mut Collect {
                bindings: &mut bindings,
                ids: &mut ids,
            },
        );
        struct Identify<'a>(&'a HashMap<String, LocalId>);
        impl VisitorMut for Identify<'_> {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node
                    && let Some(id) = self.0.get(name.spelling())
                {
                    name.identify(*id);
                }
            }

            fn binding(&mut self, _name: &mut String) {}
        }
        visit_statements_mut(statements, &mut Identify(&ids));
        let by_id = bindings
            .iter()
            .filter_map(|(old, new)| ids.get(old).map(|id| (*id, new.clone())))
            .collect();
        rename_generated_locals(statements, &by_id);
        struct ForgetIds;
        impl VisitorMut for ForgetIds {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node {
                    *name = GoIdentifier::name(name.spelling().to_string());
                }
            }

            fn binding(&mut self, _name: &mut String) {}

            fn local_binding(&mut self, name: &mut GoIdentifier) {
                *name = GoIdentifier::name(name.spelling().to_string());
            }
        }
        visit_statements_mut(statements, &mut ForgetIds);
    }

    #[test]
    fn renaming_keeps_nested_function_declarations_and_reads_together() {
        let mut statements = vec![define(
            "callback_1".to_string(),
            function("arg_1", "local_1"),
        )];
        rename_suffix(&mut statements);
        assert_eq!(
            statements,
            vec![define("callback".to_string(), function("arg", "local"))]
        );
    }

    #[test]
    fn generated_rename_changes_only_the_matching_reference_id() {
        use rustc_hash::FxHashMap as HashMap;

        let mut statements = vec![LoweredStatement::Return(vec![
            GoExpression::from_node(GoExpressionNode::Identifier(GoIdentifier::local(
                "tmp_1".to_string(),
                LocalId(1),
            ))),
            GoExpression::from_node(GoExpressionNode::Identifier(GoIdentifier::local(
                "tmp_1".to_string(),
                LocalId(2),
            ))),
        ])];
        let by_id = HashMap::from_iter([(LocalId(1), "tmp".to_string())]);
        rename_generated_locals(&mut statements, &by_id);
        let LoweredStatement::Return(values) = &statements[0] else {
            panic!("expected return");
        };
        assert_eq!(values[0].rendered(), "tmp");
        assert_eq!(values[1].rendered(), "tmp_1");
    }

    #[test]
    fn generated_rename_changes_only_the_matching_definition_id() {
        use rustc_hash::FxHashMap as HashMap;

        let mut statements = vec![
            define("tmp_1".to_string(), GoExpression::literal("1".to_string())),
            define("tmp_1".to_string(), GoExpression::literal("2".to_string())),
        ];
        for (statement, id) in statements.iter_mut().zip([LocalId(1), LocalId(2)]) {
            let LoweredStatement::Define(definition) = statement else {
                panic!("expected definition");
            };
            definition.names[0].identify(id);
        }
        let by_id = HashMap::from_iter([(LocalId(1), "tmp".to_string())]);
        rename_generated_locals(&mut statements, &by_id);
        let [
            LoweredStatement::Define(first),
            LoweredStatement::Define(second),
        ] = statements.as_slice()
        else {
            panic!("expected two definitions");
        };
        assert_eq!(first.names[0].spelling(), "tmp");
        assert_eq!(second.names[0].spelling(), "tmp_1");
    }

    #[test]
    fn visitation_reaches_nested_function_names_once() {
        #[derive(Default)]
        struct Names {
            bindings: Vec<String>,
            reads: Vec<String>,
        }
        impl VisitorMut for Names {
            fn expression(&mut self, node: &mut GoExpressionNode) {
                if let GoExpressionNode::Identifier(name) = node {
                    self.reads.push(name.to_string());
                }
            }
            fn binding(&mut self, name: &mut String) {
                self.bindings.push(name.clone());
            }
        }
        let mut statements = vec![define("callback".to_string(), function("arg", "local"))];
        let mut names = Names::default();
        visit_statements_mut(&mut statements, &mut names);
        assert_eq!(names.bindings, ["arg", "local", "callback"]);
        assert_eq!(names.reads, ["arg", "local"]);
    }

    #[test]
    fn renaming_reaches_each_else_if_initializer() {
        let mut statements = vec![LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: Some(Definition::single("first_1".to_string(), name("source"))),
            condition: name("first_1"),
            then_body: LoweredBlock {
                statements: Vec::new(),
            },
            else_arm: ElseArm::ElseIf(Box::new(IfPlan {
                condition_setup: Vec::new(),
                initializer: Some(Definition::single("second_1".to_string(), name("source"))),
                condition: name("second_1"),
                then_body: LoweredBlock {
                    statements: Vec::new(),
                },
                else_arm: ElseArm::None,
            })),
        })];
        rename_suffix(&mut statements);
        let LoweredStatement::If(first) = &statements[0] else {
            panic!("expected if");
        };
        assert_eq!(first.initializer.as_ref().unwrap().names, ["first"]);
        assert_eq!(first.condition, name("first"));
        let ElseArm::ElseIf(second) = &first.else_arm else {
            panic!("expected else-if");
        };
        assert_eq!(second.initializer.as_ref().unwrap().names, ["second"]);
        assert_eq!(second.condition, name("second"));
    }
}
