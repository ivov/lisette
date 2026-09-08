use crate::patterns::binding_decls::pattern_has_bindings;
use crate::plan::bodies::GoUses;
use crate::plan::go_expression::GoExpressionNode;

use syntax::ast::{Expression, MatchArm, Pattern, Span};
use syntax::types::Type;

use crate::Planner;
use crate::calls::comma_ok::{CommaOkValueSlot, PairCondition};
use crate::context::expression::ExpressionContext;
use crate::names::go_name::{self, GeneratedPackage, testkit_qualifier};
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::binding_emit::{
    apply_refutable_root_assertion, apply_root_assertion, compose_refutable_condition,
    tree_assignment_statements, tree_binding_statements, with_tree_bindings,
};
use crate::patterns::decision_tree::{
    self, PatternBinding, PatternInfo, SubjectRoot, render_condition,
};
use crate::patterns::matching::{ArmBinding, field_binding, ok_pattern_field, some_pattern_field};
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopTransfer, LoweredBlock, LoweredStatement, PlacePlan, define,
    discard, expression_statement,
};
use crate::plan::go_expression::CompositeLayout;
use crate::plan::values::GoExpression;
use crate::state::bindings::BindingValue;
use rustc_hash::FxHashSet as HashSet;

#[derive(Clone, Copy)]
pub(crate) struct AnnotatedPattern<'a> {
    pub(crate) pattern: &'a Pattern,
}

#[derive(Clone, Copy)]
pub(crate) struct TypedSubject<'a> {
    pub(crate) var: &'a GoExpression,
    pub(crate) ty: &'a Type,
}

#[derive(Clone, Copy)]
enum RefutableFail<'a> {
    ElseBlock(&'a Expression),
    AssertFail(Span),
}

pub(crate) enum PatternSubject<'a> {
    /// Already in a Go variable named by the caller.
    Existing { var: GoExpression },
    /// Pattern-site picks: inline the scrutinee identifier when safe, else
    /// hoist into a fresh temp with the given hint.
    Expression {
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'a str>,
    },
}

impl<'a> PatternSubject<'a> {
    pub(crate) fn for_value(var: impl Into<String>) -> Self {
        Self::Existing {
            var: GoExpression::name(var.into()),
        }
    }

    pub(crate) fn expression(
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'a str>,
    ) -> Self {
        Self::Expression {
            scrutinee,
            pattern,
            temp_hint,
        }
    }
}

/// For composite scrutinees, the declaration is deferred so the caller can
/// pick `var := expr` vs `_ = expr` based on body usage.
struct ResolvedSubject {
    subject: GoExpression,
    pending: Option<PendingSubject>,
}

struct PendingSubject {
    name: String,
    expression: GoExpression,
}

impl ResolvedSubject {
    fn existing(subject: GoExpression) -> Self {
        Self {
            subject,
            pending: None,
        }
    }

    fn var(&self) -> &GoExpression {
        &self.subject
    }

    fn push_declaration(self, statements: &mut Vec<LoweredStatement>, body: &[LoweredStatement]) {
        if let Some(PendingSubject { name, expression }) = self.pending {
            if GoUses::of(body).contains(&name) {
                statements.push(define(name, expression));
            } else {
                statements.push(discard(expression));
            }
        }
    }
}

struct RefutableAlternative {
    info: PatternInfo,
    subject: GoExpression,
    ok_test: Option<GoExpression>,
}

/// The identifier under a chain of field accesses.
fn field_path_root(expression: &Expression) -> Option<&str> {
    let mut expression = expression.unwrap_parens();
    while let Expression::DotAccess {
        expression: inner, ..
    } = expression
    {
        expression = inner.unwrap_parens();
    }
    match expression {
        Expression::Identifier { value, .. } => Some(value),
        _ => None,
    }
}

fn is_field_path(expression: &GoExpression) -> bool {
    let GoExpressionNode::Selector { base, .. } = expression.node() else {
        return false;
    };
    let mut base = base.as_ref();
    while let GoExpressionNode::Selector { base: inner, .. } = base {
        base = inner;
    }
    matches!(base, GoExpressionNode::Identifier(_))
}

