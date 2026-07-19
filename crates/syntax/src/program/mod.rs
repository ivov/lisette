mod definition;
mod emit_input;
mod file;
mod module;
mod resolution;

pub use definition::{
    Attributes, Definition, DefinitionBody, Interface, MethodSignatures, TypeAttribute, Visibility,
};
pub use emit_input::{EmitInput, EqualityIndex, MutationInfo, TestFunction, TestIndex, UnusedInfo};
pub use file::{File, FileImport, go_import_default_name};
pub use module::{Module, ModuleId, ModuleInfo};
pub use resolution::{
    CallKind, ChannelOperation, DotAccessKind, NativeTypeKind, ReceiverCoercion,
    ResolvedDefinitions, channel_operation, resolved_definition,
};
