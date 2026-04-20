use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Type;

use super::{Definition, File, ModuleInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ReceiverId(Span);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverCoercion {
    /// Insert `&` to convert `T` to `Ref<T>`
    AutoAddress,
    /// Insert `*` to convert `Ref<T>` to `T`
    AutoDeref,
}

#[derive(Debug, Clone, Default)]
pub struct CoercionInfo {
    receivers: HashMap<ReceiverId, ReceiverCoercion>,
}

impl CoercionInfo {
    pub fn mark_coercion(&mut self, span: Span, coercion: ReceiverCoercion) {
        self.receivers.insert(ReceiverId(span), coercion);
    }

    pub fn get_coercion(&self, span: Span) -> Option<ReceiverCoercion> {
        self.receivers.get(&ReceiverId(span)).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BindingId(Span);

#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    bindings: HashSet<BindingId>,
    definitions: HashSet<BindingId>,
    pub imports_by_module: HashMap<EcoString, HashSet<EcoString>>,
}

impl UnusedInfo {
    pub fn mark_binding_unused(&mut self, span: Span) {
        self.bindings.insert(BindingId(span));
    }

    pub fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { span, .. } => self.bindings.contains(&BindingId(*span)),
            Pattern::AsBinding { span, name, .. } => {
                let name_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - name.len() as u32,
                    name.len() as u32,
                );
                self.bindings.contains(&BindingId(name_span))
            }
            _ => false,
        }
    }

    pub fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        match rest {
            RestPattern::Bind { span, .. } => self.bindings.contains(&BindingId(*span)),
            _ => false,
        }
    }

    pub fn mark_definition_unused(&mut self, span: Span) {
        self.definitions.insert(BindingId(span));
    }

    pub fn is_unused_definition(&self, span: &Span) -> bool {
        self.definitions.contains(&BindingId(*span))
    }
}

#[derive(Debug, Clone, Default)]
pub struct MutationInfo {
    bindings: HashSet<AstBindingId>,
}

impl MutationInfo {
    pub fn mark_binding_mutated(&mut self, id: AstBindingId) {
        self.bindings.insert(id);
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.bindings.contains(&id)
    }
}

/// What a dot access resolved to during type checking.
///
/// Pre-computed in semantics to avoid re-derivation in the emitter.
/// The emitter can use this to skip cascading `try_classify_*` lookups.
/// `is_exported` indicates whether the Go name should be capitalized.
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
///
/// Defined in `syntax` so that semantics can classify calls without depending on
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
        let resolved = ty.resolve().strip_refs();
        if let Type::Constructor { ref id, .. } = resolved
            && (id.starts_with("@import/go:") || id.starts_with("go:"))
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
///
/// Pre-computed in semantics to avoid re-derivation in the emitter's call dispatch.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ResolutionId(Span);

/// Pre-computed resolution metadata from type checking.
///
/// Follows the same pattern as `CoercionInfo`: keyed by expression span,
/// populated during inference, consumed by the emitter.
#[derive(Debug, Clone, Default)]
pub struct ResolutionInfo {
    dot_accesses: HashMap<ResolutionId, DotAccessKind>,
    calls: HashMap<ResolutionId, CallKind>,
}

impl ResolutionInfo {
    pub fn mark_dot_access(&mut self, span: Span, kind: DotAccessKind) {
        self.dot_accesses.insert(ResolutionId(span), kind);
    }

    pub fn get_dot_access(&self, span: Span) -> Option<DotAccessKind> {
        self.dot_accesses.get(&ResolutionId(span)).copied()
    }

    pub fn mark_call(&mut self, span: Span, meta: CallKind) {
        self.calls.insert(ResolutionId(span), meta);
    }

    pub fn get_call(&self, span: Span) -> Option<CallKind> {
        self.calls.get(&ResolutionId(span)).copied()
    }
}

pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<EcoString, Definition>,
    pub modules: HashMap<String, ModuleInfo>,
    pub entry_module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub coercions: CoercionInfo,
    pub resolutions: ResolutionInfo,
    pub cached_modules: HashSet<String>,
    pub ufcs_methods: HashSet<(String, String)>,
    pub go_package_names: HashMap<String, String>,
}
