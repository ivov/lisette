//! Lowered body IR: the typed vocabulary `plan::lower` produces and `render/`
//! consumes.

use crate::plan::go_expression::{BinaryOp, GoExpressionNode};
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::values::{EvaluationEffect, GoExpression, ValuePlan};
use rustc_hash::FxHashSet as HashSet;
use std::mem::{replace, take};
use syntax::types::Type;

pub(crate) fn define(name: impl Into<GoIdentifier>, value: GoExpression) -> Statement {
    LoweredStatement::Define(Definition::single(name, value)).into()
}

pub(crate) fn define_many<T: Into<GoIdentifier>>(names: Vec<T>, value: GoExpression) -> Statement {
    LoweredStatement::Define(Definition {
        names: names.into_iter().map(Into::into).collect(),
        value,
    })
    .into()
}

pub(crate) fn discard(value: GoExpression) -> Statement {
    LoweredStatement::Discard(value).into()
}

pub(crate) fn expression_statement(expression: GoExpression) -> Statement {
    LoweredStatement::ExpressionStatement {
        expression,
        diverges: false,
    }
    .into()
}

pub(crate) fn assign(target: GoExpression, value: GoExpression) -> Statement {
    LoweredStatement::Assign(AssignForm::Simple {
        target_capture: Vec::new(),
        target,
        value: ValuePlan::computed(Vec::new(), value, EvaluationEffect::Pure),
    })
    .into()
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
    pub(crate) statements: Vec<Statement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopTransfer {
    Unlabeled,
    Source(LoopId),
    Labeled(String),
}

#[derive(Clone, PartialEq, Eq)]
enum FlowExit {
    Break(LoopTransfer),
    Continue(LoopTransfer),
}

#[derive(Clone)]
struct FlowSummary {
    falls_through: bool,
    go_falls_through: bool,
    source_exits: Vec<FlowExit>,
    go_exits: Vec<FlowExit>,
}

impl FlowSummary {
    fn next() -> Self {
        Self {
            falls_through: true,
            go_falls_through: true,
            source_exits: Vec::new(),
            go_exits: Vec::new(),
        }
    }

    fn exit(exit: FlowExit) -> Self {
        Self {
            falls_through: false,
            go_falls_through: false,
            source_exits: vec![exit.clone()],
            go_exits: vec![exit],
        }
    }

    fn terminal() -> Self {
        Self {
            falls_through: false,
            go_falls_through: false,
            source_exits: Vec::new(),
            go_exits: Vec::new(),
        }
    }

    fn source_never() -> Self {
        Self {
            go_falls_through: true,
            ..Self::terminal()
        }
    }

    fn sequence(mut self, next: Self) -> Self {
        let source_reaches_next = self.falls_through;
        let go_reaches_next = self.go_falls_through;
        self.falls_through &= next.falls_through;
        self.go_falls_through &= next.go_falls_through;
        if source_reaches_next {
            self.source_exits.extend(next.source_exits);
        }
        if go_reaches_next {
            self.go_exits.extend(next.go_exits);
        }
        self
    }

    fn branch(mut self, other: Self) -> Self {
        self.falls_through |= other.falls_through;
        self.go_falls_through |= other.go_falls_through;
        self.source_exits.extend(other.source_exits);
        self.go_exits.extend(other.go_exits);
        self
    }

    fn statements(statements: &[Statement]) -> Self {
        statements.iter().fold(Self::next(), |flow, statement| {
            flow.sequence(statement.kind.flow())
        })
    }

    fn consume_unlabeled_breaks(&mut self) {
        self.falls_through |= remove_unlabeled_breaks(&mut self.source_exits);
        self.go_falls_through |= remove_unlabeled_breaks(&mut self.go_exits);
    }
}

fn remove_unlabeled_breaks(exits: &mut Vec<FlowExit>) -> bool {
    let had_break = exits
        .iter()
        .any(|exit| matches!(exit, FlowExit::Break(LoopTransfer::Unlabeled)));
    exits.retain(|exit| !matches!(exit, FlowExit::Break(LoopTransfer::Unlabeled)));
    had_break
}

/// Put `line` on the first statement unless it has a more specific directive.
pub(crate) fn directed_first(
    line: Option<String>,
    mut statements: Vec<Statement>,
) -> Vec<Statement> {
    if let Some(first) = statements.first_mut()
        && first.line.is_none()
    {
        first.line = line;
    }
    statements
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Definition {
    pub(crate) names: Vec<GoIdentifier>,
    pub(crate) value: GoExpression,
}

impl Definition {
    pub(crate) fn single(name: impl Into<GoIdentifier>, value: GoExpression) -> Self {
        Self {
            names: vec![name.into()],
            value,
        }
    }
}

/// A lowered statement entry with its optional sourcemap `line` directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Statement {
    pub(crate) line: Option<String>,
    pub(crate) kind: LoweredStatement,
}

impl From<LoweredStatement> for Statement {
    fn from(kind: LoweredStatement) -> Self {
        Self { line: None, kind }
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
    Return(Vec<GoExpression>),
    Assign(AssignForm),
    Async {
        keyword: String,
        call: GoExpression,
    },
    Select(SelectStatementPlan),
    Switch(SwitchStatementPlan),
    Define(Definition),
    AssignMany {
        targets: Vec<GoExpression>,
        value: GoExpression,
    },
    /// `var name go_type` (with `= value` when `value` is set).
    VarDecl {
        name: GoIdentifier,
        go_type: String,
        value: Option<GoExpression>,
    },
    Discard(GoExpression),
    ExpressionStatement {
        expression: GoExpression,
        diverges: bool,
    },
    /// `panic("unreachable")` generated to complete a Go return path.
    UnreachablePanic,
}

/// A source `const` (or `var` when the value is not Go-const-eligible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstPlan {
    pub(crate) is_const: bool,
    pub(crate) name: GoIdentifier,
    pub(crate) ty_str: String,
    pub(crate) value: GoExpression,
}

/// An assignment statement, structured by shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignForm {
    /// `target++`, `target--`, or `target op= rhs`.
    Compound {
        target_capture: Vec<Statement>,
        target: GoExpression,
        kind: CompoundKind,
    },
    /// `target = value`.
    Simple {
        target_capture: Vec<Statement>,
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
        operator: BinaryOp,
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
    pub(crate) postlude: Vec<Statement>,
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
        binding: Option<GoIdentifier>,
    },
}

