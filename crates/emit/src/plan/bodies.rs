//! Lowered body IR: the typed vocabulary `plan::lower` produces and `render/`
//! consumes.

use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{EvaluationEffect, GoExpression, ValuePlan};
use rustc_hash::FxHashSet as HashSet;
use syntax::types::Type;

pub(crate) fn define(name: String, value: GoExpression) -> LoweredStatement {
    LoweredStatement::Define(Definition::single(name, value))
}

pub(crate) fn define_many(names: Vec<String>, value: GoExpression) -> LoweredStatement {
    LoweredStatement::Define(Definition { names, value })
}

pub(crate) fn discard(value: GoExpression) -> LoweredStatement {
    LoweredStatement::Discard(value)
}

pub(crate) fn expression_statement(expression: GoExpression) -> LoweredStatement {
    LoweredStatement::ExpressionStatement {
        expression,
        diverges: false,
    }
}

pub(crate) fn assign(target: GoExpression, value: GoExpression) -> LoweredStatement {
    LoweredStatement::Assign(AssignForm::Simple {
        target_capture: Vec::new(),
        target,
        value: ValuePlan::computed(Vec::new(), value, EvaluationEffect::Pure),
    })
}

/// Destination for a lowered block's tail. The enclosing function's return
/// context (for nested `return`/`?`) is read from the scope stack via
/// `Planner::return_ctx`; `Return` is also the tail target.
pub(crate) enum PlacePlan<'a> {
    Statement,
    Return,
    Assign {
        local: &'a GoExpression,
        target_ty: Option<&'a Type>,
    },
}

