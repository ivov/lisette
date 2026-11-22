use crate::abi::ReturnSlot;
use crate::expressions::access::struct_call::plan_struct_literal;
use crate::types::go_type::ChannelDirection;
use syntax::types::SimpleKind;

use super::{NativeCallContext, NativeCallForm};
use crate::EmitFacts;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::ValueLayout;
use crate::abi::transition::multi_value_return;
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, Statement,
    assign, define,
};
use crate::ir::go_expression::{
    BinaryOp, CompositeLayout, FunctionLiteralLayout, GoExpressionNode, GoParameter,
};
use crate::ir::values::{CaptureBoundary, GoExpression, Stability, ValuePlan};
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::matching::{PreludeVariant, prelude_constructor};
use crate::plan::calls::{CallPlan, CallableOrigin, ResolvedCallee};
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use crate::{Planner, written_span};
use syntax::EcoString;
use syntax::ast::{Expression, Literal};
use syntax::program::NativeOperation;
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::program::{CallOperation, FieldSelection};
use syntax::types::{CompoundKind, FunctionParameter, FunctionType, SubstitutionMap, Type};

struct TupleStructTarget<'a> {
    go_ty: GoType,
    field_tys: Vec<Type>,
    selections: Vec<&'a FieldSelection>,
    owner_type: Type,
}

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

pub(crate) struct CallTypeArgsRequest<'e, 'c> {
    pub(crate) function: &'e Expression,
    pub(crate) callee: &'e ResolvedCallee<'c>,
    pub(crate) written_type_arguments: usize,
    pub(crate) call_ty: Option<&'e Type>,
    pub(crate) arg_shape: CallArgShape,
    pub(crate) slot: Option<&'e Type>,
    pub(crate) receiver_passed: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CallArgShape {
    pub value_count: usize,
    pub has_spread: bool,
}

fn all_type_params_inferrable(
    vars: &[EcoString],
    params: &[FunctionParameter],
    receiver_count: usize,
    arg_shape: CallArgShape,
) -> bool {
    let variadic_idx = params
        .iter()
        .position(|p| p.ty.is_native(CompoundKind::VarArgs))
        .filter(|&i| i == params.len() - 1);
    let variadic_has_args = arg_shape.has_spread
        || variadic_idx.is_some_and(|i| arg_shape.value_count + receiver_count > i);

    vars.iter().all(|var| {
        let param_ty = Type::Parameter(var.clone());
        params.iter().enumerate().any(|(i, pt)| {
            pt.ty.contains_type(&param_ty) && (Some(i) != variadic_idx || variadic_has_args)
        })
    })
}

impl<'a> Planner<'a> {
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