/// A single `case <labels>:` plus its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchCasePlan {
    pub(crate) labels: Vec<GoExpression>,
    pub(crate) body: LoweredBlock,
}

impl SwitchStatementPlan {
    fn flow(&self) -> FlowSummary {
        let mut branches = self
            .cases
            .iter()
            .map(|case| FlowSummary::statements(&case.body.statements));
        let first = branches.next().or_else(|| {
            self.default
                .as_ref()
                .map(|body| FlowSummary::statements(&body.statements))
        });
        let mut cases = branches.fold(first.unwrap_or_else(FlowSummary::next), FlowSummary::branch);
        if let Some(default) = &self.default {
            if !self.cases.is_empty() {
                cases = cases.branch(FlowSummary::statements(&default.statements));
            }
        } else {
            cases.falls_through = true;
            cases.go_falls_through = true;
        }
        cases.consume_unlabeled_breaks();
        cases.sequence(FlowSummary::statements(&self.postlude))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStatementPlan {
    pub(crate) arms: Vec<SelectArmPlan>,
}

/// A single `select` arm: a `case`/`default:` header plus its body block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectArmPlan {
    /// `case <receive_vars> := <-<channel>:`, or `case <-<channel>:` when
    /// `receive_vars` is empty.
    Receive {
        receive_vars: Vec<GoIdentifier>,
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
    fn flow(&self) -> FlowSummary {
        let mut arms = self
            .arms
            .iter()
            .map(|arm| FlowSummary::statements(&arm.body().statements));
        let Some(first) = arms.next() else {
            return FlowSummary::terminal();
        };
        let mut flow = arms.fold(first, FlowSummary::branch);
        flow.consume_unlabeled_breaks();
        flow
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
    pub(crate) prologue: Vec<Statement>,
    pub(crate) kind: LoopKind,
    pub(crate) header: LoopHeader,
    pub(crate) body: LoweredBlock,
}

impl LoopPlan {
    fn flow(&self) -> FlowSummary {
        let mut body = FlowSummary::statements(&self.body.statements);
        let label = self.kind.label();
        let source_breaks = consume_loop_exits(&mut body.source_exits, label);
        let go_breaks = consume_loop_exits(&mut body.go_exits, label);
        body.falls_through = source_breaks || !matches!(self.header, LoopHeader::Infinite);
        body.go_falls_through = go_breaks || !matches!(self.header, LoopHeader::Infinite);
        FlowSummary::statements(&self.prologue).sequence(body)
    }
}

fn consume_loop_exits(exits: &mut Vec<FlowExit>, label: Option<&str>) -> bool {
    let mut breaks_here = false;
    exits.retain(|exit| {
        let targets_loop = match exit {
            FlowExit::Break(LoopTransfer::Unlabeled)
            | FlowExit::Continue(LoopTransfer::Unlabeled) => true,
            FlowExit::Break(LoopTransfer::Labeled(target))
            | FlowExit::Continue(LoopTransfer::Labeled(target)) => label == Some(target.as_str()),
            FlowExit::Break(LoopTransfer::Source(_))
            | FlowExit::Continue(LoopTransfer::Source(_)) => false,
        };
        if targets_loop && matches!(exit, FlowExit::Break(_)) {
            breaks_here = true;
        }
        !targets_loop
    });
    breaks_here
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopHeader {
    Infinite,
    While(GoExpression),
    Range {
        key: Option<GoIdentifier>,
        value: Option<GoIdentifier>,
        iterable: GoExpression,
    },
    Counted {
        variable: GoIdentifier,
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
    pub(crate) condition_setup: Vec<Statement>,
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

fn visit_statements(statements: &[Statement], visit: &mut impl FnMut(&GoExpressionNode)) {
    for statement in statements {
        statement.kind.visit_expressions(visit);
    }
}

pub(crate) fn for_each_statement(statements: &[Statement], f: &mut impl FnMut(&LoweredStatement)) {
    for statement in statements {
        f(&statement.kind);
        statement.kind.for_each_nested_statement(f);
    }
}

pub(crate) fn rename_generated_locals(
    statements: &mut [Statement],
    by_id: &rustc_hash::FxHashMap<LocalId, String>,
) {
    struct Rename<'a>(&'a rustc_hash::FxHashMap<LocalId, String>);

    impl super::visit::VisitorMut for Rename<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::Identifier(name) = node
                && let Some(final_name) = name.id().and_then(|id| self.0.get(&id))
            {
                *name.spelling_mut() = final_name.clone();
            }
        }

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if let Some(final_name) = name.id().and_then(|id| self.0.get(&id)) {
                *name.spelling_mut() = final_name.clone();
            }
        }
    }

    super::visit::visit_statements_mut(statements, &mut Rename(by_id));
}

pub(crate) fn for_each_statements_mut(
    statements: &mut Vec<Statement>,
    f: &mut impl FnMut(&mut Vec<Statement>),
) {
    f(statements);
    for statement in statements {
        statement.kind.for_each_nested_statements_mut(f);
    }
}

pub(crate) fn legalize_else_if_scopes(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |statements| {
        for statement in statements {
            if let LoweredStatement::If(plan) = &mut statement.kind {
                plan.legalize_else_if_scope();
            }
        }
    });
}

#[derive(Default)]
pub(crate) struct GoUses {
    locals: HashSet<LocalId>,
    unidentified: HashSet<String>,
    spellings: HashSet<String>,
}

impl GoUses {
    pub(crate) fn of(statements: &[Statement]) -> Self {
        let mut uses = Self::default();
        uses.extend(statements);
        uses
    }