impl PlacePlan<'_> {
    pub(crate) fn is_return(&self) -> bool {
        matches!(self, PlacePlan::Return)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoweredBlock {
    pub(crate) statements: Vec<LoweredStatement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopTransfer {
    Unlabeled,
    Source(LoopId),
    Labeled(String),
}

pub(crate) fn directed(directive: String, stmt: LoweredStatement) -> LoweredStatement {
    if directive.is_empty() {
        stmt
    } else {
        LoweredStatement::Directed {
            directive,
            inner: Box::new(stmt),
        }
    }
}

pub(crate) fn directed_first(
    directive: String,
    statements: Vec<LoweredStatement>,
) -> Vec<LoweredStatement> {
    if directive.is_empty() {
        return statements;
    }
    let mut statements = statements.into_iter();
    let first = statements.next().unwrap_or_else(|| {
        LoweredStatement::Body(LoweredBlock {
            statements: Vec::new(),
        })
    });
    let mut directed_statements = vec![directed(directive, first)];
    directed_statements.extend(statements);
    directed_statements
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Definition {
    pub(crate) names: Vec<String>,
    pub(crate) value: GoExpression,
}

impl Definition {
    pub(crate) fn single(name: String, value: GoExpression) -> Self {
        Self {
            names: vec![name],
            value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoweredStatement {
    If(IfPlan),
    Loop(LoopPlan),
    Block(LoweredBlock),
    /// A nested block rendered without adding braces.
    Body(LoweredBlock),
    Break(LoopTransfer),
    Continue(LoopTransfer),
    Const(ConstPlan),
    Return(ReturnForm),
    BreakValue(BreakValuePlan),
    Let(LetPlan),
    Assign(AssignForm),
    Async {
        keyword: String,
        call: GoExpression,
    },
    Select(SelectStatementPlan),
    Switch(SwitchStatementPlan),
    WhileLet(LoweredBlock),
    Define(Definition),
    AssignMany {
        targets: Vec<GoExpression>,
        value: GoExpression,
    },
    /// `var name go_type` (with `= value` when `value` is set).
    VarDecl {
        name: String,
        go_type: String,
        value: Option<GoExpression>,
    },
    Discard(GoExpression),
    ExpressionStatement {
        expression: GoExpression,
        diverges: bool,
    },
    /// A statement preceded by a sourcemap `//line` directive.
    Directed {
        directive: String,
        inner: Box<LoweredStatement>,
    },
    /// `panic("unreachable")` tail after a non-exhaustive branch in return
    /// position: a structured diverging leaf.
    UnreachablePanic,
}

/// A source `const` (or `var` when the value is not Go-const-eligible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstPlan {
    pub(crate) is_const: bool,
    pub(crate) name: String,
    pub(crate) ty_str: String,
    pub(crate) value: ValuePlan,
}

/// A source `return expr` statement, classified by `ReturnForm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnForm {
    Plain {
        value: ValuePlan,
    },
    /// Bare `return` for a unit-typed function. `side_effect` is run first
    /// when the returned expression is impure.
    Unit {
        side_effect: Option<LoweredBlock>,
    },
    /// `return v0, v1, ...` for a lowered multi-value ABI return.
    Multi {
        values: Vec<GoExpression>,
    },
    /// An already-lowered return sequence.
    Body {
        body: LoweredBlock,
    },
}

/// A `break value` statement. A diverged value terminates on its own; all
/// other values carry the action and transfer needed to finish the break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BreakValuePlan {
    Diverged {
        value: ValuePlan,
    },
    Transfer {
        value: ValuePlan,
        action: BreakValueAction,
        target: LoopTransfer,
    },
}

/// What to do with a non-diverging `break value` after its setup has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BreakValueAction {
    /// Inside a loop with a result slot, when the value is a unit-typed
    /// call: emit `<value>` as a side-effect statement (skipped if value
    /// text is empty), then `<result_var> = struct{}{}`, then break.
    UnitCallIntoResult { result_var: String },
    /// Inside a loop with a result slot: emit `<result_var> = <value>`
    /// (skipped if value text is empty), then break.
    AssignToResult { result_var: String },
    /// No result slot: emit `_ = <value>` (skipped if value text is empty),
    /// then break.
    Discard,
}

/// A lowered `let` binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LetPlan {
    /// Optional `var X T` emitted before a never-typed value so dead code can
    /// still reference the binding.
    pub(crate) declaration: Option<Box<LoweredStatement>>,
    pub(crate) body: LoweredBlock,
}

/// An assignment statement, structured by shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignForm {
    /// `target++`, `target--`, or `target op= rhs`.
    Compound {
        target_capture: Vec<LoweredStatement>,
        target: GoExpression,
        kind: CompoundKind,
    },
    /// `target = value`.
    Simple {
        target_capture: Vec<LoweredStatement>,
        target: GoExpression,
        value: ValuePlan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompoundKind {
    Increment,
    Decrement,
    /// `target op= rhs`. An effectful RHS forces the target's prior value
    /// into `pinned_left`, rendered as `target = pinned_left op rhs`.
    OpAssign {
        op_text: String,
        rhs: Box<ValuePlan>,
        pinned_left: Option<GoExpression>,
    },
}

/// A `switch` statement (value or type switch). The renderer owns the
/// `switch`/`case`/`default:` syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchStatementPlan {
    pub(crate) kind: SwitchKind,
    pub(crate) cases: Vec<SwitchCasePlan>,
    pub(crate) default: Option<LoweredBlock>,
    /// Statements after the switch, such as an unreachable panic.
    pub(crate) postlude: Vec<LoweredStatement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchKind {
    Conditional,
    /// `switch <subject> {`
    Value {
        subject: GoExpression,
    },
    /// `switch <binding> := <subject>.(type) {` when `binding` is set,
    /// otherwise `switch <subject>.(type) {`.
    Type {
        subject: GoExpression,
        binding: Option<String>,
    },
}

/// A single `case <labels>:` plus its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchCasePlan {
    pub(crate) labels: Vec<GoExpression>,
    pub(crate) body: LoweredBlock,
}

impl SwitchStatementPlan {
    fn ends_with_diverge(&self) -> bool {
        self.postlude
            .last()
            .is_some_and(LoweredStatement::ends_with_diverge)
    }
}

/// A `select` statement: optional retry-loop wrapper around the `select`, an
/// ordered set of arms, plus hoisted setup and a trailing postlude (e.g. an
/// unreachable panic). The renderer owns the `for`/`select`/`case`/`default:`
/// syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStatementPlan {
    /// Side-effecting setup hoisted before the `select` (channel/value temps).
    pub(crate) setup: Vec<LoweredStatement>,
    /// When set, the `select` is wrapped in `for { ... break }` for retry.
    pub(crate) retry_loop: bool,
    pub(crate) arms: Vec<SelectArmPlan>,
    /// Statements after the `select`/retry loop, such as an unreachable panic.
    pub(crate) postlude: Vec<LoweredStatement>,
}

