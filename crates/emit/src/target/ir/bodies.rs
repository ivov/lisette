use crate::target::ir::go_expression::Position;
use crate::target::ir::go_expression::{
    Expressions, FunctionLiteralLayout, GoBinaryOperator, GoExpressionNode, TreeVisitor,
};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::ir::visit::VisitorMut;
use crate::target::types::go_type::GoType;
use rustc_hash::FxHashSet;

pub(crate) fn define(name: impl Into<GoIdentifier>, value: GoExpression) -> Statement {
    LoweredStatement::Define(Definition::single(name, value)).into()
}

pub(crate) fn declare(name: impl Into<GoIdentifier>, go_type: GoType) -> Statement {
    LoweredStatement::VarDeclaration {
        name: name.into(),
        go_type,
        value: None,
    }
    .into()
}

pub(crate) fn define_many<T: Into<GoIdentifier>>(names: Vec<T>, value: GoExpression) -> Statement {
    LoweredStatement::Define(Definition {
        names: names.into_iter().map(Into::into).collect(),
        value,
    })
    .into()
}

pub(crate) fn discard(value: GoExpression) -> Statement {
    LoweredStatement::Discard { arity: 1, value }.into()
}

pub(crate) fn effect_statement(value: GoExpression) -> Statement {
    if value.stands_as_a_statement() {
        LoweredStatement::ExpressionStatement {
            expression: value,
            diverges: false,
        }
        .into()
    } else if value.constant_kind().is_some()
        || (value.effects().can_erase() && value.effects().reads().is_fixed())
    {
        LoweredStatement::Body(LoweredBlock::default()).into()
    } else {
        discard(value)
    }
}

pub(crate) fn diverging_statement(call: GoExpression) -> Statement {
    if call.stands_as_a_statement() {
        LoweredStatement::ExpressionStatement {
            expression: call,
            diverges: true,
        }
        .into()
    } else {
        effect_statement(call)
    }
}

pub(crate) fn async_statement(kind: AsyncKind, call: GoExpression) -> Statement {
    let call = if call.stands_as_a_statement() {
        call
    } else {
        GoExpression::call(
            GoExpression::unchecked_function_literal(
                Vec::new(),
                GoType::default(),
                LoweredBlock {
                    statements: vec![effect_statement(call)],
                },
                FunctionLiteralLayout::MultiLine,
            ),
            Vec::new(),
        )
    };

    LoweredStatement::Async { kind, call }.into()
}

