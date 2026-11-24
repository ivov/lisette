use crate::Planner;
use crate::lowering::abi::layout::{FunctionLayout, ValueLayout};
use crate::target::names::go_name;
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::{ChannelDirection, GoType, GoTypeKind, is_prelude_type};
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::program::SlotOrigin;
use syntax::program::{Definition, DefinitionBody};
use syntax::program::{GO_IMPORT_PREFIX, NativeTypeKind, strip_go_prefix, strip_prelude_prefix};
use syntax::types::{CompoundKind, SimpleKind, Type, peel_underlying, unqualified_name};

impl Planner<'_> {
    pub(in crate::lowering) fn plain_literal_go_type(&self, ty: &Type) -> Option<GoType> {
        let peeled = self.facts.peel_alias(ty);
        match &peeled {
            Type::Simple(_) => {}
            Type::Nominal { params, .. } if params.is_empty() => {}
            _ => return None,
        }
        Some(self.go_type(&peeled))
    }

    pub(in crate::lowering) fn target_go_type(&self, ty: &Type) -> GoType {
        self.go_type(&self.facts.peel_alias(ty))
    }

    pub(in crate::lowering) fn go_type(&self, ty: &Type) -> GoType {
        assert!(
            !self.facts.is_never(ty),
            "internal error: a `Never` value has no Go type, emit writes it as a statement"
        );
        self.component_go_type(ty)
    }

    pub(in crate::lowering) fn component_go_type(&self, ty: &Type) -> GoType {
        self.value_layout(ty, SlotOrigin::Lisette).go_type(self)
    }

    fn leaf_go_type(&self, ty: &Type) -> GoType {
        match ty {
            Type::Nominal {
                id,
                params: parameters,
                ..
            } => self.constructor_go_type(id, parameters, ty),
            Type::Simple(SimpleKind::Unit) | Type::Never => GoType::new("struct{}"),
            Type::Simple(kind) => GoType::new(kind.leaf_name().to_string()),
            Type::Compound {
                kind,
                args: arguments,
                ..
            } => self.compound_go_type(*kind, arguments, ty),
            Type::Var { .. } | Type::Uninferred | Type::Ignored => GoType::new("any"),
            Type::Forall { .. } => GoType::new("any"),
            Type::Parameter(name) => GoType::new(self.generic_go_name(name).into_owned()),
            Type::Error => self.reject_untyped_position("an error type"),
            Type::ImportNamespace(_) => self.reject_untyped_position("a package name"),
            Type::ReceiverPlaceholder => GoType::new("any"),
            Type::Function(_) | Type::Tuple(_) | Type::Array { .. } => {
                unreachable!("the value layout renders a structural type")
            }
        }
    }

    fn compound_go_type(&self, kind: CompoundKind, arguments: &[Type], ty: &Type) -> GoType {
        match kind {
            CompoundKind::Ref => unreachable!("the value layout renders a reference"),
            CompoundKind::VarArgs => {
                let element = self
                    .component_go_type(arguments.first().expect("varargs have an element type"));
                GoType::variadic(&element)
            }
            _ => {
                let native = NativeTypeKind::from_type(ty)
                    .expect("every other compound type is a native Go type");
                self.native_go_type(native, ty)
            }
        }
    }

    fn constructor_go_type(&self, qualified_name: &str, parameters: &[Type], ty: &Type) -> GoType {
        if let Some(index) = self.facts.type_parameter_alias_index(qualified_name)
            && let Some(target) = parameters.get(index)
        {
            return self.go_type(target);
        }

        if let Some(go) = self.anon_struct_go_type(qualified_name) {
            return go;
        }

        let (name, package) = self.unqualify_name(qualified_name);

        let kind = physical_type_kind(ty, |id| self.facts.definition(id));
        let param_types: Vec<GoType> = parameters
            .iter()
            .map(|parameter| self.component_go_type(parameter))
            .collect();
        if let Some(package) = package {
            let member = name.rsplit('.').next().unwrap_or(&name);
            return GoType::named(member, &param_types, Some(package)).with_physical_kind(kind);
        }
        if let Some(name) = strip_prelude_prefix(qualified_name)
            && is_prelude_type(name)
        {
            return GoType::prelude(name, &param_types);
        }
        GoType::named(&name, &param_types, None).with_physical_kind(kind)
    }

    fn native_go_type(&self, kind: NativeTypeKind, ty: &Type) -> GoType {
        let argument = |index| {
            let stripped = ty.strip_refs();
            let arguments = stripped.get_type_params().expect("native type arguments");
            self.component_go_type(&arguments[index])
        };
        match kind {
            NativeTypeKind::String => GoType::new("string"),
            NativeTypeKind::Slice | NativeTypeKind::EnumeratedSlice => {
                GoType::slice_of(&argument(0))
            }
            NativeTypeKind::Map => GoType::map_of(&argument(0), &argument(1)),
            NativeTypeKind::Channel => {
                GoType::channel(ChannelDirection::Bidirectional, &argument(0))
            }
            NativeTypeKind::Sender => GoType::channel(ChannelDirection::Send, &argument(0)),
            NativeTypeKind::Receiver => GoType::channel(ChannelDirection::Receive, &argument(0)),
            NativeTypeKind::Array => self.reject_untyped_position("an array without a length"),
        }
    }

    fn reject_untyped_position(&self, what: &'static str) -> GoType {
        self.namespace.rejections().reject(what, |span| {
            diagnostics::emit::type_without_go_form(what, span)
        });
        GoType::new("any")
    }

    fn unqualify_name(&self, id: &str) -> (String, Option<PackageUse>) {
        if id == "Unknown" {
            return ("any".to_string(), None);
        }
        let (package, unqualified) = if let Some(rest) = strip_go_prefix(id) {
            let Some((path, ty)) = rest.rsplit_once('.') else {
                return (go_name::escape_keyword(id).into_owned(), None);
            };
            (&id[..GO_IMPORT_PREFIX.len() + path.len()], ty)
        } else {
            let Some(split) = id.split_once('.') else {
                return (go_name::escape_type_name(id).into_owned(), None);
            };
            split
        };

        if unqualified == "Unknown" {
            return ("any".to_string(), None);
        }

        let escaped = self.facts.type_member_go_name(package, unqualified);

        if self.facts.is_foreign_package(package) {
            if !escaped.starts_with(char::is_uppercase) {
                self.namespace.rejections().reject(id, |span| {
                    let go_type = strip_go_prefix(id).unwrap_or(id);
                    diagnostics::emit::unnamable_go_type(go_type, span)
                });
            }
            let package = self.package_use_for_package(package);
            (
                format!("{}.{}", package.qualifier(), escaped),
                Some(package),
            )
        } else {
            (escaped.into_owned(), None)
        }
    }

    pub(in crate::lowering) fn return_go_type(
        &self,
        abi: &CallableReturnAbi,
        return_ty: &Type,
    ) -> GoType {
        let result_ty = if abi.is_lowered() {
            self.facts.peel_alias(return_ty)
        } else {
            return_ty.clone()
        };
        let result = self.value_layout(&result_ty, SlotOrigin::Lisette);
        self.function_layout(Vec::new(), &result, abi.clone(), None)
            .result_go_type(self)
            .unwrap_or_default()
    }

    fn anon_struct_go_type(&self, id: &str) -> Option<GoType> {
        let definition = self.facts.definition(id)?;
        if !definition.is_anon_struct() {
            return None;
        }
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return None;
        };
        Some(GoType::structure(
            fields
                .iter()
                .map(|field| {
                    (
                        self.facts.declared_field_go_name(id, field),
                        self.component_go_type(&field.ty),
                    )
                })
                .collect(),
        ))
    }
}

