mod definition;
mod emit_input;
mod file;
mod module;

pub use definition::{Definition, Interface, MethodSignatures, Visibility};
pub use emit_input::{
    CallKind, CoercionInfo, DotAccessKind, EmitInput, MutationInfo, NativeTypeKind,
    ReceiverCoercion, ResolutionInfo, UnusedInfo,
};
pub use file::{File, FileImport};
pub use module::{Module, ModuleId, ModuleInfo};
