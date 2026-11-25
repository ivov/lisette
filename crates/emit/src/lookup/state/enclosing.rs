use crate::target::ir::bodies::LoopId;
use crate::target::ir::values::GoExpression;
use crate::target::types::return_abi::LoweredReturnAbi;
use syntax::types::Type;

#[derive(Clone, Debug, Default)]
pub(crate) enum ReturnContext {
    #[default]
    None,
    Void(Type),
    Tagged(Type),
    Lowered {
        return_ty: Type,
        shape: LoweredReturnAbi,
    },
}

impl ReturnContext {
    pub(crate) fn ty(&self) -> Option<&Type> {
        match self {
            ReturnContext::None => None,
            ReturnContext::Void(ty)
            | ReturnContext::Tagged(ty)
            | ReturnContext::Lowered { return_ty: ty, .. } => Some(ty),
        }
    }

    pub(crate) fn lowered(&self) -> Option<(&Type, &LoweredReturnAbi)> {
        match self {
            ReturnContext::Lowered { return_ty, shape } => Some((return_ty, shape)),
            _ => None,
        }
    }

    pub(crate) fn lowered_shape(&self) -> Option<LoweredReturnAbi> {
        match self {
            ReturnContext::Lowered { shape, .. } => Some(shape.clone()),
            _ => None,
        }
    }

    pub(crate) fn expect_ty(&self) -> Type {
        self.ty()
            .cloned()
            .expect("lowered abi requires a return context")
    }
}

#[derive(Clone)]
pub(crate) struct LoopContext {
    pub(crate) id: LoopId,
    pub(crate) result: Option<GoExpression>,
    pub(crate) result_ty: Option<Type>,
}
