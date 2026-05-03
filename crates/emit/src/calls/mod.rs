pub(crate) mod dispatch;
pub(crate) mod go_interop;
mod native;
mod regular;
mod ufcs;

use crate::types::native::NativeGoType;
use syntax::ast::{Annotation, Expression};
use syntax::types::Type;

pub(super) struct NativeCallContext<'a> {
    pub function: &'a Expression,
    pub args: &'a [Expression],
    pub spread: Option<&'a Expression>,
    pub type_args: &'a [Annotation],
    pub call_ty: Option<&'a Type>,
    pub native_type: &'a NativeGoType,
    pub method: &'a str,
}