    pub(crate) fn extend(&mut self, statements: &[Statement]) {
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

    pub(crate) fn contains_identifier(&self, name: &GoIdentifier) -> bool {
        let id = name.id().expect("local-use queries need a local ID");
        assert!(
            !self.unidentified.contains(name.spelling()),
            "unidentified reference to queried local {}",
            name.spelling()
        );
        self.locals.contains(&id)
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.spellings.contains(name)
    }

    fn record(&mut self, node: &GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = node {
            self.spellings.insert(name.spelling().to_string());
            if let Some(id) = name.id() {
                self.locals.insert(id);
            } else if name.is_pending() {
                self.unidentified.insert(name.spelling().to_string());
            }
        }
    }
}

impl LoweredBlock {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        visit_statements(&self.statements, visit);
    }

    pub(crate) fn ends_with_diverge(&self) -> bool {
        !FlowSummary::statements(&self.statements).falls_through
    }

    pub(crate) fn ensure_go_termination(&mut self) {
        terminate_go_path(&mut self.statements);
    }

    pub(crate) fn go_terminates(&self) -> bool {
        !FlowSummary::statements(&self.statements).go_falls_through
    }

    /// Whether the block has no statements.
    pub(crate) fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }

    pub(crate) fn renders_empty(&self) -> bool {
        self.statements
            .iter()
            .all(|statement| !statement.kind.emits_output())
    }
}

