//! Lowered body IR: the typed vocabulary `plan::lower` produces and `render/`
//! consumes. `RawGo` is a transitional node holding pre-rendered Go.

use crate::ReturnContext;
use crate::plan::values::ValuePlan;
use crate::utils::{output_ends_with_diverge, output_references_var};
use syntax::types::Type;

/// Destination for a lowered block's tail. Every variant carries the enclosing
/// function's `return_ctx` so nested `return`/`?` lower correctly regardless of
/// the place's own destination; for `Return` it is also the tail target.
pub(crate) enum PlacePlan<'a> {
    Statement {
        return_ctx: &'a ReturnContext,
    },
    Return(&'a ReturnContext),
    Assign {
        local: &'a str,
        target_ty: Option<&'a Type>,
        return_ctx: &'a ReturnContext,
    },
}

impl<'a> PlacePlan<'a> {
    pub(crate) fn is_return(&self) -> bool {
        matches!(self, PlacePlan::Return(_))
    }

    /// Construct a statement-position place for the given enclosing context.
    pub(crate) fn statement(return_ctx: &'a ReturnContext) -> Self {
        PlacePlan::Statement { return_ctx }
    }

    /// The enclosing function's return context (for nested `return`/`?`).
    pub(crate) fn return_ctx(&self) -> &'a ReturnContext {
        match self {
            PlacePlan::Statement { return_ctx }
            | PlacePlan::Return(return_ctx)
            | PlacePlan::Assign { return_ctx, .. } => return_ctx,
        }
    }
}

pub(crate) struct LoweredBlock {
    pub(crate) statements: Vec<LoweredStatement>,
}

pub(crate) enum LoweredStatement {
    If(IfPlan),
    Loop(LoopPlan),
    Block(LoweredBlock),
    Break {
        directive: String,
        label: Option<String>,
    },
    Continue {
        directive: String,
        label: Option<String>,
    },
    Const(ConstPlan),
    Return(ReturnStatementPlan),
    BreakValue(BreakValuePlan),
    Let(LetPlan),
    Assign(AssignPlan),
    Expression(ExpressionStatementPlan),
    Match(MatchStatementPlan),
    Select(SelectStatementPlan),
    Switch(SwitchStatementPlan),
    WhileLet(WhileLetPlan),
    /// Eval-order temp capture: `name := value`.
    TempBind {
        name: String,
        value: String,
    },
    /// `name := <closure_open><body><closure_close>` (try-block IIFE,
    /// recover-block closure). `closure_open`/`close` are opaque Go text.
    ClosureBind {
        name: String,
        closure_open: String,
        body: LoweredBlock,
        closure_close: String,
    },
    RawGo(String),
}

/// A source `const` (or `var` when the value is not Go-const-eligible).
pub(crate) struct ConstPlan {
    pub(crate) directive: String,
    pub(crate) is_const: bool,
    pub(crate) name: String,
    pub(crate) ty_str: String,
    pub(crate) value: ValuePlan,
}

/// A source `return expr` statement, classified by `ReturnForm`.
pub(crate) struct ReturnStatementPlan {
    pub(crate) directive: String,
    pub(crate) form: ReturnForm,
}

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
        values: Vec<String>,
    },
    /// `PartialTuple`/`Tuple` tail destructure: tag-check `IfPlan`s and
    /// `Return` leaves built by `abi_transition`.
    LoweredAbi {
        body: LoweredBlock,
    },
    /// `Result`/`Option`-wrapped return through `FalliblePlanner`.
    Wrapped {
        body: LoweredBlock,
    },
}

/// `break value` statement. `disposition` decides how the value text
/// reaches the loop-result slot (or is discarded); a trailing `break`
/// terminates unless the value already diverged.
pub(crate) struct BreakValuePlan {
    pub(crate) directive: String,
    pub(crate) value: ValuePlan,
    pub(crate) disposition: BreakValueDisposition,
    pub(crate) label: Option<String>,
}