/// A single `select` arm: a `case`/`default:` header plus its body block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectArmPlan {
    /// `case <receive_vars> := <-<channel>:`, or `case <-<channel>:` when
    /// `receive_vars` is `None`.
    Receive {
        receive_vars: Option<String>,
        channel: GoExpression,
        body: LoweredBlock,
    },
    /// `case <channel> <- <value>:`
    Send {
        channel: GoExpression,
        value: GoExpression,
        body: LoweredBlock,
    },
    /// `default:`
    Default { body: LoweredBlock },
}

impl SelectStatementPlan {
    fn ends_with_diverge(&self) -> bool {
        self.postlude
            .last()
            .is_some_and(LoweredStatement::ends_with_diverge)
            || self.all_arms_diverge()
    }

    pub(crate) fn all_arms_diverge(&self) -> bool {
        !self.arms.is_empty() && self.arms.iter().all(|arm| arm.body().ends_with_diverge())
    }
}

impl SelectArmPlan {
    pub(crate) fn body(&self) -> &LoweredBlock {
        match self {
            SelectArmPlan::Receive { body, .. }
            | SelectArmPlan::Send { body, .. }
            | SelectArmPlan::Default { body } => body,
        }
    }

    pub(crate) fn body_mut(&mut self) -> &mut LoweredBlock {
        match self {
            SelectArmPlan::Receive { body, .. }
            | SelectArmPlan::Send { body, .. }
            | SelectArmPlan::Default { body } => body,
        }
    }
}

