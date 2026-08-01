use ecow::EcoString;
use syntax::ast::Literal;
use syntax::lex::rune_codepoint;
use syntax::types::{SimpleKind, Type};

use crate::store::Store;

#[derive(Debug, Clone)]
pub struct ClosedMember {
    /// Qualified the way the user writes it (e.g. `time.Sunday`), for the diagnostic.
    display_name: EcoString,
    /// The member's source literal, for rendering the valid-set hint.
    literal: Literal,
}

impl ClosedMember {
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn literal(&self) -> &Literal {
        &self.literal
    }

    pub fn value(&self, base: SimpleKind) -> DomainValue {
        DomainValue::from_literal(&self.literal, base)
            .expect("closed-domain members have a literal compatible with their base")
    }
}

/// The curated valid-value set of a `#[go(closed_domain)]` named primitive.
#[derive(Debug, Clone)]
pub struct ClosedDomain {
    base: SimpleKind,
    type_display: EcoString,
    members: Vec<ClosedMember>,
}

impl ClosedDomain {
    pub fn base(&self) -> SimpleKind {
        self.base
    }

    pub fn type_display(&self) -> &str {
        &self.type_display
    }

    pub fn members(&self) -> &[ClosedMember] {
        &self.members
    }
}

/// A literal reduced to its comparable form for a closed domain's base kind.
/// Float bases are unsupported, so only integers (signed `i128`) and strings occur.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DomainValue {
    Int(i128),
    Str(String),
}

impl DomainValue {
    pub fn from_literal(literal: &Literal, base: SimpleKind) -> Option<DomainValue> {
        // `rune` is a signed integer kind, so handle it before the integer arm
        // to accept char literals as codepoints. A negative const is stored as
        // its two's-complement `u64`, so signed bases reinterpret it as `i64`.
        match base {
            SimpleKind::Rune => match literal {
                Literal::Char(text) => rune_codepoint(text).map(|cp| DomainValue::Int(cp as i128)),
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i64 as i128)),
                _ => None,
            },
            SimpleKind::String => match literal {
                Literal::String { value, .. } => Some(DomainValue::Str(value.clone())),
                _ => None,
            },
            _ if is_unsigned_base(base) => match literal {
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i128)),
                _ => None,
            },
            _ if base.is_signed_int() => match literal {
                Literal::Integer { value, .. } => Some(DomainValue::Int(*value as i64 as i128)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `uintptr` is an unsigned integer for value purposes but is excluded from
/// `SimpleKind::is_unsigned_int`, so it is folded in here.
fn is_unsigned_base(base: SimpleKind) -> bool {
    base.is_unsigned_int() || base == SimpleKind::Uintptr
}

impl Store {
    pub fn closed_domain(&self, type_id: &str) -> Option<ClosedDomain> {
        let definition = self.get_definition(type_id)?;
        if !definition.is_closed_domain() {
            return None;
        }

        // Float domains rely on exact equality over fragile values and do not
        // occur in the Go stdlib, so they are deliberately unsupported.
        let base = self.underlying_simple_kind(&definition.ty)?;
        if base.is_float() {
            return None;
        }

        let declaring_module = self.module_for_qualified_name(type_id)?;
        let module = self.get_module(declaring_module)?;
        let mut members: Vec<ClosedMember> = module
            .definitions
            .iter()
            .filter_map(|(qualified_name, definition)| {
                let literal = definition.const_value()?;
                let Type::Nominal { id, .. } = &definition.ty else {
                    return None;
                };
                if id != type_id || DomainValue::from_literal(literal, base).is_none() {
                    return None;
                }
                Some(ClosedMember {
                    display_name: domain_display_name(qualified_name.as_str()).into(),
                    literal: literal.clone(),
                })
            })
            .collect();
        if members.is_empty() {
            return None;
        }
        members.sort_by_key(|member| member.value(base));

        Some(ClosedDomain {
            base,
            type_display: domain_display_name(type_id).into(),
            members,
        })
    }
}

fn domain_display_name(qualified: &str) -> String {
    let Some((module, name)) = qualified.rsplit_once('.') else {
        return qualified.to_string();
    };
    match module.strip_prefix("go:") {
        Some(go_module) => {
            let package = go_module.rsplit('/').next().unwrap_or(go_module);
            format!("{package}.{name}")
        }
        None => name.to_string(),
    }
}