/// What to do with the value of a `break value` statement after its setup
/// has run.
pub(crate) enum BreakValueDisposition {
    /// The value diverged (empty `Propagate`); no further code is emitted
    /// and the `break` is skipped because the value already terminates.
    Diverged,
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

/// A `let` binding, classified by shape.
pub(crate) struct LetPlan {
    pub(crate) directive: String,
    pub(crate) form: LetForm,
}

pub(crate) enum LetForm {
    /// `!`-typed value. `declaration` is the optional `var X T` line so dead code
    /// can still reference the binding.
    Never {
        declaration: Option<String>,
        body: LoweredBlock,
    },
    /// `let x = value` (single identifier), including `let x = expr?`.
    SimpleIdentifier {
        body: LoweredBlock,
    },
    /// `let _ = value` or all-unused tuple destructure.
    Discard {
        body: LoweredBlock,
    },
    ComplexPattern {
        body: LoweredBlock,
    },
    /// `let (a, b) = go_call()`.
    MultiValueCall {
        body: LoweredBlock,
    },
    LetElse {
        body: LoweredBlock,
    },
}

impl LetForm {
    pub(crate) fn body(&self) -> &LoweredBlock {
        match self {
            LetForm::Never { body, .. }
            | LetForm::SimpleIdentifier { body }
            | LetForm::Discard { body }
            | LetForm::ComplexPattern { body }
            | LetForm::MultiValueCall { body }
            | LetForm::LetElse { body } => body,
        }
    }
}

/// An assignment statement, structured by shape.
pub(crate) struct AssignPlan {
    pub(crate) directive: String,
    pub(crate) form: AssignForm,
}

pub(crate) enum AssignForm {
    /// `target++`, `target--`, or `target op= rhs`.
    Compound {
        target_capture: Vec<LoweredStatement>,
        target_str: String,
        kind: CompoundKind,
    },
    /// `target = value`.
    Simple {
        target_capture: Vec<LoweredStatement>,
        target_str: String,
        value: ValuePlan,
    },
    /// `target = nil` — clearing a Go-imported nullable field with `None`.
    NilClear {
        target_capture: Vec<LoweredStatement>,
        target_str: String,
    },
    /// `_ = expr` discard (drops lowered multi-return values).
    Discard { body: LoweredBlock },
    /// The RHS diverges (never-typed); emitted as a statement.
    NeverTyped { body: LoweredBlock },
}

pub(crate) enum CompoundKind {
    Increment,
    Decrement,
    /// `target op= rhs`. `op_text` is the rendered Go operator.
    OpAssign {
        op_text: String,
        rhs: ValuePlan,
    },
}

/// A bare expression statement.
pub(crate) struct ExpressionStatementPlan {
    pub(crate) directive: String,
    pub(crate) form: ExpressionStatementForm,
}

pub(crate) enum ExpressionStatementForm {
    /// `go <value>` / `defer <value>` at statement position.
    Async {
        value: ValuePlan,
    },
    /// `<keyword> func() { <body> }()` IIFE wrapper for Task/Defer block
    /// forms and inner expressions requiring an IIFE (`needs_iife_for_async`).
    AsyncBlock {
        keyword: String,
        body: LoweredBlock,
    },
    /// `expr?` in statement position (discards the ok value).
    Propagate {
        body: LoweredBlock,
    },
    Discard {
        body: LoweredBlock,
    },
}

pub(crate) struct MatchStatementPlan {
    pub(crate) directive: String,
    pub(crate) body: LoweredBlock,
}

/// A `switch` statement (value or type switch). The renderer owns the
/// `switch`/`case`/`default:` syntax.
pub(crate) struct SwitchStatementPlan {
    pub(crate) directive: String,
    pub(crate) kind: SwitchKind,
    pub(crate) cases: Vec<SwitchCasePlan>,
    pub(crate) default: Option<LoweredBlock>,
    /// Statements after the switch, such as an unreachable panic.
    pub(crate) postlude: Vec<LoweredStatement>,
}

pub(crate) enum SwitchKind {
    /// `switch <subject> {`
    Value { subject: String },
    /// `switch <binding> := <subject>.(type) {` when `binding` is set,
    /// otherwise `switch <subject>.(type) {`.
    Type {
        subject: String,
        binding: Option<String>,
    },
}

/// A single `case <labels>:` plus its body.
pub(crate) struct SwitchCasePlan {
    pub(crate) labels: String,
    pub(crate) body: LoweredBlock,
}

impl SwitchStatementPlan {
    fn references_var(&self, var: &str) -> bool {
        let kind_references = match &self.kind {
            SwitchKind::Value { subject } => output_references_var(subject, var),
            SwitchKind::Type { subject, binding } => {
                output_references_var(subject, var)
                    || binding
                        .as_deref()
                        .is_some_and(|binding| output_references_var(binding, var))
            }
        };
        kind_references
            || self.cases.iter().any(|case| case.references_var(var))
            || self
                .default
                .as_ref()
                .is_some_and(|body| body.references_var(var))
            || self.postlude.iter().any(|s| s.references_var(var))
    }

