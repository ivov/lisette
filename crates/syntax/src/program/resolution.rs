use std::fmt::{self, Display, Formatter};

use ecow::EcoString;

use super::{PRELUDE_PACKAGE_ID, is_go_import, is_prelude_symbol};
use crate::ast::Expression;
use crate::types::Symbol;
use crate::types::{CompoundKind, SimpleKind, SubstitutionMap, Type};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodName {
    pub owner: Symbol,
    pub name: EcoString,
}

impl MethodName {
    pub fn new(owner: Symbol, name: &str) -> Self {
        Self {
            owner,
            name: name.into(),
        }
    }

    pub fn is_go(&self) -> bool {
        is_go_import(&self.owner)
    }
}

impl Display for MethodName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CalleeName {
    Definition(Symbol),
    Method(MethodName),
}

impl CalleeName {
    pub fn definition(&self) -> Option<&Symbol> {
        match self {
            Self::Definition(name) => Some(name),
            Self::Method(_) => None,
        }
    }

    pub fn method(&self) -> Option<&MethodName> {
        match self {
            Self::Definition(_) => None,
            Self::Method(method) => Some(method),
        }
    }

    pub fn is_go(&self) -> bool {
        match self {
            Self::Definition(name) => is_go_import(name),
            Self::Method(method) => method.is_go(),
        }
    }

    pub fn is_prelude(&self) -> bool {
        match self {
            Self::Definition(name) => is_prelude_symbol(name),
            Self::Method(method) => is_prelude_symbol(&method.owner),
        }
    }

    pub fn qualified_name(&self) -> Symbol {
        match self {
            Self::Definition(name) => name.clone(),
            Self::Method(method) => method.owner.with_segment(&method.name),
        }
    }

    pub fn last_segment(&self) -> &str {
        match self {
            Self::Definition(name) => name.last_segment(),
            Self::Method(method) => &method.name,
        }
    }
}

impl Display for CalleeName {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Definition(name) => name.fmt(f),
            Self::Method(method) => method.fmt(f),
        }
    }
}

pub fn resolved_callee(expression: &Expression) -> Option<CalleeName> {
    match expression.unwrap_parens() {
        Expression::Identifier { resolution, .. } => resolution.callee_name(),
        Expression::DotAccess { resolution, .. } => resolution.callee_name(),
        _ => None,
    }
}

pub fn resolved_definition_name(expression: &Expression) -> Option<&str> {
    match expression.unwrap_parens() {
        Expression::Identifier { resolution, .. } => resolution.definition(),
        Expression::DotAccess { resolution, .. } => resolution.defined_name().map(Symbol::as_str),
        _ => None,
    }
}

/// The prelude enum and member an expression resolves to, as `("prelude.Option", "None")`.
pub fn prelude_constructor(expression: &Expression) -> Option<(&str, &str)> {
    let (owner, member) = resolved_definition_name(expression)?.rsplit_once('.')?;
    let owner = match (owner, member) {
        (PRELUDE_PACKAGE_ID, "Some" | "None") => "prelude.Option",
        (PRELUDE_PACKAGE_ID, "Ok" | "Err") => "prelude.Result",
        _ => owner,
    };
    Some((owner, member))
}

pub fn resolved_instantiation(expression: &Expression) -> Option<&SubstitutionMap> {
    match expression.unwrap_parens() {
        Expression::Identifier { resolution, .. } => resolution.instantiation(),
        Expression::DotAccess { resolution, .. } => resolution.instantiation(),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelOperation<'a> {
    Receive {
        channel: &'a Expression,
    },
    Send {
        channel: &'a Expression,
        value: &'a Expression,
    },
}

impl<'a> ChannelOperation<'a> {
    pub fn channel(self) -> &'a Expression {
        match self {
            Self::Receive { channel } | Self::Send { channel, .. } => channel,
        }
    }

    pub fn value(self) -> Option<&'a Expression> {
        match self {
            Self::Receive { .. } => None,
            Self::Send { value, .. } => Some(value),
        }
    }
}

pub fn channel_operation(expression: &Expression) -> Option<ChannelOperation<'_>> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return None;
    };

    let (channel, method, operands): (&Expression, &str, &[Expression]) = match callee.as_ref() {
        Expression::DotAccess {
            expression: channel,
            member,
            ..
        } => (channel, member, args),
        Expression::Identifier { value, .. } if has_channel_ufcs_prefix(value) => {
            let (channel, operands) = args.split_first()?;
            (channel, value.rsplit('.').next()?, operands)
        }
        _ => return None,
    };

    match (method, operands) {
        ("receive", []) => Some(ChannelOperation::Receive { channel }),
        ("send", [value]) => Some(ChannelOperation::Send { channel, value }),
        _ => None,
    }
}

