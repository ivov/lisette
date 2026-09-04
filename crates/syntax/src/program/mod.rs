mod definition;
mod emit_input;
mod file;
mod package;
mod resolution;

pub use crate::ast::Visibility;
pub use definition::{
    AliasKind, Attributes, ConstantValue, Definition, DefinitionBody, Interface, InterfaceInstance,
    InterfaceRequirement, Method, MethodOrigin, Methods, TypeAttribute, ValueKind,
    interface_declares_any_method, interface_instances, interface_requirements, method_for_type,
    methods_for_type, type_has_any_method,
};
pub use emit_input::{
    BindingMutation, EmitInput, EqualityIndex, MutationInfo, TestFunction, TestIndex, UnusedInfo,
};
pub use file::{File, FileImport, go_import_default_name, is_test_file, unaliased_binding_name};
pub use package::{Package, PackageId, UninferredExports, is_internal_package_id};
pub use resolution::{
    CallKind, ChannelOperation, DotAccessKind, DotAccessResolution, NativeTypeKind,
    ReceiverCoercion, channel_operation, resolved_definition,
};
