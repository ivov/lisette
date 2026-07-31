mod definition;
mod emit_input;
mod file;
mod module;
mod resolution;

pub use definition::{
    AliasKind, Attributes, Definition, DefinitionBody, Interface, MethodSignatures, TypeAttribute,
    ValueKind, Visibility,
};
pub use emit_input::{
    BindingMutation, EmitInput, EqualityIndex, MutationInfo, TestFunction, TestIndex, UnusedInfo,
};
pub use file::{File, FileImport, go_import_default_name, unaliased_binding_name};
pub use module::{Module, ModuleId};
pub use resolution::{
    CallKind, ChannelOperation, DotAccessKind, DotAccessResolution, NativeTypeKind,
    ReceiverCoercion, channel_operation, resolved_definition,
};
