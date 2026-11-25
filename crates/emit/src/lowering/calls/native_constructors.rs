use super::NativeCallContext;
use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::go_returns::multi_value_return;
use crate::lowering::abi::return_abi;
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::ValuePlan;
use crate::output::finish::function_literals::function_literal;
use crate::output::render::expressions::rendered;
use crate::target::ir::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement};
use crate::target::ir::go_expression::{
    BinaryOp, CompositeLayout, FunctionLiteralLayout, GoParameter,
};
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::types::go_type::{ChannelDirection, GoType};
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::ast::{Expression, Literal};
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::types::{SimpleKind, Type};

fn keys_checked_as_go_constants(keys: &[&Expression], key_kind: Option<SimpleKind>) -> bool {
    let Some(literals) = keys
        .iter()
        .map(|key| match key {
            Expression::Literal { literal, .. } => Some(literal),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };

    let integer_key = key_kind.is_some_and(SimpleKind::is_integer);
    literals
        .iter()
        .all(|literal| matches!(literal, Literal::Boolean(_) | Literal::String { .. }))
        || integer_key
            && (literals
                .iter()
                .all(|literal| matches!(literal, Literal::Integer { .. }))
                || literals
                    .iter()
                    .all(|literal| matches!(literal, Literal::Char(_))))
}

impl Planner<'_> {
    fn native_element_type(&mut self, function: &Expression) -> GoType {
        let element = self.facts.checked(function).callee().first_type_argument();
        self.go_type(element)
    }

    fn stage_size_argument(&mut self, ctx: &NativeCallContext) -> (Vec<Statement>, GoExpression) {
        match ctx.args.first() {
            Some(a) => self
                .plan_operand(a, ExpressionContext::value())
                .into_parts(),
            None => (Vec::new(), GoExpression::literal("0".to_string())),
        }
    }

    pub(super) fn try_plan_native_constructor(
        &mut self,
        ctx: &NativeCallContext,
    ) -> Option<ValuePlan> {
        match (ctx.native_type, ctx.method) {
            (NativeTypeKind::Channel, "new") => {
                let element = self.native_element_type(ctx.function);
                Some(ValuePlan::computed(
                    Vec::new(),
                    GoExpression::call(
                        GoExpression::external_name("make".to_string()),
                        vec![GoExpression::type_name(GoType::channel(
                            ChannelDirection::Bidirectional,
                            &element,
                        ))],
                    ),
                ))
            }
            (NativeTypeKind::Channel, "buffered") => {
                let element = self.native_element_type(ctx.function);
                let (setup, capacity) = self.stage_size_argument(ctx);
                Some(ValuePlan::computed(
                    setup,
                    GoExpression::call(
                        GoExpression::external_name("make".to_string()),
                        vec![
                            GoExpression::type_name(GoType::channel(
                                ChannelDirection::Bidirectional,
                                &element,
                            )),
                            capacity,
                        ],
                    ),
                ))
            }
            (NativeTypeKind::Map, "new") => {
                let (key, val) = map_entry_types(&self.facts, ctx.function);
                let (key, val) = (self.go_type(&key), self.go_type(&val));
                Some(ValuePlan::computed(
                    Vec::new(),
                    GoExpression::call(
                        GoExpression::external_name("make".to_string()),
                        vec![GoExpression::type_name(GoType::map_of(&key, &val))],
                    ),
                ))
            }
            (NativeTypeKind::Map, "from") => self.try_plan_map_from_pairs(ctx),
            (NativeTypeKind::Slice, "new") => {
                let element = self.native_element_type(ctx.function);
                Some(ValuePlan::computed(
                    Vec::new(),
                    GoExpression::empty_composite(GoType::slice_of(&element)),
                ))
            }
            (NativeTypeKind::Slice, "make") => Some(self.plan_slice_make(ctx)),
            (NativeTypeKind::Array, "new") => {
                let array_ty = ctx
                    .call_ty
                    .filter(|ty| self.facts.array_parts(ty).is_some())?;
                Some(ValuePlan::computed(Vec::new(), self.lisette_zero(array_ty)))
            }
            (NativeTypeKind::Array, "from") => self.try_plan_array_from(ctx),
            _ => None,
        }
    }

    fn plan_slice_make(&mut self, ctx: &NativeCallContext) -> ValuePlan {
        let element_ty = self
            .facts
            .checked(ctx.function)
            .callee()
            .first_type_argument()
            .clone();

        let element = self.go_type(&element_ty);
        let (setup, length) = self.stage_size_argument(ctx);
        let value = if self.facts.go_zero_is_lisette_zero(&element_ty) {
            GoExpression::call(
                GoExpression::external_name("make".to_string()),
                vec![GoExpression::type_name(GoType::slice_of(&element)), length],
            )
        } else {
            self.zero_filled(GoType::slice_of(&element), &element_ty, Some(length))
        };

        ValuePlan::computed(setup, value)
    }

    fn try_plan_array_from(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
        let callee_ty = ctx.function.get_type();
        let returned = callee_ty
            .as_function_type()
            .map(|f| f.return_type.as_ref())
            .or(ctx.call_ty)?;
        let inner = self
            .facts
            .prelude_container_arguments(returned)?
            .into_iter()
            .next()?;
        let (length, element) = self.facts.array_parts(&inner)?;
        let argument = ctx.args.first()?;
        let abi = return_abi(&self.facts, returned, ReturnSlot::Function);
        let CallableReturnAbi::Lowered(
            shape @ LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }),
        ) = &abi
        else {
            return None;
        };

        let result_list = self.return_go_type(&abi, returned);
        let none_values = self.lowered_none_values(shape, returned);

        let element_go = self.go_type(&element);
        let array_go = GoType::array_of(length, &element_go);

        let staged = self.plan_operand(argument, ExpressionContext::value());
        let (setup, source) = staged.into_parts();

        let slice_name = self.scope.helper_local("s", self.names);
        let slice = || GoExpression::identifier(slice_name.clone());
        let literal = |text: String| GoExpression::literal(text);
        let value = GoExpression::call(
            function_literal(
                vec![GoParameter::new(
                    slice_name.clone(),
                    GoType::slice_of(&element_go),
                )],
                result_list,
                LoweredBlock {
                    statements: vec![
                        LoweredStatement::If(IfPlan::plain(
                            GoExpression::binary(
                                GoExpression::call(
                                    GoExpression::external_name("len".to_string()),
                                    vec![slice()],
                                ),
                                BinaryOp::Ne,
                                literal(length.to_string()),
                            ),
                            LoweredBlock {
                                statements: vec![multi_value_return(none_values)],
                            },
                            ElseArm::None,
                        ))
                        .into(),
                        multi_value_return(
                            shape.ok_values(vec![GoExpression::conversion(array_go, slice())]),
                        ),
                    ],
                },
                FunctionLiteralLayout::MultiLine,
            ),
            vec![source],
        );

        Some(ValuePlan::computed(setup, value))
    }

    fn try_plan_map_from_pairs(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
        if ctx.spread.is_some() {
            return None;
        }
        let [argument] = ctx.args else {
            return None;
        };
        let Expression::Literal {
            literal: Literal::Slice(entries),
            ..
        } = argument
        else {
            return None;
        };

        let pairs = entries
            .iter()
            .map(|entry| match entry {
                Expression::Tuple { elements, .. } => match elements.as_slice() {
                    [key, value] => Some((key, value)),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;

        let (key_ty, value_ty) = map_entry_types(&self.facts, ctx.function);
        let keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
        if !keys_checked_as_go_constants(&keys, self.facts.simple_kind(&key_ty)) {
            return None;
        }
        let value_go_ty = self.go_type(&value_ty);
        let map_ty = GoType::map_of(&self.go_type(&key_ty), &value_go_ty);

        if pairs.is_empty() {
            return Some(ValuePlan::computed(
                Vec::new(),
                GoExpression::empty_composite(map_ty),
            ));
        }

        let stages: Vec<ValuePlan> = pairs
            .iter()
            .flat_map(|(key, value)| [(*key, &key_ty), (*value, &value_ty)])
            .map(|(e, slot)| {
                let origin = self.facts.function_type_origin(slot, SlotOrigin::Lisette);
                self.plan_composite_value(
                    e,
                    ExpressionContext::value().with_function_slot_origin(origin),
                )
            })
            .collect();

        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "entry");
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();

        let mut lowered = Vec::with_capacity(pairs.len());
        let mut widest = 0;
        for (entry, (key, value)) in entries.iter().zip(&pairs) {
            let staged_key = values.next().expect("each pair stages a key");
            let staged_value = values.next().expect("each pair stages a value");
            let [key_coercion, value_coercion] =
                self.pair_slot_coercions(entry, [key, value], [&key_ty, &value_ty]);
            let (key_setup, coerced_key) = key_coercion.lower(self, staged_key);
            setup.extend(key_setup);
            let is_whole_literal =
                value_coercion.is_identity() && staged_value.is_composite_literal();
            let (value_setup, coerced_value) = value_coercion.lower(self, staged_value);
            setup.extend(value_setup);
            widest = widest
                .max(rendered(&coerced_key).len() + rendered(&coerced_value).len() + ": ".len());
            let coerced_value = if is_whole_literal {
                coerced_value.elide_composite_type(&value_go_ty)
            } else {
                coerced_value
            };
            lowered.push((Some(coerced_key), coerced_value));
        }

        let layout = CompositeLayout::for_elements(lowered.len(), widest);
        let value = GoExpression::composite(Some(map_ty), lowered, layout)
            .position_elements(|index, _| self.facts.source_position(&entries[index].get_span()));
        Some(ValuePlan::computed(setup, value))
    }
}

fn map_entry_types<'a>(facts: &EmitFacts<'a>, function: &Expression) -> (Type, Type) {
    let mut types = facts.checked(function).callee().type_arguments.types();
    match (types.next(), types.next()) {
        (Some(key), Some(value)) => (key.clone(), value.clone()),
        _ => panic!("internal error: a map constructor has no checked entry types"),
    }
}