    fn try_plan_native_constructor(&mut self, ctx: &NativeCallContext) -> Option<ValuePlan> {
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
                let (key, val) = self.facts.map_entry_types(ctx.function);
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
            let zero = self.lisette_zero(&element_ty);
            let slice_type = GoType::slice_of(&element);
            let slice_name = self.scope.helper_local("s", self.package);
            let index_name = self.scope.helper_local("i", self.package);
            let slice = || GoExpression::identifier(slice_name.clone());
            let index = || GoExpression::identifier(index_name.clone());
            GoExpression::immediate_call(
                slice_type.clone(),
                LoweredBlock {
                    statements: vec![
                        define(
                            slice_name.clone(),
                            GoExpression::call(
                                GoExpression::external_name("make".to_string()),
                                vec![GoExpression::type_name(slice_type), length],
                            ),
                        ),
                        LoweredStatement::Loop(LoopPlan {
                            kind: LoopKind::Generated { label: None },
                            header: LoopHeader::Range {
                                key: Some(index_name.clone()),
                                value: None,
                                iterable: slice(),
                            },
                            body: LoweredBlock {
                                statements: vec![assign(
                                    GoExpression::index(slice(), index()),
                                    zero,
                                )],
                            },
                        })
                        .into(),
                        plain_return(slice()),
                    ],
                },
                FunctionLiteralLayout::MultiLine,
            )
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
        let abi = self.facts.return_abi(returned, ReturnSlot::Function);
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

        let slice_name = self.scope.helper_local("s", self.package);
        let slice = || GoExpression::identifier(slice_name.clone());
        let literal = |text: String| GoExpression::literal(text);
        let value = GoExpression::call(
            GoExpression::function_literal(
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

        let (key_ty, value_ty) = self.facts.map_entry_types(ctx.function);
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
                let origin = self.function_type_origin(slot, SlotOrigin::Lisette);
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
                .max(coerced_key.rendered().len() + coerced_value.rendered().len() + ": ".len());
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

    pub(crate) fn try_plan_negated_call(
        &mut self,
        setup: &mut Vec<Statement>,
        call_expression: &Expression,
    ) -> Option<GoExpression> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            type_arguments,
            ..
        } = call_expression
        else {
            return None;
        };
        let spread = spread.as_deref();
        if self.is_local_binding(callee) {
            return None;
        }
        let CallOperation::Native { kind, method } = &self.facts.checked(callee).callee().operation
        else {
            return None;
        };
        let kind = *kind;
        let plan = self.plan_call(call_expression)?;
        let native_ctx = NativeCallContext {
            function: callee,
            form: NativeCallForm::method(callee),
            args,
            spread,
            written_type_arguments: type_arguments.annotations().len(),
            abi: plan.resolved.abi,
            call_ty: None,
            native_type: kind,
            method,
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
        };
        self.try_plan_negated_native_method(setup, &native_ctx)
    }

    pub(crate) fn plan_call_expression(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self
            .plan_call(call_expression)
            .expect("plan_call yields Some for a Call expression");
        self.plan_call_with_plan(call_expression, call_ty, ctx, plan)
    }

    pub(crate) fn plan_call_with_plan(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        plan: CallPlan<'a>,
    ) -> ValuePlan {
        let lowered = self.plan_unpositioned_call(call_expression, call_ty, ctx, plan);
        self.facts
            .with_source_positions(&written_span(call_expression), lowered)
    }

    fn plan_unpositioned_call(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
        plan: CallPlan<'a>,
    ) -> ValuePlan {
        let Expression::Call {
            expression: callee,
            args,
            type_arguments,
            spread,
            ..
        } = call_expression
        else {
            unreachable!("plan_call_expression requires a Call expression");
        };
        let spread = spread.as_deref();
        let written_type_arguments = type_arguments.annotations().len();

        if let Some(predicate) = self.try_plan_fused_predicate_value(call_expression, false) {
            return predicate;
        }
        if let Some(defaulted) = self.try_plan_defaulted_call_value(call_expression) {
            return defaulted;
        }

        match &plan.resolved.origin {
            CallableOrigin::Source(CallOperation::TupleStructConstructor {
                is_newtype: false,
                ..
            }) => {
                if let Some(result) = self.try_plan_tuple_struct_call(call_expression, call_ty, ctx)
                {
                    return result;
                }
            }
            CallableOrigin::Source(CallOperation::Zero) => {
                let ty = self
                    .facts
                    .checked(callee)
                    .callee()
                    .first_type_argument()
                    .clone();
                let value = self.lisette_zero(&ty);
                let value = match self.constant_needs_go_type(value.constant_kind(), &ty) {
                    Some(go_type) => GoExpression::conversion(go_type, value),
                    None => value,
                };
                return ValuePlan::computed(Vec::new(), value);
            }
            CallableOrigin::Source(CallOperation::AssertType) => {
                let (setup, value) = self.plan_assert_type(callee, args);
                return ValuePlan::plain_call(setup, value);
            }
            CallableOrigin::Source(CallOperation::UfcsFunction) => {
                return self.plan_ufcs_call(callee, args, written_type_arguments, spread, &plan);
            }
            CallableOrigin::Source(
                operation @ (CallOperation::NativeConstructor { kind, method }
                | CallOperation::Native { kind, method }),
            ) => {
                let form = match operation {
                    CallOperation::NativeConstructor { .. } => NativeCallForm::Constructor,
                    _ => NativeCallForm::method(callee),
                };
                let native_ctx = NativeCallContext {
                    function: callee,
                    form,
                    args,
                    spread,
                    written_type_arguments,
                    abi: plan.resolved.abi,
                    call_ty,
                    native_type: *kind,
                    method,
                    capture_boundary: ctx.capture_boundary(),
                    retired_receiver: ctx.retired_receiver(),
                };
                return self.plan_native_call(&native_ctx);
            }
            CallableOrigin::Source(CallOperation::ReceiverMethod { is_public }) => {
                let method = extract_receiver_ufcs_method(callee);
                let receiver = plan
                    .resolved
                    .instantiated
                    .as_function_type()
                    .and_then(|f| f.params.first())
                    .map_or(&Type::Never, |receiver| &receiver.ty);
                let go_method = self.facts.receiver_method_go_name(
                    receiver,
                    &method,
                    *is_public,
                    self.selects_tagged_twin(callee),
                );
                return self.plan_receiver_method_ufcs(args, go_method, spread, &plan);
            }
            CallableOrigin::GoInterop
            | CallableOrigin::Source(
                CallOperation::Function
                | CallOperation::BoundMethod { .. }
                | CallOperation::TupleStructConstructor {
                    is_newtype: true, ..
                }
                | CallOperation::Unavailable,
            ) => {}
        }

        self.plan_regular_call(call_expression, &plan, call_ty, ctx)
    }

    pub(super) fn plan_native_call(&mut self, context: &NativeCallContext) -> ValuePlan {
        if let Some(result) = self.try_plan_native_constructor(context) {
            return result;
        }
        let result = self.lower_native_method(context);
        let is_constructor = matches!(context.form, NativeCallForm::Constructor);
        let split = context.receiver_and_arguments();
        let reads_fixed_length = matches!(context.method, "length" | "capacity")
            && !is_constructor
            && !matches!(
                context.native_type,
                NativeTypeKind::Channel | NativeTypeKind::Sender | NativeTypeKind::Receiver
            )
            && split.is_some_and(|(receiver, _)| {
                self.facts.is_unmutated_identifier(receiver)
                    && !self.facts.is_reference(&receiver.get_type())
            });
        if let GoExpressionNode::Identifier(name) = result.value.node()
            && result
                .setup
                .iter()
                .any(|statement| statement.kind.binds_local(name))
        {
            return ValuePlan::captured(result.setup, name.clone());
        }
        let receiver_arity = split.map_or(0, |(_, arguments)| arguments.len());
        let plain_call = !is_constructor
            && NativeOperation::new(context.native_type, context.method)
                .lowers_to_plain_call(receiver_arity);
        let plan = if plain_call {
            ValuePlan::plain_call(result.setup, result.value)
        } else {
            ValuePlan::computed(result.setup, result.value)
        };
        if reads_fixed_length {
            plan.with_stability(Stability::Fixed)
        } else {
            plan
        }
    }

    fn try_plan_tuple_struct_call(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> Option<ValuePlan> {
        let Expression::Call { args, .. } = call_expression else {
            return None;
        };
        let target = self.resolve_tuple_struct_target(call_expression, call_ty)?;

        let arg_ctx = match (ctx.retired_receiver(), args.len()) {
            (Some(retired), 1) => ExpressionContext::value().with_retired_receiver(retired),
            _ => ExpressionContext::value(),
        };
        let field_layouts: Vec<_> = target
            .field_tys
            .iter()
            .zip(&target.selections)
            .enumerate()
            .map(|(index, (ty, selection))| {
                self.field_slot_layout(&target.owner_type, Some(selection), &index.to_string(), ty)
            })
            .collect();
        let (stages, source_layouts): (Vec<ValuePlan>, Vec<ValueLayout>) = args
            .iter()
            .zip(&target.field_tys)
            .map(|(a, slot)| {
                let origin = self.function_type_origin(slot, SlotOrigin::Lisette);
                let ctx = arg_ctx.with_function_slot_origin(origin);
                let source = self.emitted_value_layout(a, ctx);
                (self.plan_composite_value(a, ctx), source)
            })
            .unzip();
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "arg");
        let mut setup = sequenced.setup;

        let mut field_pairs = Vec::with_capacity(target.field_tys.len());
        for (i, (field_ty, value)) in target.field_tys.iter().zip(sequenced.values).enumerate() {
            let coerced = self.coerce_into_slot(
                &mut setup,
                (&args[i], value),
                field_ty,
                field_layouts[i]
                    .as_ref()
                    .map(|target| (&source_layouts[i], target)),
            );
            field_pairs.push((go_name::tuple_struct_field(i), coerced));
        }

        let value =
            plan_struct_literal(&target.go_ty, field_pairs).position_elements(|index, _| {
                args.get(index)
                    .and_then(|argument| self.facts.source_position(&argument.get_span()))
            });
        Some(ValuePlan::computed(setup, value))
    }

    fn resolve_tuple_struct_target(
        &mut self,
        call_expression: &Expression,
        call_ty: Option<&Type>,
    ) -> Option<TupleStructTarget<'a>> {
        let Expression::Call {
            expression: function,
            args,
            ..
        } = call_expression
        else {
            return None;
        };
        let ty = function.get_type();
        let f = ty.as_function_type()?;
        let return_ty = call_ty
            .cloned()
            .unwrap_or_else(|| f.return_type.as_ref().clone());
        let checked = self.facts.checked(call_expression);
        let selections: Vec<&'a FieldSelection> = (0..args.len())
            .map(|index| checked.positional_field(index))
            .collect();
        let field_tys = selections
            .iter()
            .map(|selection| selection.field_ty())
            .collect();
        let go_ty = self.go_type(&return_ty);
        Some(TupleStructTarget {
            go_ty,
            field_tys,
            selections,
            owner_type: return_ty,
        })
    }

