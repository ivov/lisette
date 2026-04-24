//! Resolution metadata attached to `Expression::Call` and
//! `Expression::DotAccess` during type checking. Inference populates these
//! so downstream consumers (the emitter in particular) do not re-derive the
//! classification from the typed AST.

use crate::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverCoercion {
    /// Insert `&` to convert `T` to `Ref<T>`
    AutoAddress,
    /// Insert `*` to convert `Ref<T>` to `T`
    AutoDeref,
}

/// What a dot access resolved to during type checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotAccessKind {
    /// Named struct field access
    StructField { is_exported: bool },
    /// Tuple struct field access (e.g., `point.0` on `struct Point(int, int)`).
    /// `is_newtype` is true when the struct has exactly 1 field and no generics,
    /// meaning access should emit a type cast rather than `.F0`.
    TupleStructField { is_newtype: bool },
    /// Tuple element access (e.g., `t.0`, `t.1`)
    TupleElement,
    /// Module member access (e.g., `mod.func`)
    ModuleMember,
    /// Value enum variant (Go constant, e.g., `reflect.String`)
    ValueEnumVariant,
    /// ADT enum variant constructor (e.g., `makeColorRed[T]()`)
    EnumVariant,
    /// Instance method (has `self` receiver)
    InstanceMethod { is_exported: bool },
    /// Instance method used as a first-class value (not called).
    /// E.g., `Point.area` used as a callback. The emitter needs to know
    /// whether the receiver is a pointer to emit Go method expression syntax.
    InstanceMethodValue {
        is_exported: bool,
        is_pointer_receiver: bool,
    },
    /// Static method (no `self` receiver)
    StaticMethod { is_exported: bool },
}

/// What kind of native built-in type (Slice, Map, Channel, etc.) a call targets.
/// Defined here so semantics can classify calls without depending on
/// emit-specific types. The emitter maps this to its internal `NativeGoType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTypeKind {
    Slice,
    EnumeratedSlice,
    Map,
    Channel,
    Sender,
    Receiver,
    String,
}

impl NativeTypeKind {
    pub fn from_type(ty: &Type) -> Option<Self> {
        let resolved = ty.strip_refs();
        // Skip module namespaces and Go-imported types: their leaf name can
        // collide with a native type (e.g. `Slice`), but they are not native.
        if resolved.as_import_namespace().is_some() {
            return None;
        }
        if let Type::Nominal { ref id, .. } = resolved
            && id.as_str().starts_with("go:")
        {
            return None;
        }
        let name = resolved.get_name()?;
        Self::from_name(name)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Slice" => Some(Self::Slice),
            "EnumeratedSlice" => Some(Self::EnumeratedSlice),
            "Map" => Some(Self::Map),
            "Channel" => Some(Self::Channel),
            "Sender" => Some(Self::Sender),
            "Receiver" => Some(Self::Receiver),
            "string" => Some(Self::String),
            _ => None,
        }
    }
}

/// What a call expression resolved to during type checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// Regular function or method call
    Regular,
    /// Tuple struct constructor (e.g., `Point(1, 2)`)
    TupleStructConstructor,
    /// Type assertion (`assert_type`)
    AssertType,
    /// UFCS method call: `receiver.method()` where method is a free function
    UfcsMethod,
    /// Native type constructor (e.g., `Channel.new`, `Map.new`, `Slice.new`)
    NativeConstructor(NativeTypeKind),
    /// Native type instance method via dot access (e.g., `slice.append(x)`)
    NativeMethod(NativeTypeKind),
    /// Native type method via identifier (e.g., `Slice.contains(s, x)`)
    NativeMethodIdentifier(NativeTypeKind),
    /// Receiver method in UFCS syntax: `Type.method(receiver, args)`
    ReceiverMethodUfcs { is_public: bool },
}