/// A statement-position loop. `prologue` is pre-loop setup (a for-loop's
/// iterable capture); its kind records whether transfers inside it can target
/// an enclosing source loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopPlan {
    pub(crate) prologue: Vec<LoweredStatement>,
    pub(crate) kind: LoopKind,
    pub(crate) header: LoopHeader,
    pub(crate) body: LoweredBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopHeader {
    Infinite,
    While(GoExpression),
    Range {
        key: Option<String>,
        value: Option<String>,
        iterable: GoExpression,
    },
    Counted {
        variable: String,
        start: GoExpression,
        condition: Option<GoExpression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopKind {
    Source { label: Option<String> },
    Generated { label: Option<String> },
}

impl LoopKind {
    pub(crate) fn label(&self) -> Option<&str> {
        match self {
            LoopKind::Source { label } | LoopKind::Generated { label } => label.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IfPlan {
    /// Side-effecting setup hoisted before the `if` condition (temps from a
    /// condition that lowered to statements).
    pub(crate) condition_setup: Vec<LoweredStatement>,
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
    pub(crate) then_body: LoweredBlock,
    pub(crate) else_arm: ElseArm,
}

impl IfPlan {
    pub(crate) fn plain(
        condition: GoExpression,
        then_body: LoweredBlock,
        else_arm: ElseArm,
    ) -> Self {
        Self {
            condition_setup: Vec::new(),
            initializer: None,
            condition,
            then_body,
            else_arm,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElseArm {
    None,
    ElseIf(Box<IfPlan>),
    /// `inline` is set when the preceding branch diverges so Go would reject
    /// a dead `else`: the body emits unwrapped after `}` instead of `else {}`.
    Else {
        body: LoweredBlock,
        inline: bool,
    },
}

impl ElseArm {
    pub(crate) fn from_body(body: LoweredBlock, inline: bool) -> Self {
        if body.renders_empty() {
            return ElseArm::None;
        }
        ElseArm::Else { body, inline }
    }
}

fn visit_statements(statements: &[LoweredStatement], visit: &mut impl FnMut(&GoExpressionNode)) {
    for statement in statements {
        statement.visit_expressions(visit);
    }
}

pub(crate) fn for_each_statement(
    statements: &[LoweredStatement],
    f: &mut impl FnMut(&LoweredStatement),
) {
    for statement in statements {
        f(statement);
        statement.for_each_nested_statement(f);
    }
}

pub(crate) fn for_each_statements_mut(
    statements: &mut Vec<LoweredStatement>,
    f: &mut impl FnMut(&mut Vec<LoweredStatement>),
) {
    f(statements);
    for statement in statements {
        statement.for_each_nested_statements_mut(f);
    }
}

#[derive(Default)]
pub(crate) struct GoUses {
    names: HashSet<String>,
}

impl GoUses {
    pub(crate) fn of(statements: &[LoweredStatement]) -> Self {
        let mut uses = Self::default();
        uses.extend(statements);
        uses
    }

    pub(crate) fn extend(&mut self, statements: &[LoweredStatement]) {
        visit_statements(statements, &mut |node| self.record(node));
    }

    pub(crate) fn extend_cases(&mut self, cases: &[SwitchCasePlan]) {
        for case in cases {
            case.visit_expressions(&mut |node| self.record(node));
        }
    }

    pub(crate) fn extend_if(&mut self, plan: &IfPlan) {
        plan.visit_expressions(&mut |node| self.record(node));
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    fn record(&mut self, node: &GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = node {
            self.names.insert(name.clone());
        }
    }
}

impl LoweredBlock {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        visit_statements(&self.statements, visit);
    }

    /// Whether the block's last rendered line is `break`, `continue`,
    /// `return`, or `panic(...)`.
    pub(crate) fn ends_with_diverge(&self) -> bool {
        self.statements
            .last()
            .is_some_and(LoweredStatement::ends_with_diverge)
    }

    /// Whether the block has no statements.
    pub(crate) fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    pub(crate) fn renders_empty(&self) -> bool {
        self.statements
            .iter()
            .all(|statement| !statement.emits_output())
    }
}

impl LoweredStatement {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        match self {
            LoweredStatement::If(plan) => plan.visit_expressions(visit),
            LoweredStatement::Loop(plan) => {
                visit_statements(&plan.prologue, visit);
                match &plan.header {
                    LoopHeader::Infinite => {}
                    LoopHeader::While(condition) => condition.node().visit(visit),
                    LoopHeader::Range { iterable, .. } => iterable.node().visit(visit),
                    LoopHeader::Counted {
                        start, condition, ..
                    } => {
                        start.node().visit(visit);
                        if let Some(condition) = condition {
                            condition.node().visit(visit);
                        }
                    }
                }
                plan.body.visit_expressions(visit);
            }
            LoweredStatement::Block(body)
            | LoweredStatement::Body(body)
            | LoweredStatement::WhileLet(body) => body.visit_expressions(visit),
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::UnreachablePanic => {}
            LoweredStatement::Const(plan) => plan.value.visit_expressions(visit),
            LoweredStatement::Return(form) => match form {
                ReturnForm::Plain { value } => value.visit_expressions(visit),
                ReturnForm::Unit { side_effect } => {
                    if let Some(body) = side_effect {
                        body.visit_expressions(visit);
                    }
                }
                ReturnForm::Multi { values } => {
                    for value in values {
                        value.node().visit(visit);
                    }
                }
                ReturnForm::Body { body } => body.visit_expressions(visit),
            },
            LoweredStatement::BreakValue(plan) => match plan {
                BreakValuePlan::Diverged { value } | BreakValuePlan::Transfer { value, .. } => {
                    value.visit_expressions(visit)
                }
            },
            LoweredStatement::Let(plan) => {
                if let Some(declaration) = &plan.declaration {
                    declaration.visit_expressions(visit);
                }
                plan.body.visit_expressions(visit);
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound {
                    target_capture,
                    target,
                    kind,
                } => {
                    visit_statements(target_capture, visit);
                    target.node().visit(visit);
                    if let CompoundKind::OpAssign {
                        rhs, pinned_left, ..
                    } = kind
                    {
                        rhs.visit_expressions(visit);
                        if let Some(left) = pinned_left {
                            left.node().visit(visit);
                        }
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    target,
                    value,
                } => {
                    visit_statements(target_capture, visit);
                    target.node().visit(visit);
                    value.visit_expressions(visit);
                }
            },
            LoweredStatement::Async { call, .. } => call.node().visit(visit),
            LoweredStatement::Select(plan) => {
                visit_statements(&plan.setup, visit);
                for arm in &plan.arms {
                    match arm {
                        SelectArmPlan::Receive { channel, body, .. } => {
                            channel.node().visit(visit);
                            body.visit_expressions(visit);
                        }
                        SelectArmPlan::Send {
                            channel,
                            value,
                            body,
                        } => {
                            channel.node().visit(visit);
                            value.node().visit(visit);
                            body.visit_expressions(visit);
                        }
                        SelectArmPlan::Default { body } => body.visit_expressions(visit),
                    }
                }
                visit_statements(&plan.postlude, visit);
            }
            LoweredStatement::Switch(plan) => {
                match &plan.kind {
                    SwitchKind::Conditional => {}
                    SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                        subject.node().visit(visit)
                    }
                }
                for case in &plan.cases {
                    case.visit_expressions(visit);
                }
                if let Some(default) = &plan.default {
                    default.visit_expressions(visit);
                }
                visit_statements(&plan.postlude, visit);
            }
            LoweredStatement::Define(definition) => definition.value.node().visit(visit),
            LoweredStatement::AssignMany { targets, value } => {
                for target in targets {
                    target.node().visit(visit);
                }
                value.node().visit(visit);
            }
            LoweredStatement::VarDecl { value, .. } => {
                if let Some(value) = value {
                    value.node().visit(visit);
                }
            }
            LoweredStatement::Discard(expression)
            | LoweredStatement::ExpressionStatement { expression, .. } => {
                expression.node().visit(visit)
            }
            LoweredStatement::Directed { inner, .. } => inner.visit_expressions(visit),
        }
    }

    fn for_each_nested_statement(&self, f: &mut impl FnMut(&LoweredStatement)) {
        match self {
            LoweredStatement::If(plan) => plan.for_each_statement(f),
            LoweredStatement::Loop(plan) => {
                for_each_statement(&plan.prologue, f);
                for_each_statement(&plan.body.statements, f);
            }
            LoweredStatement::Block(body)
            | LoweredStatement::Body(body)
            | LoweredStatement::WhileLet(body) => for_each_statement(&body.statements, f),
            LoweredStatement::Const(plan) => for_each_statement(&plan.value.setup, f),
            LoweredStatement::Return(form) => match form {
                ReturnForm::Plain { value } => for_each_statement(&value.setup, f),
                ReturnForm::Unit { side_effect } => {
                    if let Some(body) = side_effect {
                        for_each_statement(&body.statements, f);
                    }
                }
                ReturnForm::Multi { .. } => {}
                ReturnForm::Body { body } => for_each_statement(&body.statements, f),
            },
            LoweredStatement::BreakValue(
                BreakValuePlan::Diverged { value } | BreakValuePlan::Transfer { value, .. },
            ) => for_each_statement(&value.setup, f),
            LoweredStatement::Let(plan) => {
                if let Some(declaration) = &plan.declaration {
                    f(declaration);
                    declaration.for_each_nested_statement(f);
                }
                for_each_statement(&plan.body.statements, f);
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound {
                    target_capture,
                    kind,
                    ..
                } => {
                    for_each_statement(target_capture, f);
                    if let CompoundKind::OpAssign { rhs, .. } = kind {
                        for_each_statement(&rhs.setup, f);
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    value,
                    ..
                } => {
                    for_each_statement(target_capture, f);
                    for_each_statement(&value.setup, f);
                }
            },
            LoweredStatement::Select(plan) => {
                for_each_statement(&plan.setup, f);
                for arm in &plan.arms {
                    for_each_statement(&arm.body().statements, f);
                }
                for_each_statement(&plan.postlude, f);
            }
            LoweredStatement::Switch(plan) => {
                for case in &plan.cases {
                    for_each_statement(&case.body.statements, f);
                }
                if let Some(default) = &plan.default {
                    for_each_statement(&default.statements, f);
                }
                for_each_statement(&plan.postlude, f);
            }
            LoweredStatement::Directed { inner, .. } => {
                f(inner);
                inner.for_each_nested_statement(f);
            }
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    fn for_each_nested_statements_mut(&mut self, f: &mut impl FnMut(&mut Vec<LoweredStatement>)) {
        match self {
            LoweredStatement::If(plan) => plan.for_each_statements_mut(f),
            LoweredStatement::Loop(plan) => {
                for_each_statements_mut(&mut plan.prologue, f);
                for_each_statements_mut(&mut plan.body.statements, f);
            }
            LoweredStatement::Block(body)
            | LoweredStatement::Body(body)
            | LoweredStatement::WhileLet(body) => for_each_statements_mut(&mut body.statements, f),
            LoweredStatement::Const(plan) => for_each_statements_mut(&mut plan.value.setup, f),
            LoweredStatement::Return(form) => match form {
                ReturnForm::Plain { value } => for_each_statements_mut(&mut value.setup, f),
                ReturnForm::Unit { side_effect } => {
                    if let Some(body) = side_effect {
                        for_each_statements_mut(&mut body.statements, f);
                    }
                }
                ReturnForm::Multi { .. } => {}
                ReturnForm::Body { body } => for_each_statements_mut(&mut body.statements, f),
            },
            LoweredStatement::BreakValue(
                BreakValuePlan::Diverged { value } | BreakValuePlan::Transfer { value, .. },
            ) => for_each_statements_mut(&mut value.setup, f),
            LoweredStatement::Let(plan) => {
                if let Some(declaration) = &mut plan.declaration {
                    declaration.for_each_nested_statements_mut(f);
                }
                for_each_statements_mut(&mut plan.body.statements, f);
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound {
                    target_capture,
                    kind,
                    ..
                } => {
                    for_each_statements_mut(target_capture, f);
                    if let CompoundKind::OpAssign { rhs, .. } = kind {
                        for_each_statements_mut(&mut rhs.setup, f);
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    value,
                    ..
                } => {
                    for_each_statements_mut(target_capture, f);
                    for_each_statements_mut(&mut value.setup, f);
                }
            },
            LoweredStatement::Select(plan) => {
                for_each_statements_mut(&mut plan.setup, f);
                for arm in &mut plan.arms {
                    for_each_statements_mut(&mut arm.body_mut().statements, f);
                }
                for_each_statements_mut(&mut plan.postlude, f);
            }
            LoweredStatement::Switch(plan) => {
                for case in &mut plan.cases {
                    for_each_statements_mut(&mut case.body.statements, f);
                }
                if let Some(default) = &mut plan.default {
                    for_each_statements_mut(&mut default.statements, f);
                }
                for_each_statements_mut(&mut plan.postlude, f);
            }
            LoweredStatement::Directed { inner, .. } => inner.for_each_nested_statements_mut(f),
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    /// The Go name this statement binds, seeing through a sourcemap directive.
    pub(crate) fn bound_name(&self) -> Option<&str> {
        match self {
            LoweredStatement::Directed { inner, .. } => inner.bound_name(),
            LoweredStatement::Define(Definition { names, .. }) => match names.as_slice() {
                [name] => Some(name),
                _ => None,
            },
            LoweredStatement::VarDecl {
                name,
                value: Some(_),
                ..
            } => Some(name),
            _ => None,
        }
    }

    pub(crate) fn binds_name(&self, go_name: &str) -> bool {
        self.bound_name() == Some(go_name)
    }

    /// `false` when the statement binds nothing.
    pub(crate) fn rename_bound_name(&mut self, go_name: &str) -> bool {
        let bound = match self {
            LoweredStatement::Directed { inner, .. } => return inner.rename_bound_name(go_name),
            LoweredStatement::Define(Definition { names, .. }) => match names.as_mut_slice() {
                [name] => name,
                _ => return false,
            },
            LoweredStatement::VarDecl {
                name,
                value: Some(_),
                ..
            } => name,
            _ => return false,
        };
        *bound = go_name.to_string();
        true
    }

    fn emits_output(&self) -> bool {
        match self {
            LoweredStatement::If(_)
            | LoweredStatement::Loop(_)
            | LoweredStatement::Block(_)
            | LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Const(_)
            | LoweredStatement::Select(_)
            | LoweredStatement::Switch(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::UnreachablePanic => true,
            LoweredStatement::Body(body) => !body.renders_empty(),
            LoweredStatement::Return(plan) => match plan {
                ReturnForm::Body { body } => !body.renders_empty(),
                ReturnForm::Plain { .. } | ReturnForm::Unit { .. } | ReturnForm::Multi { .. } => {
                    true
                }
            },
            LoweredStatement::BreakValue(plan) => match plan {
                BreakValuePlan::Diverged { value } => {
                    value.setup.iter().any(LoweredStatement::emits_output)
                }
                BreakValuePlan::Transfer { .. } => true,
            },
            LoweredStatement::Let(plan) => plan.declaration.is_some() || !plan.body.renders_empty(),
            LoweredStatement::Assign(plan) => match plan {
                AssignForm::Compound { .. } | AssignForm::Simple { .. } => true,
            },
            LoweredStatement::WhileLet(body) => !body.renders_empty(),
            LoweredStatement::ExpressionStatement { expression, .. } => !expression.is_empty(),
            LoweredStatement::Directed { directive, inner } => {
                !directive.is_empty() || inner.emits_output()
            }
        }
    }

    fn ends_with_diverge(&self) -> bool {
        match self {
            LoweredStatement::If(plan) => plan.ends_with_diverge(),
            LoweredStatement::Loop(_) | LoweredStatement::Block(_) | LoweredStatement::Const(_) => {
                false
            }
            LoweredStatement::Body(body) => body.ends_with_diverge(),
            LoweredStatement::Break(_) | LoweredStatement::Continue(_) => true,
            LoweredStatement::Return(_) => true,
            LoweredStatement::BreakValue(_) => true,
            LoweredStatement::Let(plan) => plan.body.ends_with_diverge(),
            LoweredStatement::Assign(plan) => match plan {
                AssignForm::Compound { .. } | AssignForm::Simple { .. } => false,
            },
            LoweredStatement::Async { .. } => false,
            LoweredStatement::Select(plan) => plan.ends_with_diverge(),
            LoweredStatement::Switch(plan) => plan.ends_with_diverge(),
            LoweredStatement::WhileLet(body) => body.ends_with_diverge(),
            LoweredStatement::Define(_)
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::AssignMany { .. } => false,
            LoweredStatement::ExpressionStatement { diverges, .. } => *diverges,
            LoweredStatement::Directed { inner, .. } => inner.ends_with_diverge(),
            LoweredStatement::UnreachablePanic => true,
        }
    }

    pub(crate) fn blocks_fallthrough(&self) -> bool {
        if let LoweredStatement::Directed { inner, .. } = self {
            return inner.blocks_fallthrough();
        }
        !matches!(self, LoweredStatement::WhileLet(_)) && self.ends_with_diverge()
    }
}

impl SwitchCasePlan {
    fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        for label in &self.labels {
            label.node().visit(visit);
        }
        self.body.visit_expressions(visit);
    }
}

impl IfPlan {
    fn for_each_statement(&self, f: &mut impl FnMut(&LoweredStatement)) {
        for_each_statement(&self.condition_setup, f);
        for_each_statement(&self.then_body.statements, f);
        match &self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.for_each_statement(f),
            ElseArm::Else { body, .. } => for_each_statement(&body.statements, f),
        }
    }

    fn for_each_statements_mut(&mut self, f: &mut impl FnMut(&mut Vec<LoweredStatement>)) {
        for_each_statements_mut(&mut self.condition_setup, f);
        for_each_statements_mut(&mut self.then_body.statements, f);
        match &mut self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.for_each_statements_mut(f),
            ElseArm::Else { body, .. } => for_each_statements_mut(&mut body.statements, f),
        }
    }

    fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        visit_statements(&self.condition_setup, visit);
        if let Some(initializer) = &self.initializer {
            initializer.value.node().visit(visit);
        }
        self.condition.node().visit(visit);
        self.then_body.visit_expressions(visit);
        match &self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.visit_expressions(visit),
            ElseArm::Else { body, .. } => body.visit_expressions(visit),
        }
    }

    fn ends_with_diverge(&self) -> bool {
        if !self.then_body.ends_with_diverge() {
            return false;
        }
        match &self.else_arm {
            ElseArm::None => false,
            ElseArm::ElseIf(inner) if inner.condition_setup.is_empty() => inner.ends_with_diverge(),
            ElseArm::ElseIf(_) => false,
            ElseArm::Else { body, .. } => body.ends_with_diverge(),
        }
    }
}