    fn plan_assert_type(
        &mut self,
        function: &Expression,
        args: &[Expression],
    ) -> (Vec<Statement>, GoExpression) {
        let asserted = self
            .facts
            .checked(function)
            .callee()
            .first_type_argument()
            .clone();
        let function_layouts = self.any_slot_function_layouts(&asserted);
        let target_ty = match &function_layouts {
            Some((stored, _)) => stored.go_type(self),
            None => self.go_type(&asserted),
        };
        let (mut setup, arguments) = match args.first() {
            Some(a) => {
                let (setup, value) = self
                    .plan_composite_value(a, ExpressionContext::value())
                    .into_parts();
                (setup, vec![value])
            }
            None => (Vec::new(), Vec::new()),
        };
        let call = GoExpression::call(
            GoExpression::instantiation(
                GoExpression::generated(GeneratedPackage::Prelude, "AssertType"),
                GoType::type_arguments(&[target_ty]),
            ),
            arguments,
        );
        let Some((stored, lisette)) = function_layouts else {
            return (setup, call);
        };
        let option_type = function
            .get_type()
            .get_function_ret()
            .cloned()
            .expect("assert_type returns an Option");
        let option_layout = |payload| ValueLayout::TaggedOption {
            option_type: option_type.clone(),
            payload: Box::new(payload),
        };
        let bridge = CoercionPlan::bridge(self, &option_layout(stored), &option_layout(lisette));
        let (bridge_setup, value) = bridge.lower(self, call);
        setup.extend(bridge_setup);
        (setup, value)
    }

