use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::borrow::Borrow;
use std::cell::OnceCell;

use ecow::EcoString;

/// Dot-qualified identifier for a named type, method, value, or variant.
///
/// Wraps the qualified name (`"main.Point.sum"`, `"prelude.Option"`,
/// `"go:net/http.Handler"`) as a single `EcoString` and exposes structured
/// accessors. Centralizes the join/split logic that used to live in ad-hoc
/// `format!("{}.{}", ..)` and `split_once('.')` call sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Symbol(EcoString);

impl Symbol {
    /// Joins a module id and a local (possibly multi-segment) name.
    ///
    /// `Symbol::from_parts("main", "Point.sum")` → `"main.Point.sum"`.
    pub fn from_parts(module: &str, local: &str) -> Self {
        let mut s = String::with_capacity(module.len() + 1 + local.len());
        s.push_str(module);
        s.push('.');
        s.push_str(local);
        Self(EcoString::from(s))
    }

    /// Appends an additional dot-segment to an already-qualified symbol.
    ///
    /// `Symbol::from_raw("main.Shape").with_segment("Circle")` →
    /// `"main.Shape.Circle"`.
    pub fn with_segment(&self, segment: &str) -> Self {
        let mut s = String::with_capacity(self.0.len() + 1 + segment.len());
        s.push_str(&self.0);
        s.push('.');
        s.push_str(segment);
        Self(EcoString::from(s))
    }