impl ValueLayout {
    pub(in crate::lowering) fn go_type(&self, planner: &Planner<'_>) -> GoType {
        match self {
            Self::NullableOption { payload, .. } => payload.go_type(planner),
            Self::PointerOption { payload, .. } => GoType::pointer_to(&payload.go_type(planner)),
            Self::Reference { pointee, .. } => GoType::pointer_to(&pointee.go_type(planner)),
            Self::Slice { element, .. } => GoType::slice_of(&element.go_type(planner)),
            Self::Map { key, value, .. } => {
                let key = key.go_type(planner);
                let value = value.go_type(planner);
                GoType::map_of(&key, &value)
            }
            Self::Array {
                length, element, ..
            } => GoType::array_of(length, &element.go_type(planner)),
            Self::Function {
                function_type,
                layout,
            } => {
                if self.function_name().is_some()
                    && self.same_representation(
                        &planner.value_layout(function_type, SlotOrigin::Lisette),
                    )
                {
                    planner.leaf_go_type(function_type)
                } else {
                    layout.go_type(planner)
                }
            }
            Self::TaggedOption { payload, .. } => {
                GoType::prelude("Option", &[payload.go_type(planner)])
            }
            Self::Fallible {
                fallible_type,
                payload,
                error,
            } => {
                let Type::Nominal { id, .. } = fallible_type else {
                    unreachable!("a fallible layout holds a prelude container")
                };
                GoType::prelude(
                    unqualified_name(id),
                    &[payload.go_type(planner), error.go_type(planner)],
                )
            }
            Self::Tuple { elements, .. } => {
                let types: Vec<_> = elements
                    .iter()
                    .map(|element| element.go_type(planner))
                    .collect();
                GoType::prelude(&format!("Tuple{}", elements.len()), &types)
            }
            Self::Named {
                named_type,
                underlying,
                go_alias: true,
            } => planner
                .leaf_go_type(named_type)
                .compared_as(underlying.go_type(planner)),
            Self::Plain(ty) | Self::Named { named_type: ty, .. } => planner.leaf_go_type(ty),
        }
    }
}