fn loop_break(planner: &Planner) -> LoweredStatement {
    LoweredStatement::Break(
        planner
            .current_loop_id()
            .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source),
    )
}

impl Planner<'_> {
    pub(crate) fn can_reuse_subject_identifier(&self, value: &str, binds_name: bool) -> bool {
        !binds_name
            && !value.contains('.')
            && !matches!(
                self.scope.resolve_identifier_binding(value),
                Some(BindingValue::InlineExpr(_))
            )
    }

    /// A field path such as `t.status` is read where it sits when nothing rebinds its root.
    pub(crate) fn field_path_reads_in_place(
        &self,
        subject: &Expression,
        expression: &GoExpression,
        binds_root: impl Fn(&str) -> bool,
    ) -> bool {
        let Some(root) = field_path_root(subject) else {
            return false;
        };
        is_field_path(expression) && self.can_reuse_subject_identifier(root, binds_root(root))
    }

    fn resolve_pattern_subject(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        subject: PatternSubject<'_>,
    ) -> ResolvedSubject {
        match subject {
            PatternSubject::Existing { var } => ResolvedSubject::existing(var),
            PatternSubject::Expression {
                scrutinee,
                pattern,
                temp_hint,
            } => {
                if let Expression::Identifier { value, .. } = scrutinee
                    && self.can_reuse_subject_identifier(value, pattern_binds_name(pattern, value))
                {
                    let var = self.reference_go_name(value);
                    return ResolvedSubject::existing(GoExpression::name(var));
                }
                let plan = self.lower_value(scrutinee, ExpressionContext::value());
                let rests_in_stable_name = self.plan_rests_in_stable_name(&plan);
                let (op_setup, expression) = plan.into_parts();
                setup.extend(op_setup);
                if rests_in_stable_name
                    || self.field_path_reads_in_place(scrutinee, &expression, |root| {
                        pattern_binds_name(pattern, root)
                    })
                {
                    return ResolvedSubject::existing(expression);
                }
                let name = self.fresh_var(temp_hint);
                self.declare(&name);
                ResolvedSubject {
                    subject: GoExpression::name(name.clone()),
                    pending: Some(PendingSubject { name, expression }),
                }
            }
        }
    }

    /// Lower an irrefutable pattern site (no branching): subject setup +
    /// declaration, root type assertion, per-field binding leaves.
    pub(crate) fn lower_irrefutable_pattern_site(
        &mut self,
        subject: PatternSubject<'_>,
        pattern: &Pattern,
        subject_ty: &Type,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        let resolved = self.resolve_pattern_subject(&mut statements, subject);
        let info = decision_tree::collect_pattern_info(self, pattern, subject_ty);
        self.require_packages(&info.packages);

        let mut body = Vec::new();
        let effective = apply_root_assertion(self, &mut body, &info, resolved.var());
        tree_binding_statements(self, &mut body, &info.bindings, &effective, &[]);

        resolved.push_declaration(&mut statements, &body);
        statements.extend(body);
        statements
    }

    pub(crate) fn lower_let_else_pattern_site(
        &mut self,
        ap: AnnotatedPattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_let_site(ap, scrutinee, RefutableFail::ElseBlock(else_block))
    }

    pub(crate) fn lower_let_assert_pattern_site(
        &mut self,
        ap: AnnotatedPattern,
        scrutinee: &Expression,
        pattern_span: Span,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_let_site(ap, scrutinee, RefutableFail::AssertFail(pattern_span))
    }

    fn lower_refutable_let_site(
        &mut self,
        ap: AnnotatedPattern,
        scrutinee: &Expression,
        fail: RefutableFail,
    ) -> Vec<LoweredStatement> {
        if let RefutableFail::ElseBlock(else_block) = fail {
            if let Some(statements) =
                self.lower_fused_option_let_else(ap.pattern, scrutinee, else_block)
            {
                return statements;
            }
            if let Some(statements) =
                self.lower_fused_result_let_else(ap.pattern, scrutinee, else_block)
            {
                return statements;
            }
        }

        let value_ty = scrutinee.get_type();
        let mut statements = Vec::new();
        let resolved = self.resolve_pattern_subject(
            &mut statements,
            PatternSubject::expression(scrutinee, ap.pattern, Some("subject")),
        );
        let subject = TypedSubject {
            var: resolved.var(),
            ty: &value_ty,
        };

        let subject_is_fixed = self.is_unmutated_identifier(scrutinee);
        let body = if matches!(ap.pattern, Pattern::Or { .. }) {
            self.lower_let_else_or_pattern(ap, subject, fail)
        } else {
            self.lower_let_else_single_pattern(ap, subject, fail, subject_is_fixed)
        };

        resolved.push_declaration(&mut statements, &body);
        statements.extend(body);
        statements
    }

    /// Fuse `let Ok(x) = <Go (T, error) call> else { ... }` into a direct error test.
    fn lower_fused_result_let_else(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<LoweredStatement>> {
        let field = ok_pattern_field(pattern)?;
        let fuse = self.result_fuse_plan(scrutinee)?;
        if !fuse.carries_payload() {
            return None;
        }

        let binding = self.declare_fused_binding(field);
        let slot = match &binding {
            Some((_, go_name)) => CommaOkValueSlot::Named(go_name.clone()),
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot, None);
        let fail_condition = self.pair_failure_condition(&bound);
        Some(self.finish_fused_let_else(bound.statements, fail_condition, binding, else_block))
    }

    /// Fuse `let Some(x) = <lowered Option source> else { ... }` into a direct
    /// physical-ABI test.
    fn lower_fused_option_let_else(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<LoweredStatement>> {
        let fuse = self.option_fuse_plan(scrutinee)?;
        let field = some_pattern_field(pattern)?;

        let binding = self.declare_fused_binding(field);
        let slot = match &binding {
            Some((_, go_name)) => CommaOkValueSlot::Named(go_name.clone()),
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot);
        let none_condition = bound.none_condition(self);
        let late_binding = bound.late_binding();
        let mut statements =
            self.finish_fused_let_else(bound.statements, none_condition, binding, else_block);
        statements.extend(late_binding);
        Some(statements)
    }

    fn finish_fused_let_else(
        &mut self,
        mut statements: Vec<LoweredStatement>,
        fail_condition: PairCondition,
        binding: Option<(String, String)>,
        else_block: &Expression,
    ) -> Vec<LoweredStatement> {
        // The else block sees the enclosing scope, so it lowers before the binding installs.
        let fail_body = self.lower_block_as_body(else_block);
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: fail_condition.initializer,
            condition: fail_condition.condition,
            then_body: fail_body,
            else_arm: ElseArm::None,
        }));
        if let Some((name, go_name)) = binding {
            self.scope.bind(name, go_name);
        }
        statements
    }

    fn declare_fused_binding(&mut self, pattern: &Pattern) -> Option<(String, String)> {
        self.go_name_for_binding(pattern).map(|name| {
            let escaped = go_name::escape_reserved(&name).into_owned();
            let go_name = if self.shadows_declaration(&escaped) {
                self.fresh_var(Some(&name))
            } else {
                escaped
            };
            self.declare(&go_name);
            (name, go_name)
        })
    }

    /// Resolve a while-let scrutinee to its loop-subject var, returning any
    /// setup statements (none when the scrutinee is an inlinable identifier).
    fn while_let_subject(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
    ) -> (GoExpression, Vec<LoweredStatement>) {
        if let Expression::Identifier { value, .. } = scrutinee {
            let has_collision = pattern_binds_name(pattern, value);
            if self.can_reuse_subject_identifier(value, has_collision) {
                return (
                    GoExpression::name(self.reference_go_name(value)),
                    Vec::new(),
                );
            }
        }
        let staged = self.plan_operand(scrutinee, ExpressionContext::value());
        let (mut setup, value) = staged.into_parts();
        if self
            .field_path_reads_in_place(scrutinee, &value, |root| pattern_binds_name(pattern, root))
        {
            return (value, setup);
        }
        let var = self.fresh_var(Some("subject"));
        setup.push(define(var.clone(), value));
        (GoExpression::name(var), setup)
    }

    pub(crate) fn lower_while_let(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        body: &Expression,
    ) -> LoweredBlock {
        if let Some(fused) = self.lower_fused_option_while_let(pattern, scrutinee, body) {
            return fused;
        }

        let scrutinee_ty = scrutinee.get_type();
        let (subject_var, subject_setup) = self.while_let_subject(pattern, scrutinee);

        // Or-patterns with bindings lower to an `if/else if` chain whose
        // terminal `else` breaks the loop.
        if let Pattern::Or { patterns, .. } = pattern
            && pattern_has_bindings(pattern)
        {
            let mut loop_body = subject_setup;
            self.lower_while_let_or_pattern(
                &mut loop_body,
                patterns,
                TypedSubject {
                    var: &subject_var,
                    ty: &scrutinee_ty,
                },
                body,
            );
            let plan = self.build_source_loop(
                Vec::new(),
                LoopHeader::Infinite,
                LoweredBlock {
                    statements: loop_body,
                },
            );
            return LoweredBlock {
                statements: vec![LoweredStatement::Loop(plan)],
            };
        }

        let info = decision_tree::collect_pattern_info(self, pattern, &scrutinee_ty);
        self.require_packages(&info.packages);
        let mut loop_body = subject_setup;
        let (effective, ok_test) =
            apply_refutable_root_assertion(self, &mut loop_body, &info, &subject_var);
        let condition = compose_refutable_condition(ok_test.as_ref(), &info.checks, &effective);

        let then_body = self.with_scope(|this| {
            let mut then_body: Vec<LoweredStatement> = Vec::new();
            if !matches!(pattern, Pattern::Or { .. }) {
                tree_binding_statements(this, &mut then_body, &info.bindings, &effective, &[body]);
            }
            then_body.extend(this.lower_block_as_body(body).statements);
            then_body
        });

        loop_body.push(LoweredStatement::If(IfPlan::plain(
            condition,
            LoweredBlock {
                statements: then_body,
            },
            ElseArm::from_body(
                LoweredBlock {
                    statements: vec![loop_break(self)],
                },
                false,
            ),
        )));

        let plan = self.build_source_loop(
            Vec::new(),
            LoopHeader::Infinite,
            LoweredBlock {
                statements: loop_body,
            },
        );
        LoweredBlock {
            statements: vec![LoweredStatement::Loop(plan)],
        }
    }

    /// Fuse `while let Some(x) = <lowered Option source>` into a per-iteration
    /// physical-ABI test.
    fn lower_fused_option_while_let(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        body: &Expression,
    ) -> Option<LoweredBlock> {
        let fuse = self.option_fuse_plan(scrutinee)?;
        let field = some_pattern_field(pattern)?;
        let binding = field_binding(field).filter(|b| *b != "_");

        let slot = if binding.is_some() {
            CommaOkValueSlot::Temp
        } else {
            CommaOkValueSlot::Unused
        };
        let bound = fuse.bind(self, slot);
        let none_condition = bound.none_condition(self);
        let value = bound.value();

        let mut loop_body = bound.statements;
        loop_body.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer: none_condition.initializer,
            condition: none_condition.condition,
            then_body: LoweredBlock {
                statements: vec![loop_break(self)],
            },
            else_arm: ElseArm::None,
        }));
        let (body_block, _) = self.lower_fused_arm(
            &[ArmBinding::copy(binding, value.as_ref())],
            body,
            &PlacePlan::Statement,
        );
        loop_body.extend(body_block.statements);

        let plan = self.build_source_loop(
            Vec::new(),
            LoopHeader::Infinite,
            LoweredBlock {
                statements: loop_body,
            },
        );
        Some(LoweredBlock {
            statements: vec![LoweredStatement::Loop(plan)],
        })
    }

    fn lower_refutable_fail(
        &mut self,
        fail: RefutableFail,
        subject_var: &GoExpression,
    ) -> LoweredBlock {
        match fail {
            RefutableFail::ElseBlock(else_block) => self.lower_block_as_body(else_block),
            RefutableFail::AssertFail(span) => {
                let handle = self
                    .current_test_handle()
                    .expect("let assert without a test handle should be rejected by semantics");
                self.require_testkit();
                let testkit = testkit_qualifier();
                let literal = |text: String| GoExpression::literal(text);
                let operand = GoExpression::composite(
                    Some(format!("{testkit}.Operand")),
                    vec![(
                        Some(GoExpression::name("Value".to_string())),
                        GoExpression::call(
                            GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                            vec![subject_var.clone()],
                        ),
                    )],
                    CompositeLayout::Inline { padded: false },
                );
                let call = GoExpression::call(
                    GoExpression::selector(GoExpression::name(handle), "FailAssert".to_string()),
                    vec![
                        literal(span.file_id.to_string()),
                        literal(span.byte_offset.to_string()),
                        literal((span.byte_offset + span.byte_length).to_string()),
                        literal("\"let_assert\"".to_string()),
                        literal("\"pattern did not match\"".to_string()),
                        operand,
                    ],
                );
                LoweredBlock {
                    statements: vec![expression_statement(call)],
                }
            }
        }
    }

    fn lower_let_else_single_pattern(
        &mut self,
        ap: AnnotatedPattern,
        subject: TypedSubject,
        fail: RefutableFail,
        subject_is_fixed: bool,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let mut info = decision_tree::collect_pattern_info(self, pattern, subject_ty);
        self.require_packages(&info.packages);

        let mut statements = Vec::new();
        let (effective_subject, assert_ok) =
            apply_refutable_root_assertion(self, &mut statements, &info, subject_var);

        if subject_is_fixed {
            info.checks.retain(|check| {
                !self.is_condition_established(&check.render(SubjectRoot::Var(&effective_subject)))
            });
        }

        if info.checks.is_empty() && assert_ok.is_none() {
            tree_binding_statements(
                self,
                &mut statements,
                &info.bindings,
                &effective_subject,
                &[],
            );
            return statements;
        }

        let mut guard_parts: Vec<GoExpression> = Vec::new();
        if let Some(ok) = assert_ok {
            guard_parts.push(GoExpression::unary("!", ok));
        }
        if !info.checks.is_empty() {
            let negated = match info.checks.as_slice() {
                [check] => check.render_negated(SubjectRoot::Var(&effective_subject)),
                _ => GoExpression::unary(
                    "!",
                    render_condition(&info.checks, SubjectRoot::Var(&effective_subject)),
                ),
            };
            guard_parts.push(negated);
        }
        let guard = guard_parts
            .into_iter()
            .reduce(|left, right| GoExpression::binary(left, "||", right))
            .expect("a refutable pattern has a check or a type assertion");
        let fail_body = self.lower_refutable_fail(fail, subject_var);
        statements.push(LoweredStatement::If(IfPlan::plain(
            guard,
            fail_body,
            ElseArm::None,
        )));

        tree_binding_statements(
            self,
            &mut statements,
            &info.bindings,
            &effective_subject,
            &[],
        );
        statements
    }

    fn lower_let_else_or_pattern(
        &mut self,
        ap: AnnotatedPattern,
        subject: TypedSubject,
        fail: RefutableFail,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let Pattern::Or { patterns, .. } = pattern else {
            unreachable!("lower_let_else_or_pattern requires an Or pattern");
        };
        let mut infos: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, subject_ty))
            .collect();
        share_unused_alternative_bindings(&mut infos);
        let failure = infos
            .iter()
            .all(|info| !info.checks.is_empty() || info.root_assertion.is_some())
            .then(|| self.lower_refutable_fail(fail, subject_var));

        let mut statements = Vec::new();
        self.declare_alternative_bindings(&mut statements, &infos[0].bindings);
        let alternatives = self.prepare_refutable_alternatives(&mut statements, infos, subject_var);
        let mut pieces = Vec::new();
        for alternative in alternatives {
            let RefutableAlternative {
                info,
                subject,
                ok_test,
            } = alternative;
            let mut assigns = Vec::new();
            tree_assignment_statements(self, &mut assigns, &info.bindings, &subject);
            let body = LoweredBlock {
                statements: assigns,
            };
            if info.checks.is_empty() && ok_test.is_none() {
                if pieces.is_empty() {
                    statements.extend(body.statements);
                } else {
                    statements.push(assemble_if_else_chain(pieces, body));
                }
                return statements;
            }
            let condition = compose_refutable_condition(ok_test.as_ref(), &info.checks, &subject);
            pieces.push((condition, body));
        }
        statements.push(assemble_if_else_chain(
            pieces,
            failure.expect("all alternatives are refutable"),
        ));
        statements
    }

    fn declare_alternative_bindings(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        bindings: &[PatternBinding],
    ) {
        for binding in bindings {
            let Some(go_name) = &binding.go_name else {
                self.scope.bind(&binding.lisette_name, "_");
                continue;
            };
            let Some(ty) = &binding.ty else {
                continue;
            };
            let go_name = self.claim_declared_binding(&binding.lisette_name, go_name.clone());
            let go_type = self.use_go_type(ty);
            statements.push(LoweredStatement::VarDecl {
                name: go_name,
                go_type,
                value: None,
            });
        }
    }

    fn prepare_refutable_alternatives(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        infos: Vec<PatternInfo>,
        subject: &GoExpression,
    ) -> Vec<RefutableAlternative> {
        infos
            .into_iter()
            .map(|info| {
                self.require_packages(&info.packages);
                let (subject, ok_test) =
                    apply_refutable_root_assertion(self, statements, &info, subject);
                RefutableAlternative {
                    info,
                    subject,
                    ok_test,
                }
            })
            .collect()
    }

    /// Lower a binding or-pattern while-let body into `statements`: per-
    /// alternative root assertions, then an `if/else if` chain whose terminal
    /// `else` breaks the loop.
    fn lower_while_let_or_pattern(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        patterns: &[Pattern],
        subject: TypedSubject,
        body: &Expression,
    ) {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let mut alternatives: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, subject_ty))
            .collect();
        share_unused_alternative_bindings(&mut alternatives);

        let alternatives =
            self.prepare_refutable_alternatives(statements, alternatives, subject_var);
        let mut pieces = Vec::with_capacity(alternatives.len());
        for alternative in alternatives {
            let RefutableAlternative {
                info,
                subject: effective,
                ok_test,
            } = alternative;
            let condition = compose_refutable_condition(ok_test.as_ref(), &info.checks, &effective);

            let branch = self.with_scope(|this| {
                let mut branch = Vec::new();
                with_tree_bindings(
                    this,
                    &mut branch,
                    &info.bindings,
                    &effective,
                    body,
                    |this, branch| {
                        branch.extend(this.lower_block_as_body(body).statements);
                    },
                );
                branch
            });

            pieces.push((condition, LoweredBlock { statements: branch }));
        }

        let terminal = LoweredBlock {
            statements: vec![loop_break(self)],
        };
        statements.push(assemble_if_else_chain(pieces, terminal));
    }

    pub(crate) fn lower_select_receive_pattern_site(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        body: &Expression,
        default_body: Option<&Expression>,
        place: &PlacePlan,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_arm(subject, ap, body, place, |this| {
            default_body.map(|default| this.lower_block_to_place(default, place))
        })
    }

    pub(crate) fn lower_select_match_receive_some_site(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        some_body: &Expression,
        match_arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_arm(subject, ap, some_body, place, |this| {
            Some(lower_none_arm_body(this, match_arms, place))
        })
    }

    /// Lower a refutable site whose checks gate `body` into structured IR. The
    /// `failure` callback produces the `else` block (run only on the guarded
    /// path) for the caller's failure continuation; `None` means no `else`.
    fn lower_refutable_arm(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        body: &Expression,
        place: &PlacePlan,
        failure: impl FnOnce(&mut Planner) -> Option<LoweredBlock>,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = decision_tree::collect_pattern_info(self, pattern, subject_ty);
        self.require_packages(&info.packages);
        let mut statements = Vec::new();
        let (effective, ok_test) =
            apply_refutable_root_assertion(self, &mut statements, &info, subject_var);

        if info.checks.is_empty() && ok_test.is_none() {
            with_tree_bindings(
                self,
                &mut statements,
                &info.bindings,
                &effective,
                body,
                |this, statements| {
                    let block = this.lower_block_to_place(body, place);
                    statements.extend(block.statements);
                },
            );
            return statements;
        }
        let condition = compose_refutable_condition(ok_test.as_ref(), &info.checks, &effective);
        let mut then_body = Vec::new();
        with_tree_bindings(
            self,
            &mut then_body,
            &info.bindings,
            &effective,
            body,
            |this, then_body| {
                let block = this.lower_block_to_place(body, place);
                then_body.extend(block.statements);
            },
        );
        let else_arm = match failure(self) {
            Some(body) => ElseArm::from_body(body, false),
            None => ElseArm::None,
        };
        statements.push(LoweredStatement::If(IfPlan::plain(
            condition,
            LoweredBlock {
                statements: then_body,
            },
            else_arm,
        )));
        statements
    }
}