    fn is_local_binding(&self, function: &Expression) -> bool {
        if let Expression::Identifier {
            value, resolution, ..
        } = function
        {
            self.scope
                .resolve_identifier_with_resolution(value, resolution)
                .is_some()
        } else {
            false
        }
    }

    pub(crate) fn prelude_container_type_args(&mut self, ty: &Type) -> Option<GoType> {
        let params = self.facts.prelude_container_arguments(ty)?;
        if params.is_empty() {
            return None;
        }
        params
            .iter()
            .any(|p| {
                self.facts.is_interface_or_unknown(p) || self.facts.is_go_named_function_type(p)
            })
            .then(|| self.format_type_args(&params))
    }
}

fn extract_receiver_ufcs_method(function: &Expression) -> String {
    if let Expression::Identifier { value, .. } = function
        && let Some(last_dot) = value.rfind('.')
    {
        return value[last_dot + 1..].to_string();
    }
    String::new()
}

impl<'a> EmitFacts<'a> {
    fn map_entry_types(&self, function: &Expression) -> (Type, Type) {
        let mut types = self.checked(function).callee().type_arguments.types();
        match (types.next(), types.next()) {
            (Some(key), Some(value)) => (key.clone(), value.clone()),
            _ => panic!("internal error: a map constructor has no checked entry types"),
        }
    }

    pub(super) fn callee_go_name(&self, function: &Expression) -> Option<&str> {
        let symbol = self.checked(function).callee().definition.as_ref()?;
        self.definition(symbol)?.go_name()
    }

    pub(super) fn callee_is_go_builtin(&self, callee: &Expression) -> bool {
        self.go_builtin_name(callee).is_some()
    }

    pub(super) fn go_builtin_name(&self, callee: &Expression) -> Option<&'a str> {
        self.checked(callee)
            .callee()
            .definition
            .as_deref()
            .filter(|qualified| go_name::is_prelude_go_builtin(qualified))
            .and_then(|qualified| qualified.strip_prefix("prelude."))
    }
}

