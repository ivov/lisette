use super::Definition;
use crate::types::{self, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GoHint {
    AnonStruct,
    BitFlagSet,
    ClosedDomain,
    CollapsedTypeParams,
    CommaOk,
    ComparableIf,
    Constraint,
    HiddenEmbed,
    HiddenFields,
    Incomparable,
    ReadOnly,
    SentinelMinusOne,
    Skipped,
    SupersededBy,
    Unexported,
    ValueMethodSet,
    ZeroSafe,
    ZeroUnsafe,
}
impl GoHint {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "anon_struct" => Self::AnonStruct,
            "bit_flag_set" => Self::BitFlagSet,
            "closed_domain" => Self::ClosedDomain,
            "collapsed_type_params" => Self::CollapsedTypeParams,
            "comma_ok" => Self::CommaOk,
            "comparable_if" => Self::ComparableIf,
            "constraint" => Self::Constraint,
            "hidden_embed" => Self::HiddenEmbed,
            "hidden_fields" => Self::HiddenFields,
            "incomparable" => Self::Incomparable,
            "read_only" => Self::ReadOnly,
            "sentinel_minus_one" => Self::SentinelMinusOne,
            "skipped" => Self::Skipped,
            "superseded_by" => Self::SupersededBy,
            "unexported" => Self::Unexported,
            "value_method_set" => Self::ValueMethodSet,
            "zero_safe" => Self::ZeroSafe,
            "zero_unsafe" => Self::ZeroUnsafe,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoReturnContract {
    Value,
    Error,
    Result,
    Partial,
    CommaOk,
    Nullable,
    Sentinel(i64),
    Tuple(usize),
}

pub fn go_return_contract<'a>(
    ty: &Type,
    hints: &[GoHint],
    lookup: impl Fn(&str) -> Option<&'a Definition>,
) -> GoReturnContract {
    if ty.is_partial() {
        return GoReturnContract::Partial;
    }
    if ty.is_result() {
        return if ty.ok_type().is_unit() {
            GoReturnContract::Error
        } else {
            GoReturnContract::Result
        };
    }
    if ty.is_option() {
        if hints.contains(&GoHint::SentinelMinusOne) {
            return GoReturnContract::Sentinel(-1);
        }
        return if hints.contains(&GoHint::CommaOk)
            || !types::is_nilable_go_type(&ty.ok_type(), lookup)
        {
            GoReturnContract::CommaOk
        } else {
            GoReturnContract::Nullable
        };
    }
    match ty.tuple_arity() {
        Some(arity) if arity >= 2 => GoReturnContract::Tuple(arity),
        _ => GoReturnContract::Value,
    }
}

#[derive(Debug, Clone)]
pub struct GoInteropContract {
    pub declared: Type,
    pub instantiated: Type,
    pub hints: Vec<GoHint>,
}
impl GoInteropContract {
    pub fn new(declared: &Type, instantiated: &Type, hints: &[GoHint]) -> Self {
        Self {
            declared: declared.clone(),
            instantiated: instantiated.clone(),
            hints: hints.to_vec(),
        }
    }
    pub fn return_contract<'a>(
        &self,
        lookup: impl Fn(&str) -> Option<&'a Definition>,
    ) -> GoReturnContract {
        self.declared
            .unwrap_forall()
            .get_function_ret()
            .map_or(GoReturnContract::Value, |ty| {
                go_return_contract(ty, &self.hints, lookup)
            })
    }
    pub fn permits_nullable_lift<'a>(
        &self,
        implementation_return: &Type,
        lookup: impl Fn(&str) -> Option<&'a Definition>,
    ) -> bool {
        let Some(required) = self.instantiated.unwrap_forall().get_function_ret() else {
            return false;
        };
        required.is_option()
            && required.ok_type() == *implementation_return
            && self.return_contract(lookup) == GoReturnContract::Nullable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOrigin {
    Lisette,
    GoParameter,
    GoReturn,
    GoField,
    GoAny,
}

impl SlotOrigin {
    pub fn go_parameter(is_any: bool) -> Self {
        Self::go_slot(Self::GoParameter, is_any)
    }

    pub fn go_return(is_any: bool) -> Self {
        Self::go_slot(Self::GoReturn, is_any)
    }

    pub fn go_field(is_any: bool) -> Self {
        Self::go_slot(Self::GoField, is_any)
    }

    fn go_slot(origin: Self, is_any: bool) -> Self {
        if is_any { Self::GoAny } else { origin }
    }

    pub fn nested(self) -> Self {
        if matches!(self, Self::GoAny) {
            Self::Lisette
        } else {
            self
        }
    }

    pub fn declared_by_go(self) -> bool {
        matches!(self, Self::GoParameter | Self::GoReturn | Self::GoField)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoOptionRepresentation {
    Tagged,
    Nullable,
    Pointer,
}

pub fn go_option_representation<'a>(
    ty: &Type,
    origin: SlotOrigin,
    lookup: impl Fn(&str) -> Option<&'a Definition>,
) -> GoOptionRepresentation {
    if !origin.declared_by_go() || !ty.is_option() {
        return GoOptionRepresentation::Tagged;
    }
    let inner = ty.ok_type();
    if types::is_nilable_go_type(&inner, &lookup) {
        GoOptionRepresentation::Nullable
    } else if types::contains_unknown(&inner, &lookup) || inner.has_name("any") {
        GoOptionRepresentation::Tagged
    } else {
        GoOptionRepresentation::Pointer
    }
}

pub fn go_boundary_converts_container<'a>(
    ty: &Type,
    origin: SlotOrigin,
    lookup: impl Fn(&str) -> Option<&'a Definition>,
) -> bool {
    fn converts<'a>(
        ty: &Type,
        origin: SlotOrigin,
        lookup: &impl Fn(&str) -> Option<&'a Definition>,
    ) -> bool {
        let ty = types::peel_alias(ty, lookup);
        if ty.is_option() {
            return go_option_representation(&ty, origin, lookup) != GoOptionRepresentation::Tagged;
        }
        match ty {
            Type::Compound {
                kind:
                    types::CompoundKind::Slice
                    | types::CompoundKind::EnumeratedSlice
                    | types::CompoundKind::Map,
                args: arguments,
                ..
            } => arguments.iter().any(|ty| converts(ty, origin, lookup)),
            Type::Array { element, .. } => converts(&element, origin, lookup),
            _ => false,
        }
    }
    converts(ty, origin, &lookup)
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GoTypeRecipe {
    Parameter(String),
    Slice(Box<GoTypeRecipe>),
    Map(Box<GoTypeRecipe>, Box<GoTypeRecipe>),
    Array(Box<GoTypeRecipe>, u64),
}
impl GoTypeRecipe {
    pub fn parse_list(input: &str) -> Option<Vec<Self>> {
        let mut depth = 0u32;
        let mut start = 0;
        let mut entries = Vec::new();
        for (index, character) in input.char_indices() {
            match character {
                '<' => depth += 1,
                '>' => depth = depth.checked_sub(1)?,
                ',' if depth == 0 => {
                    entries.push(Self::parse(&input[start..index])?);
                    start = index + 1;
                }
                _ => {}
            }
        }
        if depth != 0 {
            return None;
        }
        entries.push(Self::parse(&input[start..])?);
        Some(entries)
    }
    fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if let Some((name, arguments)) = input.split_once('<') {
            let arguments = arguments.strip_suffix('>')?;
            return match name.trim() {
                "Slice" => {
                    let mut arguments = Self::parse_list(arguments)?;
                    (arguments.len() == 1).then(|| Self::Slice(Box::new(arguments.remove(0))))
                }
                "Map" => {
                    let mut arguments = Self::parse_list(arguments)?;
                    (arguments.len() == 2).then(|| {
                        Self::Map(Box::new(arguments.remove(0)), Box::new(arguments.remove(0)))
                    })
                }
                "Array" => {
                    let (element, length) = arguments.rsplit_once(',')?;
                    Some(Self::Array(
                        Box::new(Self::parse(element)?),
                        length.trim().parse().ok()?,
                    ))
                }
                _ => None,
            };
        }
        let mut chars = input.chars();
        if !chars
            .next()
            .is_some_and(|character| character == '_' || character.is_alphabetic())
            || !chars.all(|character| character == '_' || character.is_alphanumeric())
        {
            return None;
        }
        Some(Self::Parameter(input.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_conversion_respects_slot_origin_and_payload_representation() {
        use crate::types::CompoundKind;
        let reference = Type::Compound {
            kind: CompoundKind::Ref,
            args: vec![Type::int()],
            writable: false,
        };
        for origin in [
            SlotOrigin::GoParameter,
            SlotOrigin::GoReturn,
            SlotOrigin::GoField,
        ] {
            assert_eq!(
                go_option_representation(&Type::option_of(reference.clone()), origin, |_| None),
                GoOptionRepresentation::Nullable
            );
            assert_eq!(
                go_option_representation(&Type::option_of(Type::int()), origin, |_| None),
                GoOptionRepresentation::Pointer
            );
            assert_eq!(
                go_option_representation(
                    &Type::option_of(Type::Nominal {
                        id: "prelude.Unknown".into(),
                        params: vec![],
                        writable: false
                    }),
                    origin,
                    |_| None
                ),
                GoOptionRepresentation::Tagged
            );
        }
        for origin in [SlotOrigin::Lisette, SlotOrigin::GoAny] {
            assert_eq!(
                go_option_representation(&Type::option_of(reference.clone()), origin, |_| None),
                GoOptionRepresentation::Tagged
            );
        }
        let container = Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![Type::option_of(reference)],
            writable: true,
        };
        assert!(go_boundary_converts_container(
            &container,
            SlotOrigin::GoField,
            |_| None
        ));
        assert!(!go_boundary_converts_container(
            &container,
            SlotOrigin::Lisette,
            |_| None
        ));
    }

    #[test]
    fn nested_recipes_are_checked_at_registration_boundary() {
        let recipe = GoTypeRecipe::parse_list("Map<K, Slice<Array<V, 2>>>, K, V").unwrap();
        assert_eq!(recipe.len(), 3);
        assert!(
            matches!(&recipe[0], GoTypeRecipe::Map(_, value) if matches!(value.as_ref(), GoTypeRecipe::Slice(_)))
        );
        for invalid in [
            "",
            "Map<K>",
            "Slice<T>>",
            "Array<T, -1>",
            "Unknown<T>",
            "T,",
            "Slice<>",
        ] {
            assert!(GoTypeRecipe::parse_list(invalid).is_none(), "{invalid}");
        }
    }
}