    /// Wraps an already-constructed qualified string. Prefer `from_parts`
    /// when the module id and local name are available separately.
    pub fn from_raw(qualified: impl Into<EcoString>) -> Self {
        Self(qualified.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_eco(&self) -> &EcoString {
        &self.0
    }

    /// Last dot-separated segment. `"main.Point.sum"` → `"sum"`.
    pub fn last_segment(&self) -> &str {
        self.0.rsplit('.').next().unwrap_or(&self.0)
    }

    /// Strips the last dot-separated segment. `"main.Point.sum"` → `"main.Point"`.
    /// Returns `None` if the symbol has no dot.
    pub fn without_last_segment(&self) -> Option<&str> {
        self.0.rsplit_once('.').map(|(rest, _)| rest)
    }

    /// Naive first segment (first dot split). Correct for user modules; for
    /// `go:net/http.Handler`-style symbols, resolve via
    /// `Store::module_for_qualified_name` instead.
    pub fn simple_module_part(&self) -> Option<&str> {
        self.0.split_once('.').map(|(m, _)| m)
    }
}

impl Borrow<str> for Symbol {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Symbol {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<&Symbol> for EcoString {
    fn from(s: &Symbol) -> Self {
        s.0.clone()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<EcoString> for Symbol {
    fn from(s: EcoString) -> Self {
        Self(s)
    }
}

impl From<Symbol> for EcoString {
    fn from(s: Symbol) -> Self {
        s.0
    }
}

impl From<&str> for Symbol {
    fn from(s: &str) -> Self {
        Self(EcoString::from(s))
    }
}

impl From<String> for Symbol {
    fn from(s: String) -> Self {
        Self(EcoString::from(s))
    }
}

impl PartialEq<str> for Symbol {
    fn eq(&self, other: &str) -> bool {
        self.0.as_str() == other
    }
}

impl PartialEq<&str> for Symbol {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_str() == *other
    }
}

/// Extract the unqualified name from a dot-qualified identifier.
///
/// `"prelude.Option"` → `"Option"`, `"**nominal.int"` → `"int"`, `"foo"` → `"foo"`
pub fn unqualified_name(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// type param name -> type variable
pub type SubstitutionMap = HashMap<EcoString, Type>;

pub fn substitute(ty: &Type, map: &HashMap<EcoString, Type>) -> Type {
    if map.is_empty() {
        return ty.clone();
    }
    match ty {
        Type::Parameter(name) => map.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Nominal {
            id,
            params,
            underlying_ty: underlying,
        } => Type::Nominal {
            id: id.clone(),
            params: params.iter().map(|p| substitute(p, map)).collect(),
            underlying_ty: underlying.as_ref().map(|u| Box::new(substitute(u, map))),
        },
        Type::Function {
            params,
            param_mutability,
            bounds,
            return_type,
        } => Type::Function {
            params: params.iter().map(|p| substitute(p, map)).collect(),
            param_mutability: param_mutability.clone(),
            bounds: bounds
                .iter()
                .map(|b| Bound {
                    param_name: b.param_name.clone(),
                    generic: substitute(&b.generic, map),
                    ty: substitute(&b.ty, map),
                })
                .collect(),
            return_type: Box::new(substitute(return_type, map)),
        },
        Type::Var { .. } | Type::Error => ty.clone(),
        Type::Forall { vars, body } => {
            let has_overlap = map.keys().any(|k| vars.contains(k));
            let substituted_body = if has_overlap {
                let filtered_map: HashMap<EcoString, Type> = map
                    .iter()
                    .filter(|(k, _)| !vars.contains(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                substitute(body, &filtered_map)
            } else {
                substitute(body, map)
            };
            Type::Forall {
                vars: vars.clone(),
                body: Box::new(substituted_body),
            }
        }
        Type::Tuple(elements) => Type::Tuple(elements.iter().map(|e| substitute(e, map)).collect()),
        Type::Compound { kind, args } => Type::Compound {
            kind: *kind,
            args: args.iter().map(|a| substitute(a, map)).collect(),
        },
        Type::Simple(_) | Type::Never | Type::ImportNamespace(_) | Type::ReceiverPlaceholder => {
            ty.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bound {
    pub param_name: EcoString,
    pub generic: Type,
    pub ty: Type,
}

/// A unique handle identifying a type variable. The binding state (Unbound /
/// Bound-to-a-Type) lives in a `TypeEnv` owned by the checker; the handle is
/// a plain id so `Type` stays a pure value (Clone, Eq, Hash, Serialize).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TypeVarId(pub u32);

impl TypeVarId {
    pub const IGNORED: TypeVarId = TypeVarId(u32::MAX);
    pub const UNINFERRED: TypeVarId = TypeVarId(u32::MAX - 1);

    pub fn is_reserved(self) -> bool {
        self == Self::IGNORED || self == Self::UNINFERRED
    }

    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Debug for TypeVarId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::IGNORED => write!(f, "ignored"),
            Self::UNINFERRED => write!(f, "uninferred"),
            TypeVarId(n) => write!(f, "#{}", n),
        }
    }
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Type {
    Simple(SimpleKind),

    Compound {
        kind: CompoundKind,
        args: Vec<Type>,
    },

    Nominal {
        id: Symbol,
        params: Vec<Type>,
        underlying_ty: Option<Box<Type>>,
    },

    /// Module namespace handle. Produced by imports (e.g. `import http "net/http"`
    /// produces an `ImportNamespace("go:net/http")` on the local identifier).
    /// Dot-access on this type resolves to the module's exports.
    ImportNamespace(EcoString),

    Function {
        params: Vec<Type>,
        param_mutability: Vec<bool>,
        bounds: Vec<Bound>,
        return_type: Box<Type>,
    },

    /// Type variable handle. Binding state lives in a `TypeEnv` owned by the
    /// checker; the inline `hint` is display metadata set at allocation time
    /// so `Display`/`Debug` work without env access.
    Var {
        id: TypeVarId,
        hint: Option<EcoString>,
    },

    Forall {
        vars: Vec<EcoString>,
        body: Box<Type>,
    },

    Parameter(EcoString),

    Never,

    Tuple(Vec<Type>),

    /// Poison type returned after an error has been reported.
    /// Unifies with everything silently, preventing cascading diagnostics.
    Error,

    /// Sentinel occupying the `self` slot of an interface method type.
    /// Unifies silently so an implementing type's receiver does not conflict
    /// with the abstract method shape. Previously encoded as
    /// `Constructor { id: "**nominal.__receiver__" }`.
    ReceiverPlaceholder,
}

impl std::fmt::Debug for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Nominal { id, params, .. } => f
                .debug_struct("Nominal")
                .field("id", id)
                .field("params", params)
                .finish(),
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                let mut s = f.debug_struct("Function");
                s.field("params", params);
                if param_mutability.iter().any(|m| *m) {
                    s.field("param_mutability", param_mutability);
                }
                s.field("bounds", bounds)
                    .field("return_type", return_type)
                    .finish()
            }
            Type::Var { id, hint } => {
                let mut s = f.debug_struct("Var");
                s.field("id", id);
                if let Some(h) = hint {
                    s.field("hint", h);
                }
                s.finish()
            }
            Type::Forall { vars, body } => f
                .debug_struct("Forall")
                .field("vars", vars)
                .field("body", body)
                .finish(),
            Type::Parameter(name) => f.debug_tuple("Parameter").field(name).finish(),
            Type::Never => write!(f, "Never"),
            Type::Tuple(elements) => f.debug_tuple("Tuple").field(elements).finish(),
            Type::Error => write!(f, "Error"),
            Type::ImportNamespace(module_id) => {
                f.debug_tuple("ImportNamespace").field(module_id).finish()
            }
            Type::ReceiverPlaceholder => write!(f, "ReceiverPlaceholder"),
            Type::Simple(kind) => f.debug_tuple("Simple").field(kind).finish(),
            Type::Compound { kind, args } => f
                .debug_struct("Compound")
                .field("kind", kind)
                .field("args", args)
                .finish(),
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Type::Nominal {
                    id: id1,
                    params: params1,
                    ..
                },
                Type::Nominal {
                    id: id2,
                    params: params2,
                    ..
                },
            ) => id1 == id2 && params1 == params2,
            (
                Type::Function {
                    params: p1,
                    param_mutability: m1,
                    bounds: b1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    param_mutability: m2,
                    bounds: b2,
                    return_type: r2,
                },
            ) => p1 == p2 && m1 == m2 && b1 == b2 && r1 == r2,
            (Type::Var { id: id1, .. }, Type::Var { id: id2, .. }) => id1 == id2,
            (
                Type::Forall {
                    vars: vars1,
                    body: body1,
                },
                Type::Forall {
                    vars: vars2,
                    body: body2,
                },
            ) => vars1 == vars2 && body1 == body2,
            (Type::Parameter(name1), Type::Parameter(name2)) => name1 == name2,
            (Type::Never, Type::Never) => true,
            (Type::Tuple(elems1), Type::Tuple(elems2)) => elems1 == elems2,
            (Type::ImportNamespace(m1), Type::ImportNamespace(m2)) => m1 == m2,
            (Type::ReceiverPlaceholder, Type::ReceiverPlaceholder) => true,
            (Type::Simple(k1), Type::Simple(k2)) => k1 == k2,
            (Type::Compound { kind: k1, args: a1 }, Type::Compound { kind: k2, args: a2 }) => {
                k1 == k2 && a1 == a2
            }
            _ => false,
        }
    }
}

thread_local! {
    static INTERNED_INT: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_STRING: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_BOOL: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_UNIT: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_FLOAT64: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_RUNE: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_BYTE: OnceCell<Type> = const { OnceCell::new() };
}

impl Type {
    pub fn simple(kind: SimpleKind) -> Type {
        Self::Simple(kind)
    }

    pub fn compound(kind: CompoundKind, args: Vec<Type>) -> Type {
        Self::Compound { kind, args }
    }

    pub fn int() -> Type {
        INTERNED_INT.with(|cell| cell.get_or_init(|| Self::simple(SimpleKind::Int)).clone())
    }

    pub fn string() -> Type {
        INTERNED_STRING.with(|cell| {
            cell.get_or_init(|| Self::simple(SimpleKind::String))
                .clone()
        })
    }

    pub fn bool() -> Type {
        INTERNED_BOOL.with(|cell| cell.get_or_init(|| Self::simple(SimpleKind::Bool)).clone())
    }

    pub fn unit() -> Type {
        INTERNED_UNIT.with(|cell| cell.get_or_init(|| Self::simple(SimpleKind::Unit)).clone())
    }

    pub fn float64() -> Type {
        INTERNED_FLOAT64.with(|cell| {
            cell.get_or_init(|| Self::simple(SimpleKind::Float64))
                .clone()
        })
    }

    pub fn rune() -> Type {
        INTERNED_RUNE.with(|cell| cell.get_or_init(|| Self::simple(SimpleKind::Rune)).clone())
    }

    pub fn byte() -> Type {
        INTERNED_BYTE.with(|cell| cell.get_or_init(|| Self::simple(SimpleKind::Byte)).clone())
    }
}

impl Type {
    pub fn uninferred() -> Self {
        Self::Var {
            id: TypeVarId::UNINFERRED,
            hint: None,
        }
    }

    pub fn ignored() -> Self {
        Self::Var {
            id: TypeVarId::IGNORED,
            hint: None,
        }
    }

    pub fn get_type_params(&self) -> Option<&[Type]> {
        match self {
            Type::Nominal { params, .. } => Some(params),
            Type::Compound { args, .. } => Some(args),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericFamily {
    SignedInt,
    UnsignedInt,
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CompoundKind {
    Ref,
    Slice,
    EnumeratedSlice,
    Map,
    Channel,
    Sender,
    Receiver,
    VarArgs,
}

impl CompoundKind {
    pub fn leaf_name(self) -> &'static str {
        match self {
            CompoundKind::Ref => "Ref",
            CompoundKind::Slice => "Slice",
            CompoundKind::EnumeratedSlice => "EnumeratedSlice",
            CompoundKind::Map => "Map",
            CompoundKind::Channel => "Channel",
            CompoundKind::Sender => "Sender",
            CompoundKind::Receiver => "Receiver",
            CompoundKind::VarArgs => "VarArgs",
        }
    }

    pub fn from_name(name: &str) -> Option<CompoundKind> {
        Some(match name {
            "Ref" => CompoundKind::Ref,
            "Slice" => CompoundKind::Slice,
            "EnumeratedSlice" => CompoundKind::EnumeratedSlice,
            "Map" => CompoundKind::Map,
            "Channel" => CompoundKind::Channel,
            "Sender" => CompoundKind::Sender,
            "Receiver" => CompoundKind::Receiver,
            "VarArgs" => CompoundKind::VarArgs,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SimpleKind {
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uintptr,
    Byte,
    Float32,
    Float64,
    Complex64,
    Complex128,
    Rune,
    Bool,
    String,
    Unit,
}

impl SimpleKind {
    pub fn leaf_name(self) -> &'static str {
        match self {
            SimpleKind::Int => "int",
            SimpleKind::Int8 => "int8",
            SimpleKind::Int16 => "int16",
            SimpleKind::Int32 => "int32",
            SimpleKind::Int64 => "int64",
            SimpleKind::Uint => "uint",
            SimpleKind::Uint8 => "uint8",
            SimpleKind::Uint16 => "uint16",
            SimpleKind::Uint32 => "uint32",
            SimpleKind::Uint64 => "uint64",
            SimpleKind::Uintptr => "uintptr",
            SimpleKind::Byte => "byte",
            SimpleKind::Float32 => "float32",
            SimpleKind::Float64 => "float64",
            SimpleKind::Complex64 => "complex64",
            SimpleKind::Complex128 => "complex128",
            SimpleKind::Rune => "rune",
            SimpleKind::Bool => "bool",
            SimpleKind::String => "string",
            SimpleKind::Unit => "Unit",
        }
    }

    pub fn from_name(name: &str) -> Option<SimpleKind> {
        Some(match name {
            "int" => SimpleKind::Int,
            "int8" => SimpleKind::Int8,
            "int16" => SimpleKind::Int16,
            "int32" => SimpleKind::Int32,
            "int64" => SimpleKind::Int64,
            "uint" => SimpleKind::Uint,
            "uint8" => SimpleKind::Uint8,
            "uint16" => SimpleKind::Uint16,
            "uint32" => SimpleKind::Uint32,
            "uint64" => SimpleKind::Uint64,
            "uintptr" => SimpleKind::Uintptr,
            "byte" => SimpleKind::Byte,
            "float32" => SimpleKind::Float32,
            "float64" => SimpleKind::Float64,
            "complex64" => SimpleKind::Complex64,
            "complex128" => SimpleKind::Complex128,
            "rune" => SimpleKind::Rune,
            "bool" => SimpleKind::Bool,
            "string" => SimpleKind::String,
            "Unit" => SimpleKind::Unit,
            _ => return None,
        })
    }

    pub fn is_arithmetic(self) -> bool {
        !matches!(
            self,
            SimpleKind::Bool | SimpleKind::String | SimpleKind::Unit | SimpleKind::Uintptr
        )
    }

    pub fn is_ordered(self) -> bool {
        self.is_arithmetic() && !matches!(self, SimpleKind::Complex64 | SimpleKind::Complex128)
    }

    pub fn is_unsigned_int(self) -> bool {
        matches!(
            self,
            SimpleKind::Byte
                | SimpleKind::Uint
                | SimpleKind::Uint8
                | SimpleKind::Uint16
                | SimpleKind::Uint32
                | SimpleKind::Uint64
        )
    }

    pub fn is_signed_int(self) -> bool {
        matches!(
            self,
            SimpleKind::Int
                | SimpleKind::Int8
                | SimpleKind::Int16
                | SimpleKind::Int32
                | SimpleKind::Int64
                | SimpleKind::Rune
        )
    }

    pub fn is_float(self) -> bool {
        matches!(self, SimpleKind::Float32 | SimpleKind::Float64)
    }

    pub fn is_complex(self) -> bool {
        matches!(self, SimpleKind::Complex64 | SimpleKind::Complex128)
    }

    pub fn numeric_family(self) -> Option<NumericFamily> {
        if self.is_signed_int() {
            Some(NumericFamily::SignedInt)
        } else if self.is_unsigned_int() {
            Some(NumericFamily::UnsignedInt)
        } else if self.is_float() {
            Some(NumericFamily::Float)
        } else {
            None
        }
    }
}

impl Type {
    pub fn get_function_ret(&self) -> Option<&Type> {
        match self {
            Type::Function { return_type, .. } => Some(return_type),
            _ => None,
        }
    }

    pub fn has_name(&self, name: &str) -> bool {
        match self {
            Type::Nominal { id, .. } => id.last_segment() == name,
            Type::Simple(kind) => kind.leaf_name() == name,
            Type::Compound { kind, .. } => kind.leaf_name() == name,
            _ => false,
        }
    }

    pub fn get_qualified_id(&self) -> Option<&str> {
        match self {
            Type::Nominal { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }

    pub fn get_underlying(&self) -> Option<&Type> {
        match self {
            Type::Nominal {
                underlying_ty: underlying,
                ..
            } => underlying.as_deref(),
            _ => None,
        }
    }

    pub fn is_result(&self) -> bool {
        self.has_qualified_id("prelude.Result")
    }

    pub fn is_option(&self) -> bool {
        self.has_qualified_id("prelude.Option")
    }

    pub fn is_partial(&self) -> bool {
        self.has_qualified_id("prelude.Partial")
    }

    fn has_qualified_id(&self, qualified_id: &str) -> bool {
        matches!(self, Type::Nominal { id, .. } if id.as_str() == qualified_id)
    }

    pub fn is_unit(&self) -> bool {
        self.is_simple(SimpleKind::Unit)
    }

    pub fn tuple_arity(&self) -> Option<usize> {
        match self {
            Type::Tuple(elements) => Some(elements.len()),
            _ => None,
        }
    }

    pub fn is_tuple(&self) -> bool {
        matches!(self, Type::Tuple(_))
    }

    pub fn as_import_namespace(&self) -> Option<&str> {
        match self {
            Type::ImportNamespace(module_id) => Some(module_id),
            _ => None,
        }
    }

    pub fn as_compound(&self) -> Option<(CompoundKind, &[Type])> {
        match self {
            Type::Compound { kind, args } => Some((*kind, args.as_slice())),
            Type::Nominal { id, params, .. } => {
                CompoundKind::from_name(id.last_segment()).map(|k| (k, params.as_slice()))
            }
            _ => None,
        }
    }

    pub fn is_native(&self, kind: CompoundKind) -> bool {
        self.as_compound().is_some_and(|(k, _)| k == kind)
    }

    pub fn is_ref(&self) -> bool {
        self.is_native(CompoundKind::Ref)
    }

    pub fn is_slice(&self) -> bool {
        self.is_native(CompoundKind::Slice)
    }

    pub fn is_map(&self) -> bool {
        self.is_native(CompoundKind::Map)
    }

    pub fn is_channel(&self) -> bool {
        self.is_native(CompoundKind::Channel)
    }

    pub fn is_receiver_placeholder(&self) -> bool {
        matches!(self, Type::ReceiverPlaceholder)
    }

    pub fn is_unknown(&self) -> bool {
        self.has_name("Unknown")
    }

    pub fn is_receiver(&self) -> bool {
        self.is_native(CompoundKind::Receiver)
    }

    pub fn is_ignored(&self) -> bool {
        matches!(self, Type::Var { id, .. } if *id == TypeVarId::IGNORED)
    }

    pub fn is_variadic(&self) -> Option<Type> {
        let last = self.get_function_params()?.last()?;
        match last.as_compound()? {
            (CompoundKind::VarArgs, _) => last.inner(),
            _ => None,
        }
    }

    pub fn is_string(&self) -> bool {
        self.is_simple(SimpleKind::String)
    }

    pub fn is_slice_of_simple(&self, element: SimpleKind) -> bool {
        match self.as_compound() {
            Some((CompoundKind::Slice, [elem])) => elem.is_simple(element),
            _ => false,
        }
    }

    pub fn is_slice_of(&self, element_name: &str) -> bool {
        match self.as_compound() {
            Some((CompoundKind::Slice, [elem])) => elem.has_name(element_name),
            _ => false,
        }
    }

    pub fn is_byte_slice(&self) -> bool {
        self.is_slice_of_simple(SimpleKind::Byte) || self.is_slice_of_simple(SimpleKind::Uint8)
    }

    pub fn is_rune_slice(&self) -> bool {
        self.is_slice_of_simple(SimpleKind::Rune)
    }

    pub fn is_byte_or_rune_slice(&self) -> bool {
        self.is_byte_slice() || self.is_rune_slice()
    }

    pub fn has_byte_or_rune_slice_underlying(&self) -> bool {
        if self.is_byte_or_rune_slice() {
            return true;
        }
        match self {
            Type::Nominal { underlying_ty, .. } => underlying_ty
                .as_deref()
                .is_some_and(|u| u.has_byte_or_rune_slice_underlying()),
            _ => false,
        }
    }

    pub fn as_simple(&self) -> Option<SimpleKind> {
        match self {
            Type::Simple(kind) => Some(*kind),
            Type::Nominal { id, .. } => SimpleKind::from_name(id.last_segment()),
            _ => None,
        }
    }

    pub fn is_simple(&self, kind: SimpleKind) -> bool {
        self.as_simple() == Some(kind)
    }

    pub fn is_boolean(&self) -> bool {
        self.is_simple(SimpleKind::Bool)
    }

    pub fn is_rune(&self) -> bool {
        self.is_simple(SimpleKind::Rune)
    }

    pub fn is_float64(&self) -> bool {
        self.is_simple(SimpleKind::Float64)
    }

    pub fn is_float32(&self) -> bool {
        self.is_simple(SimpleKind::Float32)
    }

    pub fn is_float(&self) -> bool {
        self.as_simple().is_some_and(SimpleKind::is_float)
    }

    pub fn is_variable(&self) -> bool {
        matches!(self, Type::Var { .. })
    }

    pub fn is_type_var(&self) -> bool {
        matches!(self, Type::Var { .. })
    }

    pub fn is_numeric(&self) -> bool {
        self.as_simple().is_some_and(SimpleKind::is_arithmetic)
    }

    pub fn is_ordered(&self) -> bool {
        self.as_simple().is_some_and(SimpleKind::is_ordered)
    }

    /// True for Go's `cmp.Ordered` set: ints, floats, strings, and named aliases over them.
    pub fn satisfies_ordered_constraint(&self) -> bool {
        if let Some(kind) = self.as_simple() {
            return matches!(
                kind,
                SimpleKind::Int
                    | SimpleKind::Int8
                    | SimpleKind::Int16
                    | SimpleKind::Int32
                    | SimpleKind::Int64
                    | SimpleKind::Uint
                    | SimpleKind::Uint8
                    | SimpleKind::Uint16
                    | SimpleKind::Uint32
                    | SimpleKind::Uint64
                    | SimpleKind::Uintptr
                    | SimpleKind::Byte
                    | SimpleKind::Rune
                    | SimpleKind::Float32
                    | SimpleKind::Float64
                    | SimpleKind::String
            );
        }
        match self {
            Type::Nominal { underlying_ty, .. } => underlying_ty
                .as_deref()
                .is_some_and(Type::satisfies_ordered_constraint),
            Type::Parameter(_) => true,
            _ => false,
        }
    }

    pub fn is_complex(&self) -> bool {
        self.as_simple().is_some_and(SimpleKind::is_complex)
    }

    pub fn is_unsigned_int(&self) -> bool {
        self.as_simple().is_some_and(SimpleKind::is_unsigned_int)
    }

    pub fn is_never(&self) -> bool {
        matches!(self, Type::Never)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    pub fn has_unbound_variables(&self) -> bool {
        match self {
            Type::Var { hint, .. } => hint.is_some(),
            Type::Nominal { params, .. } => params.iter().any(|p| p.has_unbound_variables()),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.has_unbound_variables())
                    || return_type.has_unbound_variables()
            }
            Type::Forall { body, .. } => body.has_unbound_variables(),
            Type::Tuple(elements) => elements.iter().any(|e| e.has_unbound_variables()),
            Type::Compound { args, .. } => args.iter().any(|a| a.has_unbound_variables()),
            Type::Simple(_)
            | Type::Parameter(_)
            | Type::Never
            | Type::Error
            | Type::ImportNamespace(_)
            | Type::ReceiverPlaceholder => false,
        }
    }

    pub fn remove_found_type_names(&self, names: &mut HashSet<EcoString>) {
        if names.is_empty() {
            return;
        }

        match self {
            Type::Nominal { id, params, .. } => {
                names.remove(id.last_segment());
                for param in params {
                    param.remove_found_type_names(names);
                }
            }
            Type::Function {
                params,
                return_type,
                bounds,
                ..
            } => {
                for param in params {
                    param.remove_found_type_names(names);
                }
                return_type.remove_found_type_names(names);
                for bound in bounds {
                    bound.generic.remove_found_type_names(names);
                    bound.ty.remove_found_type_names(names);
                }
            }
            Type::Forall { body, .. } => {
                body.remove_found_type_names(names);
            }
            Type::Var { .. } => {}
            Type::Parameter(name) => {
                names.remove(name);
            }
            Type::Tuple(elements) => {
                for element in elements {
                    element.remove_found_type_names(names);
                }
            }
            Type::Compound { kind, args } => {
                names.remove(kind.leaf_name());
                for arg in args {
                    arg.remove_found_type_names(names);
                }
            }
            Type::Simple(kind) => {
                names.remove(kind.leaf_name());
            }
            Type::Never | Type::Error | Type::ImportNamespace(_) | Type::ReceiverPlaceholder => {}
        }
    }
}

impl Type {
    pub fn get_name(&self) -> Option<&str> {
        match self {
            Type::Simple(kind) => Some(kind.leaf_name()),
            Type::Compound { kind, args } => match kind {
                CompoundKind::Ref => args.first().and_then(|inner| inner.get_name()),
                _ => Some(kind.leaf_name()),
            },
            Type::Nominal { id, params, .. } => {
                let name = id.last_segment();
                if CompoundKind::from_name(name) == Some(CompoundKind::Ref) {
                    return params.first().and_then(|inner| inner.get_name());
                }
                Some(name)
            }
            Type::ImportNamespace(module_id) => {
                let path = module_id.strip_prefix("go:").unwrap_or(module_id);
                path.rsplit('/').next()
            }
            _ => None,
        }
    }

    pub fn wraps(&self, name: &str, inner: &Type) -> bool {
        self.get_name().is_some_and(|n| n == name)
            && self
                .get_type_params()
                .and_then(|p| p.first())
                .is_some_and(|first| *first == *inner)
    }

    pub fn get_function_params(&self) -> Option<&[Type]> {
        match self {
            Type::Function { params, .. } => Some(params),
            Type::Nominal {
                underlying_ty: Some(inner),
                ..
            } => inner.get_function_params(),
            _ => None,
        }
    }

    pub fn param_count(&self) -> usize {
        match self {
            Type::Function { params, .. } => params.len(),
            _ => 0,
        }
    }

    pub fn get_param_mutability(&self) -> &[bool] {
        match self {
            Type::Function {
                param_mutability, ..
            } => param_mutability,
            _ => &[],
        }
    }

    pub fn with_replaced_first_param(&self, new_first: &Type) -> Type {
        match self {
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                if params.is_empty() {
                    return self.clone();
                }
                let mut new_params = params.clone();
                new_params[0] = new_first.clone();
                Type::Function {
                    params: new_params,
                    param_mutability: param_mutability.clone(),
                    bounds: bounds.clone(),
                    return_type: return_type.clone(),
                }
            }
            Type::Forall { vars, body } => Type::Forall {
                vars: vars.clone(),
                body: Box::new(body.with_replaced_first_param(new_first)),
            },
            _ => self.clone(),
        }
    }

    pub fn get_bounds(&self) -> &[Bound] {
        match self {
            Type::Function { bounds, .. } => bounds,
            Type::Forall { body, .. } => body.get_bounds(),
            _ => &[],
        }
    }

    pub fn get_qualified_name(&self) -> Symbol {
        match self.strip_refs() {
            Type::Nominal { id, .. } => id,
            Type::Simple(kind) => Symbol::from_parts("prelude", kind.leaf_name()),
            Type::Compound { kind, .. } => Symbol::from_parts("prelude", kind.leaf_name()),
            _ => panic!("called get_qualified_name on {:#?}", self),
        }
    }

    pub fn inner(&self) -> Option<Type> {
        self.get_type_params()
            .and_then(|args| args.first().cloned())
    }

    pub fn ok_type(&self) -> Type {
        debug_assert!(
            self.is_result() || self.is_option() || self.is_partial(),
            "ok_type called on non-Result/Option/Partial type"
        );
        self.inner()
            .expect("Result/Option/Partial should have inner type")
    }

    pub fn err_type(&self) -> Type {
        debug_assert!(
            self.is_result() || self.is_partial(),
            "err_type called on non-Result/Partial type"
        );
        self.get_type_params()
            .and_then(|args| args.get(1).cloned())
            .expect("Result/Partial should have error type")
    }
}

impl Type {
    pub fn unwrap_forall(&self) -> &Type {
        match self {
            Type::Forall { body, .. } => body.as_ref(),
            other => other,
        }
    }

    pub fn strip_refs(&self) -> Type {
        if self.is_ref() {
            return self.inner().expect("ref type must have inner").strip_refs();
        }

        self.clone()
    }

    pub fn with_receiver_placeholder(self) -> Type {
        match self {
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                let mut new_params = vec![Type::ReceiverPlaceholder];
                new_params.extend(params);

                let mut new_mutability = vec![false];
                new_mutability.extend(param_mutability);

                Type::Function {
                    params: new_params,
                    param_mutability: new_mutability,
                    bounds,
                    return_type,
                }
            }
            _ => unreachable!(
                "with_receiver_placeholder called on non-function type: {:?}",
                self
            ),
        }
    }

    pub fn remove_vars(types: &[&Type]) -> (Vec<Type>, Vec<EcoString>) {
        let mut vars = HashMap::default();
        let types = types
            .iter()
            .map(|v| Self::remove_vars_impl(v, &mut vars))
            .collect();

        (types, vars.into_values().collect())
    }

    fn remove_vars_impl(ty: &Type, vars: &mut HashMap<u32, EcoString>) -> Type {
        match ty {
            Type::Nominal {
                id: name,
                params: args,
                underlying_ty: underlying,
            } => Type::Nominal {
                id: name.clone(),
                params: args
                    .iter()
                    .map(|a| Self::remove_vars_impl(a, vars))
                    .collect(),
                underlying_ty: underlying
                    .as_ref()
                    .map(|u| Box::new(Self::remove_vars_impl(u, vars))),
            },

            Type::Function {
                params: args,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: args
                    .iter()
                    .map(|a| Self::remove_vars_impl(a, vars))
                    .collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds
                    .iter()
                    .map(|b| Bound {
                        param_name: b.param_name.clone(),
                        generic: Self::remove_vars_impl(&b.generic, vars),
                        ty: Self::remove_vars_impl(&b.ty, vars),
                    })
                    .collect(),
                return_type: Self::remove_vars_impl(return_type, vars).into(),
            },

            Type::Var { id, hint } => match vars.get(&id.0) {
                Some(g) => Type::Parameter(g.clone()),
                None => {
                    let name: EcoString = hint.clone().unwrap_or_else(|| {
                        char::from_digit(
                            (vars.len() + 10)
                                .try_into()
                                .expect("type var count fits in u32"),
                            16,
                        )
                        .expect("type var index is valid hex digit")
                        .to_uppercase()
                        .to_string()
                        .into()
                    });

                    vars.insert(id.0, name.clone());
                    Type::Parameter(name)
                }
            },

            Type::Forall { body, .. } => Self::remove_vars_impl(body, vars),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|e| Self::remove_vars_impl(e, vars))
                    .collect(),
            ),
            Type::Compound { kind, args } => Type::Compound {
                kind: *kind,
                args: args
                    .iter()
                    .map(|a| Self::remove_vars_impl(a, vars))
                    .collect(),
            },
            Type::Simple(_) | Type::Parameter(_) => ty.clone(),
            Type::Never | Type::Error | Type::ImportNamespace(_) | Type::ReceiverPlaceholder => {
                ty.clone()
            }
        }
    }

    pub fn contains_type(&self, target: &Type) -> bool {
        if *self == *target {
            return true;
        }
        match self {
            Type::Nominal { params, .. } => params.iter().any(|p| p.contains_type(target)),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.contains_type(target)) || return_type.contains_type(target)
            }
            Type::Var { .. } => false,
            Type::Forall { body, .. } => body.contains_type(target),
            Type::Tuple(elements) => elements.iter().any(|e| e.contains_type(target)),
            Type::Compound { args, .. } => args.iter().any(|a| a.contains_type(target)),
            Type::Simple(_)
            | Type::Parameter(_)
            | Type::Never
            | Type::Error
            | Type::ImportNamespace(_)
            | Type::ReceiverPlaceholder => false,
        }
    }
}

impl Type {
    pub fn underlying_numeric_type(&self) -> Option<Type> {
        self.underlying_numeric_type_recursive(&mut HashSet::default())
    }

    pub fn has_underlying_numeric_type(&self) -> bool {
        self.underlying_numeric_type().is_some()
    }

    fn underlying_numeric_type_recursive(&self, visited: &mut HashSet<Symbol>) -> Option<Type> {
        match self {
            Type::Simple(_) if self.is_numeric() => Some(self.clone()),
            Type::Nominal {
                id,
                underlying_ty: underlying,
                ..
            } => {
                if self.is_numeric() {
                    return Some(self.clone());
                }

                if !visited.insert(id.clone()) {
                    return None;
                }

                underlying
                    .as_ref()?
                    .underlying_numeric_type_recursive(visited)
            }
            _ => None,
        }
    }

    pub fn numeric_family(&self) -> Option<NumericFamily> {
        self.as_simple()?.numeric_family()
    }

    pub fn is_numeric_compatible_with(&self, other: &Type) -> bool {
        let self_underlying_ty = self.underlying_numeric_type();
        let other_underlying_ty = other.underlying_numeric_type();

        match (self_underlying_ty, other_underlying_ty) {
            (Some(s), Some(o)) => s.numeric_family() == o.numeric_family(),
            _ => false,
        }
    }

    pub fn is_aliased_numeric_type(&self) -> bool {
        match self {
            Type::Nominal { underlying_ty, .. } => {
                underlying_ty.is_some() && !self.is_numeric() && self.has_underlying_numeric_type()
            }
            _ => false,
        }
    }
}