    fn ends_with_diverge(&self) -> bool {
        self.postlude
            .last()
            .is_some_and(LoweredStatement::ends_with_diverge)
    }
}

impl SwitchCasePlan {
    pub(crate) fn references_var(&self, var: &str) -> bool {
        output_references_var(&self.labels, var) || self.body.references_var(var)
    }
}

/// A `select` statement: optional retry-loop wrapper around the `select`, an
/// ordered set of arms, plus hoisted setup and a trailing postlude (e.g. an
/// unreachable panic). The renderer owns the `for`/`select`/`case`/`default:`
/// syntax.
pub(crate) struct SelectStatementPlan {
    pub(crate) directive: String,
    /// Side-effecting setup hoisted before the `select` (channel/value temps).
    pub(crate) setup: Vec<LoweredStatement>,
    /// When set, the `select` is wrapped in `for { ... break }` for retry.
    pub(crate) retry_loop: bool,
    pub(crate) arms: Vec<SelectArmPlan>,
    /// Statements after the `select`/retry loop, such as an unreachable panic.
    pub(crate) postlude: Vec<LoweredStatement>,
}

/// A single `select` arm: a `case`/`default:` header plus its body block.
pub(crate) enum SelectArmPlan {
    /// `case <receive_vars> := <-<channel>:`, or `case <-<channel>:` when
    /// `receive_vars` is `None`.
    Receive {
        receive_vars: Option<String>,
        channel: String,
        body: LoweredBlock,
    },
    /// `case <operation>:` where `operation` is `ch <- val` or `<-ch`.
    Send {
        operation: String,
        body: LoweredBlock,
    },
    /// `default:`
    Default { body: LoweredBlock },
}

impl SelectStatementPlan {
    fn references_var(&self, var: &str) -> bool {
        self.setup.iter().any(|s| s.references_var(var))
            || self.arms.iter().any(|arm| arm.references_var(var))
            || self.postlude.iter().any(|s| s.references_var(var))
    }

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

    fn references_var(&self, var: &str) -> bool {
        let header_references = match self {
            SelectArmPlan::Receive {
                receive_vars,
                channel,
                ..
            } => {
                receive_vars
                    .as_deref()
                    .is_some_and(|vars| output_references_var(vars, var))
                    || output_references_var(channel, var)
            }
            SelectArmPlan::Send { operation, .. } => output_references_var(operation, var),
            SelectArmPlan::Default { .. } => false,
        };
        header_references || self.body().references_var(var)
    }
}

pub(crate) struct WhileLetPlan {
    pub(crate) directive: String,
    pub(crate) body: LoweredBlock,
}

/// A statement-position loop. `prologue` is pre-loop setup (a for-loop's
/// iterable capture); `header` is the rendered Go loop opener through the body's
/// opening brace; `label` is the optional break/continue label.
pub(crate) struct LoopPlan {
    pub(crate) directive: String,
    pub(crate) prologue: String,
    pub(crate) label: Option<String>,
    pub(crate) header: String,
    pub(crate) body: LoweredBlock,
}

pub(crate) struct IfPlan {
    /// Empty unless debug; set only on the leading `if`, never on nested
    /// `else if`s.
    pub(crate) directive: String,
    pub(crate) condition_setup: String,
    pub(crate) condition: String,
    pub(crate) then_body: LoweredBlock,
    pub(crate) else_arm: ElseArm,
}

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

impl LoweredBlock {
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

    /// Whether `var` appears as a standalone identifier in the rendered
    /// block.
    #[allow(dead_code)]
    pub(crate) fn references_var(&self, var: &str) -> bool {
        self.statements
            .iter()
            .any(|statement| statement.references_var(var))
    }
}

impl LoweredStatement {
    fn ends_with_diverge(&self) -> bool {
        match self {
            LoweredStatement::If(plan) => plan.ends_with_diverge(),
            LoweredStatement::Loop(_) | LoweredStatement::Block(_) | LoweredStatement::Const(_) => {
                false
            }
            LoweredStatement::Break { .. } | LoweredStatement::Continue { .. } => true,
            LoweredStatement::Return(_) => true,
            LoweredStatement::BreakValue(_) => true,
            LoweredStatement::Let(plan) => plan.form.body().ends_with_diverge(),
            LoweredStatement::Assign(plan) => match &plan.form {
                AssignForm::Compound { .. }
                | AssignForm::Simple { .. }
                | AssignForm::NilClear { .. } => false,
                AssignForm::Discard { body } | AssignForm::NeverTyped { body } => {
                    body.ends_with_diverge()
                }
            },
            LoweredStatement::Expression(plan) => match &plan.form {
                ExpressionStatementForm::Async { .. }
                | ExpressionStatementForm::AsyncBlock { .. } => false,
                ExpressionStatementForm::Propagate { body }
                | ExpressionStatementForm::Discard { body } => body.ends_with_diverge(),
            },
            LoweredStatement::Match(plan) => plan.body.ends_with_diverge(),
            LoweredStatement::Select(plan) => plan.ends_with_diverge(),
            LoweredStatement::Switch(plan) => plan.ends_with_diverge(),
            LoweredStatement::WhileLet(plan) => plan.body.ends_with_diverge(),
            LoweredStatement::TempBind { .. } | LoweredStatement::ClosureBind { .. } => false,
            LoweredStatement::RawGo(code) => output_ends_with_diverge(code),
        }
    }