fn terminate_go_path(statements: &mut Vec<Statement>) {
    if !FlowSummary::statements(statements).go_falls_through {
        return;
    }
    if let Some(last) = statements.last_mut()
        && !last.kind.flow().falls_through
    {
        last.kind.terminate_go_tail();
        if !FlowSummary::statements(statements).go_falls_through {
            return;
        }
    }
    statements.push(LoweredStatement::UnreachablePanic.into());
}

impl LoweredStatement {
    fn terminate_go_tail(&mut self) {
        match self {
            Self::If(plan) => plan.terminate_go_tail(),
            Self::Body(body) => terminate_go_path(&mut body.statements),
            _ => {}
        }
    }

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
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
                body.visit_expressions(visit)
            }
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::UnreachablePanic => {}
            LoweredStatement::Const(plan) => plan.value.node().visit(visit),
            LoweredStatement::Return(values) => {
                for value in values {
                    value.node().visit(visit);
                }
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
        }
    }

    fn for_each_nested_statement(&self, f: &mut impl FnMut(&LoweredStatement)) {
        match self {
            LoweredStatement::If(plan) => plan.for_each_statement(f),
            LoweredStatement::Loop(plan) => {
                for_each_statement(&plan.prologue, f);
                for_each_statement(&plan.body.statements, f);
            }
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
                for_each_statement(&body.statements, f)
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound {
                    target_capture,
                    kind,
                    ..
                } => {
                    for_each_statement(target_capture, f);
                    if let CompoundKind::OpAssign { rhs, .. } = kind {
                        for_each_statement(rhs.setup(), f);
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    value,
                    ..
                } => {
                    for_each_statement(target_capture, f);
                    for_each_statement(value.setup(), f);
                }
            },
            LoweredStatement::Select(plan) => {
                for arm in &plan.arms {
                    for_each_statement(&arm.body().statements, f);
                }
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
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Return(_)
            | LoweredStatement::Const(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    fn for_each_nested_statements_mut(&mut self, f: &mut impl FnMut(&mut Vec<Statement>)) {
        match self {
            LoweredStatement::If(plan) => plan.for_each_statements_mut(f),
            LoweredStatement::Loop(plan) => {
                for_each_statements_mut(&mut plan.prologue, f);
                for_each_statements_mut(&mut plan.body.statements, f);
            }
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
                for_each_statements_mut(&mut body.statements, f)
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound {
                    target_capture,
                    kind,
                    ..
                } => {
                    for_each_statements_mut(target_capture, f);
                    if let CompoundKind::OpAssign { rhs, .. } = kind {
                        for_each_statements_mut(rhs.parts_mut().0, f);
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    value,
                    ..
                } => {
                    for_each_statements_mut(target_capture, f);
                    for_each_statements_mut(value.parts_mut().0, f);
                }
            },
            LoweredStatement::Select(plan) => {
                for arm in &mut plan.arms {
                    for_each_statements_mut(&mut arm.body_mut().statements, f);
                }
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
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Return(_)
            | LoweredStatement::Const(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    pub(crate) fn binds_name(&self, go_name: &str) -> bool {
        match self {
            LoweredStatement::Define(Definition { names, .. }) => {
                names.iter().any(|name| name == go_name)
            }
            LoweredStatement::VarDecl { name, .. } => name == go_name,
            _ => false,
        }
    }

    /// `false` when the statement binds nothing.
    pub(crate) fn rename_bound_name(&mut self, go_name: &str) -> bool {
        let bound = match self {
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
        *bound = go_name.to_string().into();
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
            LoweredStatement::Return(_) => true,
            LoweredStatement::Assign(plan) => match plan {
                AssignForm::Compound { .. } | AssignForm::Simple { .. } => true,
            },
            LoweredStatement::ExpressionStatement { expression, .. } => !expression.is_empty(),
        }
    }

    fn flow(&self) -> FlowSummary {
        match self {
            LoweredStatement::If(plan) => plan.flow(),
            LoweredStatement::Loop(plan) => plan.flow(),
            LoweredStatement::Block(body) => FlowSummary {
                falls_through: true,
                ..FlowSummary::statements(&body.statements)
            },
            LoweredStatement::Body(body) => FlowSummary::statements(&body.statements),
            LoweredStatement::Break(target) => FlowSummary::exit(FlowExit::Break(target.clone())),
            LoweredStatement::Continue(target) => {
                FlowSummary::exit(FlowExit::Continue(target.clone()))
            }
            LoweredStatement::Return(_) => FlowSummary::terminal(),
            LoweredStatement::Select(plan) => plan.flow(),
            LoweredStatement::Switch(plan) => plan.flow(),
            LoweredStatement::ExpressionStatement {
                expression,
                diverges: true,
            } => {
                let go_panic = matches!(expression.node(), GoExpressionNode::Call { callee, .. }
                    if matches!(callee.as_ref(), GoExpressionNode::Identifier(name) if name == "panic"));
                if go_panic {
                    FlowSummary::terminal()
                } else {
                    FlowSummary::source_never()
                }
            }
            LoweredStatement::UnreachablePanic => FlowSummary::terminal(),
            LoweredStatement::Assign(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Const(_)
            | LoweredStatement::Define(_)
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::Discard(_)
            | LoweredStatement::AssignMany { .. }
            | LoweredStatement::ExpressionStatement { .. } => FlowSummary::next(),
        }
    }

    pub(crate) fn blocks_fallthrough(&self) -> bool {
        !self.flow().falls_through
    }
}

impl SwitchCasePlan {
    pub(super) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        for label in &self.labels {
            label.node().visit(visit);
        }
        self.body.visit_expressions(visit);
    }
}

impl IfPlan {
    fn legalize_else_if_scope(&mut self) {
        if let ElseArm::ElseIf(inner) = &mut self.else_arm {
            inner.legalize_else_if_scope();
            if !inner.condition_setup.is_empty() {
                let ElseArm::ElseIf(mut inner) = replace(&mut self.else_arm, ElseArm::None) else {
                    unreachable!("else-if was checked before replacement");
                };
                let mut statements = take(&mut inner.condition_setup);
                statements.push(LoweredStatement::If(*inner).into());
                self.else_arm = ElseArm::Else {
                    body: LoweredBlock { statements },
                    inline: false,
                };
            }
        }
    }

    fn for_each_statement(&self, f: &mut impl FnMut(&LoweredStatement)) {
        for_each_statement(&self.condition_setup, f);
        for_each_statement(&self.then_body.statements, f);
        match &self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.for_each_statement(f),
            ElseArm::Else { body, .. } => for_each_statement(&body.statements, f),
        }
    }

    fn for_each_statements_mut(&mut self, f: &mut impl FnMut(&mut Vec<Statement>)) {
        for_each_statements_mut(&mut self.condition_setup, f);
        for_each_statements_mut(&mut self.then_body.statements, f);
        match &mut self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.for_each_statements_mut(f),
            ElseArm::Else { body, .. } => for_each_statements_mut(&mut body.statements, f),
        }
    }

    pub(super) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
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

    fn terminate_go_tail(&mut self) {
        terminate_go_path(&mut self.then_body.statements);
        match &mut self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(inner) => inner.terminate_go_tail(),
            ElseArm::Else { body, .. } => terminate_go_path(&mut body.statements),
        }
    }

    fn flow(&self) -> FlowSummary {
        let then_flow = FlowSummary::statements(&self.then_body.statements);
        let else_flow = match &self.else_arm {
            ElseArm::None => FlowSummary::next(),
            ElseArm::ElseIf(inner) => inner.flow(),
            ElseArm::Else { body, .. } => FlowSummary::statements(&body.statements),
        };
        let mut branches = then_flow.clone().branch(else_flow.clone());
        if matches!(self.else_arm, ElseArm::Else { inline: true, .. }) {
            let go = then_flow.branch(FlowSummary::next()).sequence(else_flow);
            branches.go_falls_through = go.go_falls_through;
            branches.go_exits = go.go_exits;
        }
        FlowSummary::statements(&self.condition_setup).sequence(branches)
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    fn block(statements: Vec<Statement>) -> LoweredBlock {
        LoweredBlock { statements }
    }

    fn never_call() -> LoweredStatement {
        LoweredStatement::ExpressionStatement {
            expression: GoExpression::call(GoExpression::name("fail".into()), Vec::new()),
            diverges: true,
        }
    }

    fn infinite(statements: Vec<Statement>) -> LoweredStatement {
        LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            kind: LoopKind::Source { label: None },
            header: LoopHeader::Infinite,
            body: block(statements),
        })
    }

    #[test]
    fn never_call_does_not_hide_a_go_reachable_break() {
        let mut body = block(vec![
            infinite(vec![
                never_call().into(),
                LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
            ])
            .into(),
        ]);
        assert!(!FlowSummary::statements(&body.statements).falls_through);
        assert!(!body.go_terminates());
        body.ensure_go_termination();
        assert!(matches!(
            body.statements.last().map(|last| &last.kind),
            Some(LoweredStatement::UnreachablePanic)
        ));
    }

    #[test]
    fn never_call_followed_by_return_needs_no_fallback() {
        let mut body = block(vec![
            never_call().into(),
            LoweredStatement::Return(vec![GoExpression::literal("1".into())]).into(),
        ]);
        assert!(body.go_terminates());
        body.ensure_go_termination();
        assert_eq!(body.statements.len(), 2);
    }

    #[test]
    fn never_branch_gets_one_local_fallback() {
        let then_body = block(vec![
            LoweredStatement::Body(block(vec![never_call().into()])).into(),
        ]);
        let mut body = block(vec![
            LoweredStatement::If(IfPlan::plain(
                GoExpression::literal("true".into()),
                then_body,
                ElseArm::Else {
                    body: block(vec![LoweredStatement::Return(vec![]).into()]),
                    inline: false,
                },
            ))
            .into(),
        ]);
        body.ensure_go_termination();
        body.ensure_go_termination();
        let [statement] = body.statements.as_slice() else {
            panic!("expected if without a trailing panic");
        };
        let LoweredStatement::If(plan) = &statement.kind else {
            panic!("expected if");
        };
        let [nested] = plan.then_body.statements.as_slice() else {
            panic!("expected one nested statement");
        };
        let LoweredStatement::Body(nested) = &nested.kind else {
            panic!("expected nested body");
        };
        assert_eq!(nested.statements.len(), 2);
        assert!(matches!(
            nested.statements[1].kind,
            LoweredStatement::UnreachablePanic
        ));
    }

    #[test]
    fn never_call_before_later_return_gets_no_fallback() {
        let mut body = block(vec![
            LoweredStatement::If(IfPlan::plain(
                GoExpression::literal("true".into()),
                block(vec![never_call().into()]),
                ElseArm::None,
            ))
            .into(),
            LoweredStatement::Return(vec![GoExpression::literal("1".into())]).into(),
        ]);
        body.ensure_go_termination();
        let LoweredStatement::If(plan) = &body.statements[0].kind else {
            panic!("expected if");
        };
        assert_eq!(plan.then_body.statements.len(), 1);
        assert_eq!(body.statements.len(), 2);
    }

    #[test]
    fn while_let_shaped_loop_falls_through_only_with_its_break() {
        let while_let = |else_arm: ElseArm| {
            LoweredStatement::Body(block(vec![
                infinite(vec![
                    LoweredStatement::If(IfPlan::plain(
                        GoExpression::literal("ok".into()),
                        block(vec![]),
                        else_arm,
                    ))
                    .into(),
                ])
                .into(),
            ]))
        };
        let with_break = while_let(ElseArm::Else {
            body: block(vec![
                LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
            ]),
            inline: false,
        });
        assert!(!with_break.blocks_fallthrough());
        assert!(while_let(ElseArm::None).blocks_fallthrough());
    }

    #[test]
    fn inline_else_is_sequential_for_go_flow() {
        let body = block(vec![
            LoweredStatement::If(IfPlan::plain(
                GoExpression::literal("true".into()),
                block(vec![never_call().into()]),
                ElseArm::Else {
                    body: block(vec![LoweredStatement::Return(vec![]).into()]),
                    inline: true,
                },
            ))
            .into(),
        ]);
        assert!(body.go_terminates());
    }

    #[test]
    fn switch_break_stays_inside_the_switch() {
        let switch = LoweredStatement::Switch(SwitchStatementPlan {
            kind: SwitchKind::Conditional,
            cases: vec![SwitchCasePlan {
                labels: vec![GoExpression::literal("true".into())],
                body: block(vec![
                    LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
                ]),
            }],
            default: Some(block(vec![LoweredStatement::Return(vec![]).into()])),
            postlude: Vec::new(),
        });
        let body = block(vec![infinite(vec![switch.into()]).into()]);
        assert!(body.go_terminates());
    }

    #[test]
    fn labeled_break_escapes_the_switch_and_loop() {
        let switch = LoweredStatement::Switch(SwitchStatementPlan {
            kind: SwitchKind::Conditional,
            cases: vec![SwitchCasePlan {
                labels: vec![GoExpression::literal("true".into())],
                body: block(vec![
                    LoweredStatement::Break(LoopTransfer::Labeled("outer".into())).into(),
                ]),
            }],
            default: Some(block(vec![
                LoweredStatement::Continue(LoopTransfer::Labeled("outer".into())).into(),
            ])),
            postlude: Vec::new(),
        });
        let body = block(vec![
            LoweredStatement::Loop(LoopPlan {
                prologue: Vec::new(),
                kind: LoopKind::Source {
                    label: Some("outer".into()),
                },
                header: LoopHeader::Infinite,
                body: block(vec![switch.into()]),
            })
            .into(),
        ]);
        assert!(!body.go_terminates());
    }
}

#[cfg(test)]
mod go_uses_tests {
    use super::*;
    use crate::plan::visit::identify_body_locals;
    use crate::state::scope::ScopeState;
    use std::slice::from_ref;

    #[test]
    fn inner_binding_does_not_count_as_a_read_of_the_outer_local() {
        let mut scope = ScopeState::new();
        let outer = GoIdentifier::local("value".to_string(), scope.new_local_id());
        let mut statements = vec![
            LoweredStatement::Block(LoweredBlock {
                statements: vec![
                    define("value".to_string(), GoExpression::literal("1".to_string())),
                    LoweredStatement::Return(vec![GoExpression::name("value".to_string())]).into(),
                ],
            })
            .into(),
        ];
        identify_body_locals(&mut statements, from_ref(&outer), &mut scope);

        let uses = GoUses::of(&statements);
        assert!(!uses.contains_identifier(&outer));
        let LoweredStatement::Block(body) = &statements[0].kind else {
            panic!("expected block");
        };
        let LoweredStatement::Define(inner) = &body.statements[0].kind else {
            panic!("expected definition");
        };
        assert!(uses.contains_identifier(&inner.names[0]));
    }

    #[test]
    fn external_name_does_not_count_as_a_local_use() {
        let local = GoIdentifier::local("value".to_string(), LocalId(1));
        let statements = vec![
            LoweredStatement::Return(vec![GoExpression::external_name("value".to_string())]).into(),
        ];
        assert!(!GoUses::of(&statements).contains_identifier(&local));
    }
}