pub(crate) fn lower_none_arm_body(
    planner: &mut Planner,
    match_arms: &[MatchArm],
    place: &PlacePlan,
) -> LoweredBlock {
    for match_arm in match_arms {
        if let Pattern::EnumVariant { identifier, .. } = &match_arm.pattern {
            let variant_name = go_name::unqualified_name(identifier);
            if variant_name == "None" {
                return planner.lower_block_to_place(&match_arm.expression, place);
            }
        }
    }
    LoweredBlock {
        statements: Vec::new(),
    }
}

/// Peel `Some(inner)` to expose `inner`; returns the original pattern when
/// the outer is not `Some(_)`.
pub(crate) fn unwrap_some_pattern(pattern: &Pattern) -> &Pattern {
    let pattern = peel_as_binding(pattern);
    some_payload_pattern(pattern).unwrap_or(pattern)
}

pub(crate) fn some_payload_pattern(pattern: &Pattern) -> Option<&Pattern> {
    let Pattern::EnumVariant {
        identifier, fields, ..
    } = pattern
    else {
        return None;
    };
    match (go_name::unqualified_name(identifier), fields.as_slice()) {
        ("Some", [payload]) => Some(payload),
        _ => None,
    }
}

fn peel_as_binding(pattern: &Pattern) -> &Pattern {
    match pattern {
        Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
        p => p,
    }
}