pub(crate) fn assign(target: GoExpression, value: GoExpression) -> Statement {
    LoweredStatement::Assign(AssignForm::Simple { target, value }).into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
pub(super) struct FlowSummary {
    pub(super) falls_through: bool,
    exits: Vec<FlowExit>,
}

impl FlowSummary {
    fn next() -> Self {
        Self {
            falls_through: true,
            exits: Vec::new(),
        }
    }

    fn exit(exit: FlowExit) -> Self {
        Self {
            falls_through: false,
            exits: vec![exit],
        }
    }

    fn terminal() -> Self {
        Self {
            falls_through: false,
            exits: Vec::new(),
        }
    }

    fn sequence(mut self, next: Self) -> Self {
        if self.falls_through {
            self.exits.extend(next.exits);
        }
        self.falls_through &= next.falls_through;
        self
    }

    fn branch(mut self, other: Self) -> Self {
        self.falls_through |= other.falls_through;
        self.exits.extend(other.exits);
        self
    }

    pub(super) fn statements(statements: &[Statement]) -> Self {
        statements.iter().fold(Self::next(), |flow, statement| {
            flow.sequence(statement.kind.flow())
        })
    }

    fn consume_unlabeled_breaks(&mut self) {
        self.falls_through |= remove_unlabeled_breaks(&mut self.exits);
    }
}

fn remove_unlabeled_breaks(exits: &mut Vec<FlowExit>) -> bool {
    let had_break = exits
        .iter()
        .any(|exit| matches!(exit, FlowExit::Break(LoopTransfer::Unlabeled)));
    exits.retain(|exit| !matches!(exit, FlowExit::Break(LoopTransfer::Unlabeled)));
    had_break
}

pub(crate) fn directed_first(
    line: Option<Position>,
    mut statements: Vec<Statement>,
) -> Vec<Statement> {
    direct_first(&mut statements, line);
    statements
}

pub(crate) fn direct_first(statements: &mut [Statement], line: Option<Position>) {
    if let Some(first) = statements.first_mut()
        && first.line.is_none()
    {
        first.line = line;
    }
}

pub(crate) fn inherit_line_directives(statements: &mut [Statement], enclosing: Option<&Position>) {
    let mut previous = enclosing.cloned();
    for statement in statements {
        if statement.line.is_none() {
            statement.line.clone_from(&previous);
        }
        let line = statement.line.as_ref();
        match &mut statement.kind {
            LoweredStatement::If(plan) => inherit_into_if(plan, line),
            LoweredStatement::Select(plan) => inherit_into_select(plan, line),
            LoweredStatement::Switch(plan) => inherit_into_switch(plan, line),
            kind => {
                kind.for_each_expression_mut(&mut |value| inherit_into_expression(value, line));
                kind.for_each_block_mut(&mut |body| {
                    inherit_line_directives(&mut body.statements, line)
                });
            }
        }
        previous.clone_from(&statement.line);
    }
}

fn inherit_into_select(plan: &mut SelectStatementPlan, line: Option<&Position>) {
    for arm in &mut plan.arms {
        let arm_line = arm.line.as_ref().or(line);
        match &mut arm.kind {
            SelectArmKind::Receive { channel, .. } => inherit_into_expression(channel, arm_line),
            SelectArmKind::Send { channel, value, .. } => {
                inherit_into_expression(channel, arm_line);
                inherit_into_expression(value, arm_line);
            }
            SelectArmKind::Default { .. } => {}
        }
        inherit_line_directives(&mut arm.kind.body_mut().statements, arm_line);
        inherit_header(&mut arm.line, arm.kind.body(), line);
    }
}

fn inherit_into_switch(plan: &mut SwitchStatementPlan, line: Option<&Position>) {
    match &mut plan.kind {
        SwitchKind::Conditional => {}
        SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
            inherit_into_expression(subject, line)
        }
    }
    for case in &mut plan.cases {
        let case_line = case.line.as_ref().or(line);
        for label in &mut case.labels {
            inherit_into_expression(label, case_line);
        }
        inherit_line_directives(&mut case.body.statements, case_line);
        inherit_header(&mut case.line, &case.body, line);
    }
    if let Some(default) = &mut plan.default {
        let default_line = plan.default_line.as_ref().or(line);
        inherit_line_directives(&mut default.statements, default_line);
        inherit_header(&mut plan.default_line, default, line);
    }
}

fn inherit_into_if(plan: &mut IfPlan, line: Option<&Position>) {
    if let Some(initializer) = &mut plan.initializer {
        inherit_into_expression(&mut initializer.value, line);
    }
    inherit_into_expression(&mut plan.condition, line);
    inherit_line_directives(&mut plan.then_body.statements, line);
    match &mut plan.else_arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => {
            let else_if_line = plan.else_if_line.clone().or_else(|| line.cloned());
            inherit_into_if(plan, else_if_line.as_ref());
            inherit_header(&mut plan.else_if_line, &plan.then_body, line);
        }
        ElseArm::Else { body, .. } => inherit_line_directives(&mut body.statements, line),
    }
}

fn inherit_header(
    header: &mut Option<Position>,
    body: &LoweredBlock,
    enclosing: Option<&Position>,
) {
    if header.is_none() {
        *header = body
            .statements
            .first()
            .and_then(|statement| statement.line.as_ref())
            .or(enclosing)
            .cloned();
    }
}

