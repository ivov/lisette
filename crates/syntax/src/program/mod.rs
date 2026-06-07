mod definition;
mod emit_input;
mod file;
mod module;
mod resolution;

pub use definition::{
    Attributes, Definition, DefinitionBody, Interface, MethodSignatures, TypeAttribute, Visibility,
};
pub use emit_input::{EmitInput, MutationInfo, UnusedInfo};
pub use file::{File, FileImport};
pub use module::{Module, ModuleId, ModuleInfo};
pub use resolution::{CallKind, DotAccessKind, NativeTypeKind, ReceiverCoercion};