    pub(crate) fn blocks_fallthrough(&self) -> bool {
        !matches!(self, LoweredStatement::WhileLet(_)) && self.ends_with_diverge()
    }

    pub(crate) fn references_var(&self, var: &str) -> bool {
        match self {
            LoweredStatement::If(plan) => plan.references_var(var),
            LoweredStatement::Loop(plan) => plan.references_var(var),
            LoweredStatement::Block(body) => body.references_var(var),
            LoweredStatement::Break { label, .. } | LoweredStatement::Continue { label, .. } => {
                label.as_deref() == Some(var)
            }
            LoweredStatement::Const(plan) => plan.value.references_var(var),
            LoweredStatement::Return(plan) => match &plan.form {
                ReturnForm::Plain { value } => value.references_var(var),
                ReturnForm::Unit { side_effect } => {
                    side_effect.as_ref().is_some_and(|b| b.references_var(var))
                }
                ReturnForm::Multi { values } => {
                    values.iter().any(|v| output_references_var(v, var))
                }
                ReturnForm::LoweredAbi { body } | ReturnForm::Wrapped { body } => {
                    body.references_var(var)
                }
            },
            LoweredStatement::BreakValue(plan) => {
                if plan.value.references_var(var) {
                    return true;
                }
                match &plan.disposition {
                    BreakValueDisposition::UnitCallIntoResult { result_var }
                    | BreakValueDisposition::AssignToResult { result_var } => result_var == var,
                    BreakValueDisposition::Diverged | BreakValueDisposition::Discard => false,
                }
            }
            LoweredStatement::Let(plan) => {
                let declaration_ref = match &plan.form {
                    LetForm::Never { declaration, .. } => declaration
                        .as_ref()
                        .is_some_and(|s| output_references_var(s, var)),
                    _ => false,
                };
                declaration_ref || plan.form.body().references_var(var)
            }
            LoweredStatement::Assign(plan) => match &plan.form {
                AssignForm::Compound {
                    target_capture,
                    target_str,
                    kind,
                } => {
                    if target_capture.iter().any(|s| s.references_var(var)) {
                        return true;
                    }
                    if output_references_var(target_str, var) {
                        return true;
                    }
                    match kind {
                        CompoundKind::Increment | CompoundKind::Decrement => false,
                        CompoundKind::OpAssign { rhs, .. } => rhs.references_var(var),
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    target_str,
                    value,
                } => {
                    target_capture.iter().any(|s| s.references_var(var))
                        || output_references_var(target_str, var)
                        || value.references_var(var)
                }
                AssignForm::NilClear {
                    target_capture,
                    target_str,
                } => {
                    target_capture.iter().any(|s| s.references_var(var))
                        || output_references_var(target_str, var)
                }
                AssignForm::Discard { body } | AssignForm::NeverTyped { body } => {
                    body.references_var(var)
                }
            },
            LoweredStatement::Expression(plan) => match &plan.form {
                ExpressionStatementForm::Async { value } => value.references_var(var),
                ExpressionStatementForm::AsyncBlock { body, .. } => body.references_var(var),
                ExpressionStatementForm::Propagate { body }
                | ExpressionStatementForm::Discard { body } => body.references_var(var),
            },
            LoweredStatement::Match(plan) => plan.body.references_var(var),
            LoweredStatement::Select(plan) => plan.references_var(var),
            LoweredStatement::Switch(plan) => plan.references_var(var),
            LoweredStatement::WhileLet(plan) => plan.body.references_var(var),
            LoweredStatement::TempBind { name, value } => {
                output_references_var(name, var) || output_references_var(value, var)
            }
            LoweredStatement::ClosureBind {
                name,
                closure_open,
                body,
                closure_close,
            } => {
                output_references_var(name, var)
                    || output_references_var(closure_open, var)
                    || body.references_var(var)
                    || output_references_var(closure_close, var)
            }
            LoweredStatement::RawGo(code) => output_references_var(code, var),
        }
    }
}

impl IfPlan {
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

    #[allow(dead_code)]
    fn references_var(&self, var: &str) -> bool {
        output_references_var(&self.condition_setup, var)
            || output_references_var(&self.condition, var)
            || self.then_body.references_var(var)
            || match &self.else_arm {
                ElseArm::None => false,
                ElseArm::ElseIf(plan) => plan.references_var(var),
                ElseArm::Else { body, .. } => body.references_var(var),
            }
    }
}

impl LoopPlan {
    #[allow(dead_code)]
    fn references_var(&self, var: &str) -> bool {
        output_references_var(&self.prologue, var)
            || output_references_var(&self.header, var)
            || self.body.references_var(var)
    }
}