impl Planner<'_> {
    /// Map a `Some(pattern)` payload to a case-variable name and whether the
    /// payload needs decision-tree destructuring inside the arm body (rather
    /// than being bound directly by the `case v := <-ch:` header).
    pub(crate) fn classify_receive_var_pattern(&mut self, pattern: &Pattern) -> (String, bool) {
        match pattern {
            Pattern::WildCard { .. } => ("_".to_string(), false),
            Pattern::Identifier { identifier, .. } => {
                let Some(go_name) = self.go_name_for_binding(pattern) else {
                    return ("_".to_string(), false);
                };
                if self.scope.resolve_identifier_binding(identifier).is_some() {
                    return (self.fresh_var(Some("recv")), true);
                }
                (self.scope.bind(identifier, go_name), false)
            }
            _ => (self.fresh_var(Some("recv")), true),
        }
    }
}

fn share_unused_alternative_bindings(infos: &mut [PatternInfo]) {
    let unused_names: HashSet<String> = infos
        .iter()
        .flat_map(|info| info.bindings.iter())
        .filter(|binding| binding.go_name.is_none())
        .map(|binding| binding.lisette_name.clone())
        .collect();
    for info in infos.iter_mut() {
        for binding in info.bindings.iter_mut() {
            if unused_names.contains(&binding.lisette_name) {
                binding.go_name = None;
            }
        }
    }
}

/// Fold the `(condition, body)` pieces plus a terminal `else` block into a
/// nested if/else-if statement, built from the back. `pieces` must be
/// non-empty.
fn assemble_if_else_chain(
    mut pieces: Vec<(GoExpression, LoweredBlock)>,
    terminal: LoweredBlock,
) -> LoweredStatement {
    let mut else_arm = ElseArm::from_body(terminal, false);
    while pieces.len() > 1 {
        let (condition, then_body) = pieces.pop().expect("len > 1");
        else_arm = ElseArm::ElseIf(Box::new(IfPlan::plain(condition, then_body, else_arm)));
    }
    let (condition, then_body) = pieces.pop().expect("pieces is non-empty");
    LoweredStatement::If(IfPlan::plain(condition, then_body, else_arm))
}
