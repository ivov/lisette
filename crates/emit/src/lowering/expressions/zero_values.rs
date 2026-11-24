use crate::Planner;
use crate::lowering::abi::layout::ValueLayout;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::expressions::access::struct_call::plan_struct_literal;
use crate::output::finish::function_literals::immediate_call;
use crate::target::ir::bodies::{
    LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, assign, declare, define,
};
use crate::target::ir::go_expression::FunctionLiteralLayout;
use crate::target::ir::prelude::option_variant;
use crate::target::ir::values::{ConstantKind, GoExpression};
use crate::target::names::go_name;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::program::{AliasKind, DefinitionBody, FieldSelection};
use syntax::program::{SlotOrigin, is_go_import};
use syntax::types::{CompoundKind, SimpleKind, Type};

impl Planner<'_> {
    pub(in crate::lowering) fn go_field_map_zero(
        &mut self,
        owner: &Type,
        selection: &FieldSelection,
        field: &str,
        field_ty: &Type,
    ) -> Option<GoExpression> {
        let layout = self.field_slot_layout(owner, Some(selection), field, field_ty)?;
        if !layout_is_map(&layout) {
            return None;
        }
        Some(GoExpression::empty_composite(layout.go_type(self)))
    }

    pub(in crate::lowering) fn go_imported_newtype_zero(
        &mut self,
        ty: &Type,
    ) -> Option<GoExpression> {
        let Type::Nominal { id, .. } = ty else {
            return None;
        };
        if !is_go_import(id.as_str()) {
            return None;
        }
        let underlying = self.facts.newtype_underlying(ty)?;
        let go_ty = self.go_type(ty);
        self.go_newtype_zero(go_ty, &underlying)
    }

    fn go_newtype_zero(&mut self, go_ty: GoType, underlying: &Type) -> Option<GoExpression> {
        let layout = self.value_layout(underlying, SlotOrigin::Lisette);
        match layout.underlying() {
            ValueLayout::Map { .. } | ValueLayout::Array { .. } => {
                Some(GoExpression::empty_composite(go_ty))
            }
            ValueLayout::Slice { .. } => Some(GoExpression::conversion(go_ty, GoExpression::nil())),
            ValueLayout::Plain(simple @ Type::Simple(_)) => {
                let inner = self.lisette_zero(simple);
                Some(GoExpression::conversion(go_ty, inner))
            }
            ValueLayout::Plain(nominal @ Type::Nominal { id, .. })
                if is_go_import(id.as_str())
                    && self
                        .facts
                        .definition(id.as_str())
                        .is_some_and(|definition| {
                            definition.is_newtype()
                                || matches!(definition.body, DefinitionBody::Struct { .. })
                        }) =>
            {
                let inner = self.lisette_zero(nominal);
                Some(GoExpression::conversion(go_ty, inner))
            }
            _ => None,
        }
    }

    fn go_imported_zero(&mut self, ty: &Type, id: &str) -> GoExpression {
        let go_ty = self.go_type(ty);
        let is_struct = matches!(
            self.facts.definition(id).map(|d| &d.body),
            Some(DefinitionBody::Struct { .. })
        );
        let is_opaque = matches!(
            self.facts.definition(id).map(|d| &d.body),
            Some(DefinitionBody::TypeAlias {
                alias: AliasKind::Opaque(_),
                ..
            })
        );
        if !is_struct && !is_opaque {
            return GoExpression::new_zero(go_ty);
        }
        if is_struct && let Some(fields) = self.facts.struct_fields(ty) {
            let mut pairs: Vec<(String, GoExpression)> = Vec::new();
            for (name, selection) in fields {
                let field_ty = selection.field_ty();
                let Some(zero) = self.go_field_map_zero(ty, &selection, &name, &field_ty) else {
                    continue;
                };
                pairs.push((self.facts.struct_field_go_name(ty, &name, false), zero));
            }
            return plan_struct_literal(&go_ty, pairs);
        }
        GoExpression::empty_composite(go_ty)
    }

    pub(in crate::lowering) fn lisette_zero(&mut self, ty: &Type) -> GoExpression {
        self.lisette_zero_expression(ty, false)
            .expect("required zero expression")
    }

    pub(in crate::lowering) fn lisette_zero_expression(
        &mut self,
        ty: &Type,
        omit_go_zero: bool,
    ) -> Option<GoExpression> {
        if omit_go_zero && self.facts.go_zero_is_lisette_zero(ty) {
            return None;
        }
        let layout = self.value_layout(ty, SlotOrigin::Lisette);
        let zero = match (ty, &layout) {
            (Type::Simple(kind), _) => {
                let zero = match kind {
                    SimpleKind::Bool => {
                        GoExpression::constant("false".to_string(), ConstantKind::Bool)
                    }
                    SimpleKind::String => {
                        GoExpression::constant("\"\"".to_string(), ConstantKind::String)
                    }
                    SimpleKind::Unit => self.unit_value(None, None),
                    _ => GoExpression::constant("0".to_string(), ConstantKind::Int),
                };
                return Some(zero);
            }
            (
                Type::Compound {
                    kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
                    ..
                },
                ValueLayout::Slice { element, .. },
            ) => {
                let element = element.go_type(self);
                let go_type = GoType::slice_of(&element);
                GoExpression::conversion(go_type, GoExpression::nil())
            }
            (
                Type::Compound {
                    kind: CompoundKind::Map,
                    ..
                },
                ValueLayout::Map { key, value, .. },
            ) => {
                let key = key.go_type(self);
                let value = value.go_type(self);
                GoExpression::empty_composite(GoType::map_of(&key, &value))
            }
            (Type::Compound { .. }, _) => GoExpression::empty_composite(self.go_type(ty)),
            (Type::Nominal { id, params, .. }, _) => {
                return self.lisette_zero_nominal(ty, id.as_str(), params, omit_go_zero);
            }
            (Type::Tuple(slots), ValueLayout::Tuple { elements, .. }) => {
                let parts: Vec<GoExpression> = elements
                    .iter()
                    .map(|element| self.lisette_zero(element.logical_type()))
                    .collect();
                let callee = self.make_tuple_callee(slots, parts.len());
                GoExpression::pure_call(callee, parts)
            }
            (
                Type::Array { .. },
                ValueLayout::Array {
                    length, element, ..
                },
            ) => self.array_zero(*length, element.logical_type()),
            (Type::Parameter(name), _) => {
                let go_name = self.generic_go_name(name);
                GoExpression::new_zero(GoType::new(go_name.to_string()))
            }
            _ => GoExpression::empty_composite(self.go_type(ty)),
        };
        Some(zero)
    }

    fn lisette_zero_nominal(
        &mut self,
        ty: &Type,
        id: &str,
        params: &[Type],
        omit_go_zero: bool,
    ) -> Option<GoExpression> {
        if id == "prelude.Option" {
            let inner = params
                .first()
                .map(|a| self.go_type(a))
                .unwrap_or_else(|| GoType::new("any"));
            let zero = GoExpression::call(
                GoExpression::instantiation(
                    GoExpression::generated(
                        GeneratedPackage::Prelude,
                        option_variant("None").constructor,
                    ),
                    GoType::type_arguments(&[inner]),
                ),
                Vec::new(),
            );
            return Some(zero);
        }
        let go_import = is_go_import(id);
        if go_import
            && (self.facts.is_interface(ty) || self.facts.resolve_to_function_type(ty).is_some())
        {
            return Some(self.go_zero(ty));
        }
        if let Some(underlying) = self.facts.newtype_underlying(ty) {
            let go_ty = self.go_type(ty);
            if go_import {
                let zero = self
                    .go_newtype_zero(go_ty.clone(), &underlying)
                    .unwrap_or_else(|| GoExpression::new_zero(go_ty));
                return Some(zero);
            }
            let inner = self.lisette_zero(&underlying);
            return Some(GoExpression::conversion(go_ty, inner));
        }
        if go_import {
            return Some(self.go_imported_zero(ty, id));
        }
        if let Some(fields) = self.facts.struct_fields(ty) {
            let is_tuple = self.facts.is_tuple_struct_type(ty);
            let pairs: Vec<(String, GoExpression)> = fields
                .into_iter()
                .enumerate()
                .map(|(index, (name, selection))| (index, name, selection.field_ty()))
                .filter_map(|(index, name, field_ty)| {
                    let zero = self.lisette_zero_expression(&field_ty, !is_tuple)?;
                    let go_name = if is_tuple {
                        go_name::tuple_struct_field(index)
                    } else {
                        self.facts.struct_field_go_name(ty, &name, false)
                    };
                    Some((go_name, zero))
                })
                .collect();
            return Some(plan_struct_literal(&self.go_type(ty), pairs));
        }
        if let Some(underlying) = self.facts.immediate_underlying(ty) {
            return self.lisette_zero_expression(&underlying, omit_go_zero);
        }
        Some(GoExpression::empty_composite(self.go_type(ty)))
    }

    fn array_zero(&mut self, len: u64, elem: &Type) -> GoExpression {
        let elem_go = self.go_type(elem);
        let array_type = GoType::array_of(len, &elem_go);
        if len == 0 || self.facts.go_zero_is_lisette_zero(elem) {
            return GoExpression::empty_composite(array_type);
        }
        self.zero_filled(array_type, elem, None)
    }

    pub(in crate::lowering) fn zero_filled(
        &mut self,
        container_type: GoType,
        element: &Type,
        make_length: Option<GoExpression>,
    ) -> GoExpression {
        let zero = self.lisette_zero(element);
        let hint = if make_length.is_some() { "s" } else { "arr" };
        let container_name = self.scope.helper_local(hint, self.names);
        let index_name = self.scope.helper_local("i", self.names);
        let container = || GoExpression::identifier(container_name.clone());
        let index = || GoExpression::identifier(index_name.clone());
        let initial = match make_length {
            Some(length) => define(
                container_name.clone(),
                GoExpression::call(
                    GoExpression::external_name("make".to_string()),
                    vec![GoExpression::type_name(container_type.clone()), length],
                ),
            ),
            None => declare(container_name.clone(), container_type.clone()),
        };
        immediate_call(
            container_type,
            LoweredBlock {
                statements: vec![
                    initial,
                    LoweredStatement::Loop(LoopPlan {
                        kind: LoopKind::Generated { label: None },
                        header: LoopHeader::Range {
                            key: Some(index_name.clone()),
                            value: None,
                            iterable: container(),
                        },
                        body: LoweredBlock {
                            statements: vec![assign(
                                GoExpression::index(container(), index()),
                                zero,
                            )],
                        },
                    })
                    .into(),
                    plain_return(container()),
                ],
            },
            FunctionLiteralLayout::MultiLine,
        )
    }

    pub(in crate::lowering) fn go_zero(&self, ty: &Type) -> GoExpression {
        self.go_zero_of_layout(&self.value_layout(ty, SlotOrigin::Lisette))
    }

    pub(in crate::lowering) fn go_zero_of_layout(&self, layout: &ValueLayout) -> GoExpression {
        if self.facts.is_interface(layout.logical_type()) {
            return GoExpression::nil();
        }

        let go_ty = layout.go_type(self);

        let literal = |text: &str| GoExpression::literal(text.to_string());
        let constant = |text: &str, kind| GoExpression::constant(text.to_string(), kind);

        match layout {
            ValueLayout::Plain(Type::Simple(kind)) => match kind {
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
                | SimpleKind::Rune => constant("0", ConstantKind::Int),
                SimpleKind::Float32 | SimpleKind::Float64 => constant("0.0", ConstantKind::Float),
                SimpleKind::Bool => constant("false", ConstantKind::Bool),
                SimpleKind::String => constant("\"\"", ConstantKind::String),
                SimpleKind::Unit => self.unit_value(None, None),
                SimpleKind::Complex64 | SimpleKind::Complex128 => GoExpression::new_zero(go_ty),
            },
            ValueLayout::Array { .. } => GoExpression::empty_composite(go_ty),
            ValueLayout::Slice { .. }
            | ValueLayout::Map { .. }
            | ValueLayout::Function { .. }
            | ValueLayout::Reference { .. }
            | ValueLayout::NullableOption { .. }
            | ValueLayout::PointerOption { .. }
            | ValueLayout::Plain(Type::Compound {
                kind:
                    CompoundKind::Ref
                    | CompoundKind::Channel
                    | CompoundKind::Sender
                    | CompoundKind::Receiver,
                ..
            }) => literal("nil"),
            ValueLayout::Tuple { .. }
            | ValueLayout::TaggedOption { .. }
            | ValueLayout::Fallible { .. } => GoExpression::empty_composite(go_ty),
            ValueLayout::Plain(Type::Nominal { id, .. })
                if self
                    .facts
                    .definition(id.as_str())
                    .is_some_and(|definition| {
                        matches!(
                            definition.body,
                            DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. }
                        )
                    }) =>
            {
                GoExpression::empty_composite(go_ty)
            }
            _ => GoExpression::new_zero(go_ty),
        }
    }
}

fn layout_is_map(layout: &ValueLayout) -> bool {
    match layout {
        ValueLayout::Map { .. } => true,
        ValueLayout::Named { underlying, .. } => layout_is_map(underlying),
        _ => false,
    }
}