fn has_channel_ufcs_prefix(identifier: &str) -> bool {
    let Some((prefix, _)) = identifier.rsplit_once('.') else {
        return false;
    };
    matches!(
        prefix.rsplit('.').next(),
        Some("Channel" | "Sender" | "Receiver")
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverCoercion {
    /// Insert `&` to convert `T` to `Ref<T>`
    AutoAddress,
    /// Insert `*` to convert `Ref<T>` to `T`
    AutoDeref,
}

/// What a dot access resolved to during type checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DotAccessResolution {
    Unresolved,
    /// Named struct field access
    StructField {
        is_exported: bool,
        declaring_type: Option<Symbol>,
    },
    /// Tuple struct field access (e.g., `point.0` on `struct Point(int, int)`).
    /// `is_newtype` is true when the struct has exactly 1 field and no generics,
    /// meaning access should emit a type cast rather than `.F0`.
    TupleStructField {
        index: usize,
        is_newtype: bool,
    },
    /// Tuple element access (e.g., `t.0`, `t.1`)
    TupleElement {
        index: usize,
    },
    /// Package member access (e.g., `mod.func`)
    PackageMember {
        definition: Option<Symbol>,
        instantiation: SubstitutionMap,
    },
    /// ADT enum variant constructor (e.g., `makeColorRed[T]()`)
    EnumVariant {
        definition: Symbol,
        instantiation: SubstitutionMap,
    },
    /// Instance method (has `self` receiver)
    InstanceMethod {
        is_exported: bool,
        receiver_coercion: Option<ReceiverCoercion>,
        method: Option<MethodName>,
        instantiation: SubstitutionMap,
    },
    /// Instance method used as a first-class value (not called).
    /// E.g., `Point.area` used as a callback.
    InstanceMethodValue {
        is_exported: bool,
        method: Option<MethodName>,
        instantiation: SubstitutionMap,
    },
    /// Static method (no `self` receiver)
    StaticMethod {
        is_exported: bool,
        definition: Symbol,
        instantiation: SubstitutionMap,
    },
}

impl DotAccessResolution {
    pub fn reads_receiver(&self) -> bool {
        matches!(
            self,
            Self::StructField { .. }
                | Self::TupleStructField { .. }
                | Self::TupleElement { .. }
                | Self::InstanceMethod { .. }
                | Self::InstanceMethodValue { .. }
        )
    }

    pub fn is_field_read(&self) -> bool {
        matches!(
            self,
            Self::StructField { .. } | Self::TupleStructField { .. } | Self::TupleElement { .. }
        )
    }

    pub fn declaring_type(&self) -> Option<&Symbol> {
        match self {
            Self::StructField { declaring_type, .. } => declaring_type.as_ref(),
            _ => None,
        }
    }

    pub fn receiver_coercion(&self) -> Option<ReceiverCoercion> {
        match self {
            Self::InstanceMethod {
                receiver_coercion, ..
            } => *receiver_coercion,
            _ => None,
        }
    }

    pub fn defined_name(&self) -> Option<&Symbol> {
        match self {
            Self::PackageMember { definition, .. } => definition.as_ref(),
            Self::EnumVariant { definition, .. } | Self::StaticMethod { definition, .. } => {
                Some(definition)
            }
            Self::Unresolved
            | Self::StructField { .. }
            | Self::TupleStructField { .. }
            | Self::TupleElement { .. }
            | Self::InstanceMethod { .. }
            | Self::InstanceMethodValue { .. } => None,
        }
    }

    pub fn method(&self) -> Option<&MethodName> {
        match self {
            Self::InstanceMethod { method, .. } | Self::InstanceMethodValue { method, .. } => {
                method.as_ref()
            }
            _ => None,
        }
    }

    pub fn callee_name(&self) -> Option<CalleeName> {
        match self.method() {
            Some(method) => Some(CalleeName::Method(method.clone())),
            None => self.defined_name().cloned().map(CalleeName::Definition),
        }
    }

    pub fn instantiation(&self) -> Option<&SubstitutionMap> {
        match self {
            Self::PackageMember { instantiation, .. }
            | Self::EnumVariant { instantiation, .. }
            | Self::InstanceMethod { instantiation, .. }
            | Self::InstanceMethodValue { instantiation, .. }
            | Self::StaticMethod { instantiation, .. } => Some(instantiation),
            Self::Unresolved
            | Self::StructField { .. }
            | Self::TupleStructField { .. }
            | Self::TupleElement { .. } => None,
        }
    }
}

/// What kind of native built-in type (Slice, Map, Channel, etc.) a call targets.
/// Defined here so semantics can classify calls without depending on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTypeKind {
    Slice,
    EnumeratedSlice,
    Map,
    Channel,
    Sender,
    Receiver,
    String,
    Array,
}

impl NativeTypeKind {
    pub fn from_type(ty: &Type) -> Option<Self> {
        Some(match ty.strip_refs() {
            Type::Compound { kind, .. } => match kind {
                CompoundKind::Slice => Self::Slice,
                CompoundKind::EnumeratedSlice => Self::EnumeratedSlice,
                CompoundKind::Map => Self::Map,
                CompoundKind::Channel => Self::Channel,
                CompoundKind::Sender => Self::Sender,
                CompoundKind::Receiver => Self::Receiver,
                CompoundKind::Ref | CompoundKind::VarArgs => return None,
            },
            Type::Simple(SimpleKind::String) => Self::String,
            Type::Array { .. } => Self::Array,
            _ => return None,
        })
    }

    pub fn from_constructor_path(path: &str) -> Option<Self> {
        match path {
            "Channel.new" | "Channel.buffered" => Some(Self::Channel),
            "Map.new" => Some(Self::Map),
            "Slice.new" | "Slice.make" => Some(Self::Slice),
            "Array.new" | "Array.from" => Some(Self::Array),
            _ => None,
        }
    }

    pub fn is_constructor_method(name: &str) -> bool {
        matches!(name, "new" | "buffered" | "make")
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
            "Array" => Some(Self::Array),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Slice => "Slice",
            Self::EnumeratedSlice => "EnumeratedSlice",
            Self::Map => "Map",
            Self::Channel => "Channel",
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::String => "string",
            Self::Array => "Array",
        }
    }
}