fn inherit_into_expression(expression: &mut GoExpression, line: Option<&Position>) {
    fn walk(node: &mut GoExpressionNode, line: Option<&Position>) {
        if let GoExpressionNode::FunctionLiteral { body, .. } = node {
            inherit_line_directives(&mut body.statements, line);
        } else {
            node.visit_children_mut(&mut |child| walk(child, line));
        }
    }
    walk(expression.node_mut(), line);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Statement {
    pub(crate) line: Option<Position>,
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
    Body(LoweredBlock),
    Break(LoopTransfer),
    Continue(LoopTransfer),
    Const(ConstPlan),
    Return(Vec<GoExpression>),
    Assign(AssignForm),
    Async {
        kind: AsyncKind,
        call: GoExpression,
    },
    Select(SelectStatementPlan),
    Switch(SwitchStatementPlan),
    Define(Definition),
    VarDeclaration {
        name: GoIdentifier,
        go_type: GoType,
        value: Option<GoExpression>,
    },
    Discard {
        arity: usize,
        value: GoExpression,
    },
    ExpressionStatement {
        expression: GoExpression,
        diverges: bool,
    },
    UnreachablePanic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConstPlan {
    pub(crate) name: GoIdentifier,
    pub(crate) go_type: Option<GoType>,
    pub(crate) value: GoExpression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssignForm {
    Compound {
        target: GoExpression,
        kind: CompoundKind,
    },
    Simple {
        target: GoExpression,
        value: GoExpression,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AsyncKind {
    Task,
    Defer,
}

impl AsyncKind {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Task => "go",
            Self::Defer => "defer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompoundKind {
    Increment,
    Decrement,
    OperatorAssign {
        operator: GoBinaryOperator,
        rhs: GoExpression,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchStatementPlan {
    pub(crate) kind: SwitchKind,
    pub(crate) cases: Vec<SwitchCasePlan>,
    pub(crate) default: Option<LoweredBlock>,
    pub(crate) default_line: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchKind {
    Conditional,
    Value {
        subject: GoExpression,
    },
    Type {
        subject: GoExpression,
        binding: Option<GoIdentifier>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwitchCasePlan {
    pub(crate) labels: Vec<GoExpression>,
    pub(crate) body: LoweredBlock,
    pub(crate) line: Option<Position>,
}

impl SwitchStatementPlan {
    fn walk(&self, visit: &mut impl TreeVisitor) {
        match &self.kind {
            SwitchKind::Conditional => {}
            SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                subject.node().walk(visit)
            }
        }
        for case in &self.cases {
            case.walk(visit);
        }
        if let Some(default) = &self.default {
            default.walk(visit);
        }
    }

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
        }
        cases.consume_unlabeled_breaks();
        cases
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectStatementPlan {
    pub(crate) arms: Vec<SelectArmPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectArmPlan {
    pub(crate) kind: SelectArmKind,
    pub(crate) line: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectArmKind {
    Receive {
        receive_vars: Vec<GoIdentifier>,
        channel: GoExpression,
        body: LoweredBlock,
    },
    Send {
        channel: GoExpression,
        value: GoExpression,
        body: LoweredBlock,
    },
    Default {
        body: LoweredBlock,
    },
}

impl SelectStatementPlan {
    fn walk(&self, visit: &mut impl TreeVisitor) {
        for arm in &self.arms {
            match &arm.kind {
                SelectArmKind::Receive { channel, body, .. } => {
                    channel.node().walk(visit);
                    body.walk(visit);
                }
                SelectArmKind::Send {
                    channel,
                    value,
                    body,
                } => {
                    channel.node().walk(visit);
                    value.node().walk(visit);
                    body.walk(visit);
                }
                SelectArmKind::Default { body } => body.walk(visit),
            }
        }
    }

    fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut GoExpression)) {
        for arm in &mut self.arms {
            match &mut arm.kind {
                SelectArmKind::Receive { channel, .. } => visit(channel),
                SelectArmKind::Send { channel, value, .. } => {
                    visit(channel);
                    visit(value);
                }
                SelectArmKind::Default { .. } => {}
            }
        }
    }

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
        self.kind.body()
    }

    pub(crate) fn body_mut(&mut self) -> &mut LoweredBlock {
        self.kind.body_mut()
    }
}

impl SelectArmKind {
    pub(crate) fn body(&self) -> &LoweredBlock {
        match self {
            SelectArmKind::Receive { body, .. }
            | SelectArmKind::Send { body, .. }
            | SelectArmKind::Default { body } => body,
        }
    }

    pub(crate) fn body_mut(&mut self) -> &mut LoweredBlock {
        match self {
            SelectArmKind::Receive { body, .. }
            | SelectArmKind::Send { body, .. }
            | SelectArmKind::Default { body } => body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopPlan {
    pub(crate) kind: LoopKind,
    pub(crate) header: LoopHeader,
    pub(crate) body: LoweredBlock,
}

impl LoopPlan {
    fn walk(&self, visit: &mut impl TreeVisitor) {
        match &self.header {
            LoopHeader::Infinite => {}
            LoopHeader::While(condition) => condition.node().walk(visit),
            LoopHeader::Range { iterable, .. } => iterable.node().walk(visit),
            LoopHeader::Counted {
                start, condition, ..
            } => {
                start.node().walk(visit);
                if let Some(condition) = condition {
                    condition.node().walk(visit);
                }
            }
        }
        self.body.walk(visit);
    }

    fn flow(&self) -> FlowSummary {
        let mut body = FlowSummary::statements(&self.body.statements);
        let label = self.kind.label();
        let breaks = consume_loop_exits(&mut body.exits, label);
        body.falls_through = breaks || !matches!(self.header, LoopHeader::Infinite);
        body
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

impl LoopHeader {
    fn for_each_expression_mut(&mut self, visit: &mut impl FnMut(&mut GoExpression)) {
        match self {
            LoopHeader::Infinite => {}
            LoopHeader::While(condition) => visit(condition),
            LoopHeader::Range { iterable, .. } => visit(iterable),
            LoopHeader::Counted {
                start, condition, ..
            } => {
                visit(start);
                if let Some(condition) = condition {
                    visit(condition);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoopKind {
    Source { label: Option<String> },
    Generated { label: Option<String> },

    FoundFlagSearch,
}

impl LoopKind {
    pub(crate) fn label(&self) -> Option<&str> {
        match self {
            LoopKind::Source { label } | LoopKind::Generated { label } => label.as_deref(),
            LoopKind::FoundFlagSearch => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IfPlan {
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
    pub(crate) then_body: LoweredBlock,
    pub(crate) else_arm: ElseArm,
    pub(crate) else_if_line: Option<Position>,
    pub(crate) origin: IfOrigin,
}

pub(crate) struct PairCondition {
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IfOrigin {
    User,
    Match { else_label: Option<GoExpression> },
}

impl IfPlan {
    fn for_each_head_expression_mut(&mut self, visit: &mut impl FnMut(&mut GoExpression)) {
        let mut link = Some(self);
        while let Some(plan) = link {
            if let Some(initializer) = &mut plan.initializer {
                visit(&mut initializer.value);
            }
            visit(&mut plan.condition);
            link = match &mut plan.else_arm {
                ElseArm::ElseIf(next) => Some(next),
                ElseArm::None | ElseArm::Else { .. } => None,
            };
        }
    }

    pub(crate) fn plain(
        condition: GoExpression,
        then_body: LoweredBlock,
        else_arm: ElseArm,
    ) -> Self {
        Self {
            initializer: None,
            condition,
            then_body,
            else_arm,
            else_if_line: None,
            origin: IfOrigin::User,
        }
    }

    pub(crate) fn for_match(
        condition: GoExpression,
        then_body: LoweredBlock,
        else_arm: ElseArm,
    ) -> Self {
        Self {
            initializer: None,
            condition,
            then_body,
            else_arm,
            else_if_line: None,
            origin: IfOrigin::Match { else_label: None },
        }
    }

    pub(crate) fn pair_if(
        test: PairCondition,
        then_body: LoweredBlock,
        else_arm: ElseArm,
        origin: IfOrigin,
    ) -> Self {
        Self {
            initializer: test.initializer,
            condition: test.condition,
            then_body,
            else_arm,
            else_if_line: None,
            origin,
        }
    }

    pub(super) fn walk(&self, visit: &mut impl TreeVisitor) {
        if let Some(initializer) = &self.initializer {
            initializer.value.node().walk(visit);
        }
        self.condition.node().walk(visit);
        self.then_body.walk(visit);
        match &self.else_arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => plan.walk(visit),
            ElseArm::Else { body, .. } => body.walk(visit),
        }
    }

    fn flow(&self) -> FlowSummary {
        let then_flow = FlowSummary::statements(&self.then_body.statements);
        let else_flow = match &self.else_arm {
            ElseArm::None => FlowSummary::next(),
            ElseArm::ElseIf(inner) => inner.flow(),
            ElseArm::Else { body, .. } => FlowSummary::statements(&body.statements),
        };
        then_flow.branch(else_flow)
    }
}

pub(crate) fn with_setup(setup: Vec<Statement>, statement: LoweredStatement) -> LoweredStatement {
    if setup.is_empty() {
        return statement;
    }
    let mut statements = setup;
    statements.push(statement.into());
    LoweredStatement::Body(LoweredBlock { statements })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElseArm {
    None,
    ElseIf(Box<IfPlan>),
    Else {
        body: LoweredBlock,
        layout: ElseLayout,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElseLayout {
    Block,
    Inline,
}

impl ElseArm {
    pub(crate) fn block(body: LoweredBlock) -> Self {
        if body.renders_empty() {
            return ElseArm::None;
        }
        ElseArm::Else {
            body,
            layout: ElseLayout::Block,
        }
    }

    pub(crate) fn inline(body: LoweredBlock) -> Self {
        if body.renders_empty() {
            return ElseArm::None;
        }
        ElseArm::Else {
            body,
            layout: ElseLayout::Inline,
        }
    }
}

fn walk_statements(statements: &[Statement], visit: &mut impl TreeVisitor) {
    for statement in statements {
        statement.kind.walk(visit);
    }
}

pub(crate) fn rename_generated_locals(
    statements: &mut Vec<Statement>,
    by_id: &rustc_hash::FxHashMap<LocalId, String>,
) {
    struct Rename<'a>(&'a rustc_hash::FxHashMap<LocalId, String>);

    impl VisitorMut for Rename<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            if let GoExpressionNode::Identifier(name) = node
                && let Some(final_name) = name.id().and_then(|id| self.0.get(&id))
            {
                name.spelling_mut().clone_from(final_name);
            }
        }

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if let Some(final_name) = name.id().and_then(|id| self.0.get(&id)) {
                name.spelling_mut().clone_from(final_name);
            }
        }
    }

    super::visit::visit_statements_mut(statements, &mut Rename(by_id));
}

pub(crate) fn for_each_statements_mut(
    statements: &mut Vec<Statement>,
    f: &mut impl FnMut(&mut Vec<Statement>),
) {
    struct Lists<'f, F>(&'f mut F);

    impl<F: FnMut(&mut Vec<Statement>)> VisitorMut for Lists<'_, F> {
        fn expression(&mut self, _node: &mut GoExpressionNode) {}

        fn statements(&mut self, statements: &mut Vec<Statement>) {
            (self.0)(statements);
        }
    }

    super::visit::visit_statements_mut(statements, &mut Lists(f));
}

#[derive(Default)]
pub(crate) struct GoUses {
    locals: FxHashSet<LocalId>,
    spellings: FxHashSet<String>,
}

impl GoUses {
    pub(crate) fn of(statements: &[Statement]) -> Self {
        let mut uses = Self::default();
        uses.extend(statements);
        uses
    }

    pub(crate) fn extend(&mut self, statements: &[Statement]) {
        walk_statements(
            statements,
            &mut Expressions(&mut |node: &GoExpressionNode| self.record(node)),
        );
    }

    pub(crate) fn extend_cases(&mut self, cases: &[SwitchCasePlan]) {
        for case in cases {
            case.walk(&mut Expressions(&mut |node: &GoExpressionNode| {
                self.record(node)
            }));
        }
    }

    pub(crate) fn extend_if(&mut self, plan: &IfPlan) {
        plan.walk(&mut Expressions(&mut |node: &GoExpressionNode| {
            self.record(node)
        }));
    }

    pub(crate) fn contains_identifier(&self, name: &GoIdentifier) -> bool {
        let id = name.id().expect("local-use queries need a local ID");
        self.locals.contains(&id)
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.spellings.contains(name)
    }

    fn record(&mut self, node: &GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = node {
            assert!(
                !name.is_pending(),
                "local-use analysis needs identified reads"
            );
            self.spellings.insert(name.spelling().to_string());
            if let Some(id) = name.id() {
                self.locals.insert(id);
            }
        }
    }
}

impl LoweredBlock {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        self.walk(&mut Expressions(visit));
    }

    pub(crate) fn walk(&self, visit: &mut impl TreeVisitor) {
        walk_statements(&self.statements, visit);
    }

    pub(crate) fn ends_with_diverge(&self) -> bool {
        !FlowSummary::statements(&self.statements).falls_through
    }

    pub(crate) fn renders_empty(&self) -> bool {
        self.statements
            .iter()
            .all(|statement| !statement.kind.emits_output())
    }
}

impl LoweredStatement {
    pub(crate) fn visit_expressions(&self, visit: &mut impl FnMut(&GoExpressionNode)) {
        self.walk(&mut Expressions(visit));
    }

    pub(crate) fn own_types(&self, visit: &mut impl FnMut(&GoType)) {
        match self {
            LoweredStatement::VarDeclaration { go_type, .. }
            | LoweredStatement::Const(ConstPlan {
                go_type: Some(go_type),
                ..
            }) => visit(go_type),
            _ => {}
        }
    }

    pub(crate) fn walk(&self, visit: &mut impl TreeVisitor) {
        visit.statement(self);
        match self {
            LoweredStatement::If(plan) => plan.walk(visit),
            LoweredStatement::Loop(plan) => plan.walk(visit),
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => body.walk(visit),
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::UnreachablePanic => {}
            LoweredStatement::Const(plan) => plan.value.node().walk(visit),
            LoweredStatement::Return(values) => {
                for value in values {
                    value.node().walk(visit);
                }
            }
            LoweredStatement::Assign(form) => match form {
                AssignForm::Compound { target, kind } => {
                    target.node().walk(visit);
                    if let CompoundKind::OperatorAssign { rhs, .. } = kind {
                        rhs.node().walk(visit);
                    }
                }
                AssignForm::Simple { target, value } => {
                    target.node().walk(visit);
                    value.node().walk(visit);
                }
            },
            LoweredStatement::Async { call, .. } => call.node().walk(visit),
            LoweredStatement::Select(plan) => plan.walk(visit),
            LoweredStatement::Switch(plan) => plan.walk(visit),
            LoweredStatement::Define(definition) => definition.value.node().walk(visit),
            LoweredStatement::VarDeclaration { value, .. } => {
                if let Some(value) = value {
                    value.node().walk(visit);
                }
            }
            LoweredStatement::Discard {
                value: expression, ..
            }
            | LoweredStatement::ExpressionStatement { expression, .. } => {
                expression.node().walk(visit)
            }
        }
    }

    pub(crate) fn for_each_expression_mut(&mut self, visit: &mut impl FnMut(&mut GoExpression)) {
        match self {
            LoweredStatement::If(plan) => plan.for_each_head_expression_mut(visit),
            LoweredStatement::Loop(plan) => plan.header.for_each_expression_mut(visit),
            LoweredStatement::Block(_)
            | LoweredStatement::Body(_)
            | LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::VarDeclaration { value: None, .. }
            | LoweredStatement::UnreachablePanic => {}
            LoweredStatement::Return(values) => values.iter_mut().for_each(visit),
            LoweredStatement::Assign(AssignForm::Compound { target, kind }) => {
                visit(target);
                if let CompoundKind::OperatorAssign { rhs, .. } = kind {
                    visit(rhs);
                }
            }
            LoweredStatement::Assign(AssignForm::Simple { target, value }) => {
                visit(target);
                visit(value);
            }
            LoweredStatement::Select(plan) => plan.for_each_operand_mut(visit),
            LoweredStatement::Switch(plan) => {
                match &mut plan.kind {
                    SwitchKind::Conditional => {}
                    SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                        visit(subject)
                    }
                }
                for case in &mut plan.cases {
                    case.labels.iter_mut().for_each(&mut *visit);
                }
            }
            LoweredStatement::Const(ConstPlan { value, .. })
            | LoweredStatement::Define(Definition { value, .. })
            | LoweredStatement::VarDeclaration {
                value: Some(value), ..
            }
            | LoweredStatement::Async { call: value, .. }
            | LoweredStatement::Discard { value, .. }
            | LoweredStatement::ExpressionStatement {
                expression: value, ..
            } => visit(value),
        }
    }

    pub(crate) fn for_each_block_mut(&mut self, visit: &mut impl FnMut(&mut LoweredBlock)) {
        match self {
            LoweredStatement::If(plan) => {
                let mut link = Some(plan);
                while let Some(plan) = link {
                    visit(&mut plan.then_body);
                    link = match &mut plan.else_arm {
                        ElseArm::ElseIf(next) => Some(next),
                        ElseArm::Else { body, .. } => {
                            visit(body);
                            None
                        }
                        ElseArm::None => None,
                    };
                }
            }
            LoweredStatement::Loop(plan) => visit(&mut plan.body),
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => visit(body),
            LoweredStatement::Select(plan) => {
                for arm in &mut plan.arms {
                    visit(arm.body_mut());
                }
            }
            LoweredStatement::Switch(plan) => {
                for case in &mut plan.cases {
                    visit(&mut case.body);
                }
                if let Some(default) = &mut plan.default {
                    visit(default);
                }
            }
            LoweredStatement::Break(_)
            | LoweredStatement::Continue(_)
            | LoweredStatement::Const(_)
            | LoweredStatement::Return(_)
            | LoweredStatement::Assign(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Define(_)
            | LoweredStatement::VarDeclaration { .. }
            | LoweredStatement::Discard { .. }
            | LoweredStatement::ExpressionStatement { .. }
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    pub(crate) fn binds_local(&self, local: &GoIdentifier) -> bool {
        local.id().is_some_and(|id| match self {
            LoweredStatement::Define(Definition { names, .. }) => {
                names.iter().any(|name| name.id() == Some(id))
            }
            LoweredStatement::VarDeclaration { name, .. } => name.id() == Some(id),
            _ => false,
        })
    }

    pub(crate) fn rename_bound_name(&mut self, go_name: &GoIdentifier) -> bool {
        let bound = match self {
            LoweredStatement::Define(Definition { names, .. }) => match names.as_mut_slice() {
                [name] => name,
                _ => return false,
            },
            LoweredStatement::VarDeclaration {
                name,
                value: Some(_),
                ..
            } => name,
            _ => return false,
        };

        *bound = go_name.clone();
        true
    }

    pub(crate) fn emits_output(&self) -> bool {
        match self {
            LoweredStatement::Body(body) => !body.renders_empty(),
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
            | LoweredStatement::VarDeclaration { .. }
            | LoweredStatement::Discard { .. }
            | LoweredStatement::UnreachablePanic
            | LoweredStatement::Return(_) => true,
            LoweredStatement::Assign(plan) => match plan {
                AssignForm::Compound { .. } | AssignForm::Simple { .. } => true,
            },
            LoweredStatement::ExpressionStatement { expression, .. } => !expression.is_empty(),
        }
    }

    pub(super) fn flow(&self) -> FlowSummary {
        match self {
            LoweredStatement::If(plan) => plan.flow(),
            LoweredStatement::Loop(plan) => plan.flow(),
            LoweredStatement::Block(body) | LoweredStatement::Body(body) => {
                FlowSummary::statements(&body.statements)
            }
            LoweredStatement::Break(target) => FlowSummary::exit(FlowExit::Break(target.clone())),
            LoweredStatement::Continue(target) => {
                FlowSummary::exit(FlowExit::Continue(target.clone()))
            }
            LoweredStatement::Return(_)
            | LoweredStatement::ExpressionStatement { diverges: true, .. }
            | LoweredStatement::UnreachablePanic => FlowSummary::terminal(),
            LoweredStatement::Select(plan) => plan.flow(),
            LoweredStatement::Switch(plan) => plan.flow(),
            LoweredStatement::Assign(_)
            | LoweredStatement::Async { .. }
            | LoweredStatement::Const(_)
            | LoweredStatement::Define(_)
            | LoweredStatement::VarDeclaration { .. }
            | LoweredStatement::Discard { .. }
            | LoweredStatement::ExpressionStatement { .. } => FlowSummary::next(),
        }
    }

    pub(crate) fn blocks_fallthrough(&self) -> bool {
        !self.flow().falls_through
    }
}

impl SwitchCasePlan {
    pub(super) fn walk(&self, visit: &mut impl TreeVisitor) {
        for label in &self.labels {
            label.node().walk(visit);
        }
        self.body.walk(visit);
    }
}