impl Planner<'_> {
    pub(crate) fn call_type_arguments(&mut self, request: CallTypeArgsRequest<'_, '_>) -> GoType {
        let CallTypeArgsRequest {
            function,
            callee,
            written_type_arguments,
            call_ty,
            arg_shape,
            slot,
            receiver_passed,
        } = request;
        if !receiver_passed && callee_curries_receiver(callee) {
            return GoType::default();
        }
        let receiver_count = if receiver_passed {
            callee.receiver_offset.min(1)
        } else {
            0
        };
        if let Some(recipe) = callee
            .declaration
            .and_then(|declaration| declaration.go_type_param_recipe())
        {
            let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
            if has_value_args
                && generic_function(callee.declared_type()).is_some_and(|(vars, f)| {
                    all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
                })
            {
                return GoType::default();
            }
            let type_arguments = &self.facts.checked(function).callee().type_arguments;
            return self
                .reconstruct_collapsed_type_args(recipe, type_arguments)
                .unwrap_or_default();
        }

        if written_type_arguments > 0 {
            if receiver_passed {
                let type_arguments = &self.facts.checked(function).callee().type_arguments;
                return callee
                    .declared_type()
                    .and_then(|declared| {
                        self.format_generic_instantiation(declared, type_arguments)
                    })
                    .unwrap_or_default();
            }
            let written = self
                .facts
                .checked(function)
                .callee()
                .written_type_arguments(written_type_arguments);
            return self.format_type_arg_iter(written);
        }

        if let Some(inferred) = self.uninferable_type_args(
            function,
            callee.declared_type(),
            receiver_count,
            arg_shape,
            !receiver_passed,
        ) {
            return match slot.filter(|_| !receiver_passed) {
                Some(slot) => self.prelude_container_type_args(slot).unwrap_or(inferred),
                None => inferred,
            };
        }

        if matches!(
            prelude_constructor(function),
            Some(
                PreludeVariant::Some
                    | PreludeVariant::Ok
                    | PreludeVariant::Err
                    | PreludeVariant::Partial(_)
            )
        ) {
            return call_ty
                .and_then(|ty| self.prelude_container_type_args(ty))
                .or_else(|| slot.and_then(|ty| self.prelude_container_type_args(ty)))
                .unwrap_or_default();
        }
        GoType::default()
    }

    pub(crate) fn constructor_reference_type_arguments(
        &mut self,
        fn_params: &[FunctionParameter],
        ret_params: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> GoType {
        let return_params_are_inferrable = ret_params.len() <= fn_params.len()
            && ret_params
                .iter()
                .all(|ret| fn_params.iter().any(|param| param.ty.contains_type(ret)));
        let parameters_support_go_inference = fn_params
            .iter()
            .all(|param| !self.facts.is_go_named_function_type(&param.ty));
        if ctx.is_callee() && return_params_are_inferrable && parameters_support_go_inference {
            GoType::default()
        } else {
            self.format_type_args(ret_params)
        }
    }

    fn uninferable_type_args(
        &self,
        function: &Expression,
        declared: Option<&Type>,
        receiver_count: usize,
        arg_shape: CallArgShape,
        function_alias_pins: bool,
    ) -> Option<GoType> {
        let (vars, f) = generic_function(declared)?;
        let mapping = &self.facts.checked(function).callee().type_arguments;
        if all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
            && !(function_alias_pins
                && vars.iter().any(|var| {
                    mapping
                        .get(var)
                        .is_some_and(|ty| self.facts.is_go_named_function_type(ty))
                }))
        {
            return None;
        }
        self.instantiated_type_args(vars, mapping)
    }

    pub(crate) fn format_value_type_args(&mut self, expression: &Expression) -> Option<GoType> {
        let recorded = self.facts.checked(expression).function_value()?;
        let declaration = self
            .facts
            .callable_declaration(recorded.definition.as_deref()?)?;
        let instantiation = &recorded.type_arguments;
        match declaration.go_type_param_recipe() {
            Some(recipe) => self.reconstruct_collapsed_type_args(recipe, instantiation),
            None => self.format_generic_instantiation(declaration.ty(), instantiation),
        }
    }

    fn format_generic_instantiation(
        &self,
        definition_ty: &Type,
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let Type::Forall { vars, .. } = definition_ty else {
            return None;
        };
        if vars.is_empty() {
            return None;
        }
        self.instantiated_type_args(vars, mapping)
    }

    fn instantiated_type_args(
        &self,
        vars: &[EcoString],
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let resolved = vars
            .iter()
            .map(|var| mapping.get(var.as_str()))
            .collect::<Option<Vec<_>>>()?;
        Some(self.format_type_arg_iter(resolved))
    }
}

fn generic_function(declared: Option<&Type>) -> Option<(&[EcoString], &FunctionType)> {
    let Type::Forall { vars, body } = declared? else {
        return None;
    };
    let Type::Function(f) = body.as_ref() else {
        return None;
    };
    Some((vars, f))
}

fn callee_curries_receiver(callee: &ResolvedCallee<'_>) -> bool {
    let Some(Type::Forall { body, .. }) = callee.declared_type() else {
        return false;
    };
    matches!(body.as_ref(), Type::Function(_))
        && callee.instantiated.as_function_type().is_some()
        && callee.receiver_offset > 0
}
