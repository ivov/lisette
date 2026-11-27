mod definition;
mod emit_input;
mod file;
mod package;
mod resolution;

pub use crate::ast::Visibility;
pub use definition::{
    AliasKind, Attributes, ConstantValue, ConstructorField, ConstructorFields, Definition,
    DefinitionBody, Interface, InterfaceInstance, InterfaceRequirement, Method, MethodOrigin,
    Methods, TypeAttribute, ValueKind, constructor_fields, interface_declares_any_method,
    interface_instances, interface_requirements, method_for_type, methods_for_type,
    type_has_any_method,
};
pub use emit_input::{
    BindingMutation, EmitBindings, EmitInput, EqualityIndex, ImportKey, TestFunction, TestIndex,
    UnusedImports,
};
pub use file::{
    File, FileImport, SourceFileKind, go_filename, go_import_default_name, is_test_file,
    unaliased_binding_name,
};
pub use package::{
    GO_IMPORT_PREFIX, PRELUDE_PACKAGE_ID, PRELUDE_PREFIX, Package, PackageId, UninferredExports,
    is_go_import, is_internal_package_id, is_prelude_symbol, strip_go_prefix, strip_prelude_prefix,
};
pub use resolution::{
    CalleeName, ChannelOperation, DotAccessResolution, MethodName, NativeTypeKind,
    ReceiverCoercion, channel_operation, prelude_constructor, resolved_callee,
    resolved_definition_name, resolved_instantiation,
};

mod native_operation;
pub use native_operation::{
    NativeInlineForm, NativeInlinePackage, NativeInlineRule, NativeOperation,
};

mod go_interop;
pub use go_interop::{
    GoHint, GoInteropContract, GoOptionRepresentation, GoReturnContract, GoTypeRecipe, GoTypeTerm,
    GoTypeTerms, SlotOrigin, go_boundary_converts_container, go_option_representation,
    go_return_contract,
};

mod conformance;
pub use conformance::{CheckedMethod, ConformanceIndex, ConformanceWitness};

mod go_method_policy;
pub use go_method_policy::GoMethodPolicy;

mod checked;
pub use checked::{
    Adaptation, CallOperation, CheckedBodies, CheckedBodiesBuilder, CheckedCallee,
    CheckedExpression, CheckedStructLiteral, CheckedValueUse, ElementEquality, EqualityRecipe,
    FieldKey, FieldSelection, InterfaceConversion, OperandConversion, ValueUseKey,
};
