use crate::target::ir::values::GoExpression;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::ast::Expression;
use syntax::types::Type;

use crate::lookup::facts::emit_facts::EmitFacts;

use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};
use crate::lowering::expressions::operand_order::VariadicCombine;
use syntax::program::SlotOrigin;

impl CallableReturnAbi {
    pub(in crate::lowering) fn fits_return_type(&self, facts: &EmitFacts<'_>, ty: &Type) -> bool {
        match self.lowered() {
            None => true,
            Some(LoweredReturnAbi::BareError | LoweredReturnAbi::Result { .. }) => {
                facts.is_result(ty)
            }
            Some(LoweredReturnAbi::Partial { .. }) => facts.is_partial(ty),
            Some(LoweredReturnAbi::Option(_)) => facts.is_option(ty),
            Some(LoweredReturnAbi::Tuple { .. }) => facts
                .tuple_elements(ty)
                .is_some_and(|elements| elements.len() >= 2),
        }
    }
}

impl LoweredReturnAbi {
    pub(in crate::lowering) fn ok_values(
        &self,
        mut payload: Vec<GoExpression>,
    ) -> Vec<GoExpression> {
        match self {
            LoweredReturnAbi::BareError => vec![GoExpression::nil()],
            LoweredReturnAbi::Result { .. } | LoweredReturnAbi::Partial { .. } => {
                payload.push(GoExpression::nil());
                payload
            }
            LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }) => {
                payload.push(GoExpression::boolean(true));
                payload
            }
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_)) => {
                payload
            }
            LoweredReturnAbi::Tuple { .. } => {
                unreachable!("a tuple return has its own emission path")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct CallableParamAbi {
    pub(in crate::lowering) instantiated: Type,
    pub(in crate::lowering) declared: Option<Type>,
    pub(in crate::lowering) origin: SlotOrigin,
    pub(in crate::lowering) layout: ValueLayout,
    pub(in crate::lowering) variadic: Option<ValueLayout>,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct CallableAbi {
    pub(in crate::lowering) params: Vec<CallableParamAbi>,
    pub(in crate::lowering) result: CallableReturnAbi,
    pub(in crate::lowering) return_layout: ValueLayout,
    pub(in crate::lowering) result_declaration: Option<Type>,
}

impl CallableAbi {
    pub(in crate::lowering) fn param(&self, index: usize) -> Option<&CallableParamAbi> {
        self.params.get(index).or_else(|| self.variadic_param())
    }

    pub(in crate::lowering) fn variadic_param(&self) -> Option<&CallableParamAbi> {
        self.params.last().filter(|param| param.variadic.is_some())
    }

    pub(in crate::lowering) fn spread_wraps_to_any(
        &self,
        facts: &EmitFacts<'_>,
        spread: Option<&Expression>,
    ) -> bool {
        let (Some(spread), Some(variadic)) = (spread, self.variadic_param()) else {
            return false;
        };
        facts.resolves_to_unknown(&variadic.instantiated)
            && spread
                .get_type()
                .inner()
                .is_some_and(|ty| !facts.resolves_to_unknown(&ty))
    }

    pub(in crate::lowering) fn variadic_combine(
        &self,
        extra_leading: usize,
    ) -> Option<VariadicCombine> {
        let variadic = self.variadic_param()?;
        Some(VariadicCombine {
            element_ty: variadic.instantiated.clone(),
            fixed_count: self.params.len() - 1 + extra_leading,
        })
    }

    pub(in crate::lowering) fn function_layout(&self, facts: &EmitFacts<'_>) -> FunctionLayout {
        facts.function_layout(
            self.params
                .iter()
                .map(|param| param.variadic.as_ref().unwrap_or(&param.layout).clone())
                .collect(),
            &self.return_layout,
            self.result.clone(),
            self.result_declaration.as_ref(),
        )
    }
}
