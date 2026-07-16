mod clone;
pub(crate) mod dispatch;
pub(crate) mod go_interop;
pub(crate) mod native;
mod regular;
mod ufcs;

use crate::plan::values::CaptureBoundary;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;
use syntax::types::Type;

pub(super) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub resolved_type_args: &'a [Type],
    pub call_ty: Option<&'a Type>,
    pub native_type: &'a NativeGoType,
    pub method: &'a str,
    pub capture_boundary: CaptureBoundary,
    pub retired_receiver: Option<&'a Expression>,
}
