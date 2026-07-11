use crate::plan::bodies::{
    AssignForm, BreakValueDisposition, CompoundKind, ElseArm, ExpressionStatementForm, IfPlan,
    LetForm, LoopId, LoopPlan, LoweredBlock, LoweredStatement, ReturnForm,
};
use crate::plan::values::ValuePlan;

#[derive(Clone, Copy, Default)]
struct Interception {
    break_transfer: bool,
    continue_transfer: bool,
}

impl Interception {
    fn all() -> Self {
        Self {
            break_transfer: true,
            continue_transfer: true,
        }
    }

    fn breakable(self) -> Self {
        Self {
            break_transfer: true,
            ..self
        }
    }
}

pub(crate) fn legalize_source_loop(plan: &mut LoopPlan) {
    let Some(target) = plan.target else {
        return;
    };
    debug_assert!(plan.label.is_none());
    let mut legalizer = Legalizer::new(target);
    legalizer.walk_block(&mut plan.body, Interception::default());
    plan.label = legalizer.label;
}

struct Legalizer {
    target: LoopId,
    label: Option<String>,
}

impl Legalizer {
    fn new(target: LoopId) -> Self {
        Self {
            target,
            label: None,
        }
    }

    fn walk_block(&mut self, block: &mut LoweredBlock, interception: Interception) {
        self.walk_statements(&mut block.statements, interception);
    }

    fn walk_statements(&mut self, statements: &mut [LoweredStatement], interception: Interception) {
        for statement in statements {
            self.walk_statement(statement, interception);
        }
    }

    fn walk_value(&mut self, value: &mut ValuePlan, interception: Interception) {
        self.walk_statements(&mut value.setup, interception);
    }

    fn resolve_transfer(
        &mut self,
        transfer_target: Option<LoopId>,
        resolved_label: &mut Option<String>,
        intercepted: bool,
    ) {
        if transfer_target != Some(self.target) {
            return;
        }
        if intercepted {
            let label_number = self.target.0 + 1;
            let label = self
                .label
                .get_or_insert_with(|| format!("loop_{label_number}"));
            *resolved_label = Some(label.clone());
        } else {
            *resolved_label = None;
        }
    }

    fn walk_statement(&mut self, statement: &mut LoweredStatement, interception: Interception) {
        match statement {
            LoweredStatement::If(plan) => self.walk_if(plan, interception),
            LoweredStatement::Loop(plan) => {
                self.walk_statements(&mut plan.prologue, interception);
                if plan.target.is_none() {
                    self.walk_block(&mut plan.body, Interception::all());
                }
            }
            LoweredStatement::Block(body) => self.walk_block(body, interception),
            LoweredStatement::Break { target, label } => {
                self.resolve_transfer(*target, label, interception.break_transfer)
            }
            LoweredStatement::Continue { target, label } => {
                self.resolve_transfer(*target, label, interception.continue_transfer)
            }
            LoweredStatement::Const(plan) => self.walk_value(&mut plan.value, interception),
            LoweredStatement::Return(plan) => match &mut plan.form {
                ReturnForm::Plain { value } => self.walk_value(value, interception),
                ReturnForm::Unit { side_effect } => {
                    if let Some(body) = side_effect {
                        self.walk_block(body, interception);
                    }
                }
                ReturnForm::LoweredAbi { body } | ReturnForm::Wrapped { body } => {
                    self.walk_block(body, interception)
                }
                ReturnForm::Multi { .. } => {}
            },
            LoweredStatement::BreakValue(plan) => {
                self.walk_value(&mut plan.value, interception);
                if !matches!(&plan.disposition, BreakValueDisposition::Diverged) {
                    self.resolve_transfer(
                        plan.target,
                        &mut plan.label,
                        interception.break_transfer,
                    );
                }
            }
            LoweredStatement::Let(plan) => {
                if let LetForm::Never {
                    declaration: Some(declaration),
                    ..
                } = &mut plan.form
                {
                    self.walk_statement(declaration, interception);
                }
                self.walk_block(plan.form.body_mut(), interception);
            }
            LoweredStatement::Assign(plan) => match &mut plan.form {
                AssignForm::Compound {
                    target_capture,
                    kind,
                    ..
                } => {
                    self.walk_statements(target_capture, interception);
                    if let CompoundKind::OpAssign { rhs, .. } = kind {
                        self.walk_value(rhs, interception);
                    }
                }
                AssignForm::Simple {
                    target_capture,
                    value,
                    ..
                } => {
                    self.walk_statements(target_capture, interception);
                    self.walk_value(value, interception);
                }
                AssignForm::Discard { body } | AssignForm::NeverTyped { body } => {
                    self.walk_block(body, interception)
                }
            },
            LoweredStatement::Expression(plan) => match &mut plan.form {
                ExpressionStatementForm::Async { value } => self.walk_value(value, interception),
                ExpressionStatementForm::AsyncBlock { .. } => {}
                ExpressionStatementForm::Propagate { body }
                | ExpressionStatementForm::Discard { body } => self.walk_block(body, interception),
            },
            LoweredStatement::Match(plan) => self.walk_block(&mut plan.body, interception),
            LoweredStatement::Select(plan) => {
                self.walk_statements(&mut plan.setup, interception);
                let arm_interception = if plan.retry_loop {
                    Interception::all()
                } else {
                    interception.breakable()
                };
                for arm in &mut plan.arms {
                    self.walk_block(arm.body_mut(), arm_interception);
                }
                self.walk_statements(&mut plan.postlude, interception);
            }
            LoweredStatement::Switch(plan) => {
                let case_interception = interception.breakable();
                for case in &mut plan.cases {
                    self.walk_block(&mut case.body, case_interception);
                }
                if let Some(body) = &mut plan.default {
                    self.walk_block(body, case_interception);
                }
                self.walk_statements(&mut plan.postlude, interception);
            }
            LoweredStatement::WhileLet(plan) => self.walk_block(&mut plan.body, interception),
            LoweredStatement::Directed { inner, .. } => self.walk_statement(inner, interception),
            LoweredStatement::TempBind { .. }
            | LoweredStatement::VarDecl { .. }
            | LoweredStatement::ClosureBind { .. }
            | LoweredStatement::RawGo(_)
            | LoweredStatement::DivergingRawGo(_)
            | LoweredStatement::UnreachablePanic => {}
        }
    }

    fn walk_else(&mut self, arm: &mut ElseArm, interception: Interception) {
        match arm {
            ElseArm::None => {}
            ElseArm::ElseIf(plan) => self.walk_if(plan, interception),
            ElseArm::Else { body, .. } => self.walk_block(body, interception),
        }
    }

    fn walk_if(&mut self, plan: &mut IfPlan, interception: Interception) {
        self.walk_statements(&mut plan.condition_setup, interception);
        self.walk_block(&mut plan.then_body, interception);
        self.walk_else(&mut plan.else_arm, interception);
    }
}