impl FunctionLayout {
    pub(in crate::lowering) fn go_type(&self, planner: &Planner<'_>) -> GoType {
        let parameters: Vec<GoType> = self
            .parameters
            .iter()
            .map(|parameter| parameter.go_type(planner))
            .collect();
        let result = self.result_go_type(planner);
        GoType::function(&parameters, result.as_ref())
    }

    fn result_err_type(&self, planner: &Planner<'_>) -> Type {
        planner.facts.err_payload(self.result_layout.logical_type())
    }

    pub(in crate::lowering) fn result_go_type(&self, planner: &Planner<'_>) -> Option<GoType> {
        if self.return_abi == CallableReturnAbi::Void {
            return None;
        }
        let mut slots: Vec<GoType> = self
            .results
            .iter()
            .map(|result| result.go_type(planner))
            .collect();
        Some(match self.return_abi.lowered() {
            None
            | Some(LoweredReturnAbi::Option(
                OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_),
            )) => {
                let [slot] = <[GoType; 1]>::try_from(slots)
                    .unwrap_or_else(|_| unreachable!("a single-result ABI has one result slot"));
                slot
            }
            Some(LoweredReturnAbi::BareError) => planner.go_type(&self.result_err_type(planner)),
            Some(LoweredReturnAbi::Result { .. } | LoweredReturnAbi::Partial { .. }) => {
                slots.push(planner.go_type(&self.result_err_type(planner)));
                GoType::result_list(&slots)
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })) => {
                slots.push(GoType::new("bool"));
                GoType::result_list(&slots)
            }
            Some(LoweredReturnAbi::Tuple { .. }) => GoType::result_list(&slots),
        })
    }
}

fn physical_type_kind<'argument>(
    ty: &Type,
    lookup: impl Fn(&str) -> Option<&'argument Definition>,
) -> GoTypeKind {
    match peel_underlying(ty, &lookup) {
        Type::Compound {
            kind: CompoundKind::Slice,
            ..
        } => GoTypeKind::Slice,
        Type::Compound {
            kind: CompoundKind::Map,
            ..
        } => GoTypeKind::Map,
        Type::Compound {
            kind: CompoundKind::Channel | CompoundKind::Sender | CompoundKind::Receiver,
            ..
        } => GoTypeKind::Channel,
        Type::Compound {
            kind: CompoundKind::Ref,
            args: arguments,
            ..
        } if arguments
            .first()
            .is_some_and(|inner| matches!(peel_underlying(inner, &lookup), Type::Array { .. })) =>
        {
            GoTypeKind::PointerToArray
        }
        Type::Array { .. } => GoTypeKind::Array,
        _ => GoTypeKind::Value,
    }
}
