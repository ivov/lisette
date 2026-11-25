use crate::lowering::calls::predicates::Polarity;
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use crate::lowering::expressions::access::struct_call::plan_struct_literal;
use crate::lowering::expressions::operand_order::is_unmutated_identifier;
use crate::lowering::expressions::values::with_source_positions;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::go_expression::GoLiteral;

use super::{NativeCallContext, NativeCallForm};
use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::calls::call_plan::{CallPlan, CallableOrigin};
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::planner::written_span;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::Statement;
use crate::target::ir::bodies::discard;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::values::{CaptureBoundary, ConstantKind, GoExpression, Stability};
use crate::target::names::go_name;
use crate::target::names::go_name::{GeneratedPackage, GoExport};
use crate::target::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::program::{CallOperation, FieldSelection};
use syntax::program::{NativeOperation, strip_prelude_prefix};
use syntax::types::Type;

struct TupleStructTarget<'a> {
    go_ty: GoType,
    field_tys: Vec<Type>,
    selections: Vec<&'a FieldSelection>,
    owner_type: Type,
}

impl<'a> Planner<'a> {
    pub(in crate::lowering) fn try_plan_negated_call(
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
        if self.queries().is_local_binding(callee) {
            return None;
        }
        let CallOperation::Native { kind, method } = &self.facts.checked(callee).callee().operation
        else {
            return None;
        };
        let kind = *kind;
        let plan = self.queries().plan_call(CallSite::of(call_expression)?);
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
            assigned_place: None,
        };

        self.try_plan_negated_native_method(setup, &native_ctx)
    }

    pub(in crate::lowering) fn plan_call_expression(
        &mut self,
        site: CallSite<'_>,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let plan = self.queries().plan_call(site);
        self.plan_call_with_plan(plan, call_ty, ctx)
    }

    pub(in crate::lowering) fn plan_call_with_plan(
        &mut self,
        plan: CallPlan<'a, '_>,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let call_expression = plan.site.expression;
        let lowered = self.plan_unpositioned_call(plan, call_ty, ctx);
        with_source_positions(&self.facts, &written_span(call_expression), lowered)
    }

    fn plan_unpositioned_call(
        &mut self,
        plan: CallPlan<'a, '_>,
        call_ty: Option<&Type>,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let call_expression = plan.site.expression;
        let callee = plan.site.callee;
        let args = plan.site.args;
        let spread = plan.site.spread;
        let written_type_arguments = plan.site.written_type_arguments();

        if let Some(predicate) =
            self.try_plan_fused_predicate_value(call_expression, Polarity::Positive)
        {
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
                let value = match self
                    .queries()
                    .constant_needs_go_type(value.constant_kind(), &ty)
                {
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
                if let Some(function) = DotAccessSite::of(callee) {
                    return self.plan_ufcs_call(function, &plan);
                }
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
                    assigned_place: ctx.assigned_place(),
                };

                return self.plan_native_call(&native_ctx);
            }
            CallableOrigin::Source(CallOperation::ReceiverMethod { is_public }) => {
                let export = if *is_public {
                    GoExport::Exported
                } else {
                    GoExport::Unexported
                };
                let go_method = self
                    .queries()
                    .receiver_method_ufcs_go_name(callee, &plan, export);
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

        self.plan_regular_call(&plan, call_ty, ctx)
    }

    pub(super) fn plan_native_call(&mut self, context: &NativeCallContext) -> ValuePlan {
        if let Some(result) = self.try_plan_native_constructor(context) {
            return result;
        }
        let mut result = self.lower_native_method(context);
        let is_constructor = matches!(context.form, NativeCallForm::Constructor);
        let split = context.receiver_and_arguments();
        let reads_length = !is_constructor && matches!(context.method, "length" | "capacity");
        if reads_length
            && let Some((receiver, _)) = split
            && let Some(length) = self.facts.receiver_array_length(&receiver.get_type())
            && !is_constant_go_len(&result.value)
        {
            result.setup.push(discard(result.value));
            result.value = GoExpression::conversion(
                self.queries().go_type(&Type::int()),
                GoExpression::constant(GoLiteral::Int(i128::from(length)), ConstantKind::Int),
            );
        }
        let reads_fixed_length = reads_length
            && !matches!(
                context.native_type,
                NativeTypeKind::Channel | NativeTypeKind::Sender | NativeTypeKind::Receiver
            )
            && split.is_some_and(|(receiver, _)| {
                is_unmutated_identifier(&self.facts, receiver)
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
        let target = self
            .queries()
            .resolve_tuple_struct_target(call_expression, call_ty)?;

        let arg_ctx = match (ctx.assigned_place(), args.len()) {
            (Some(assigned), 1) => ExpressionContext::value().with_assigned_place(assigned),
            _ => ExpressionContext::value(),
        };
        let field_layouts: Vec<_> = target
            .field_tys
            .iter()
            .zip(&target.selections)
            .enumerate()
            .map(|(index, (ty, selection))| {
                self.facts.field_slot_layout(
                    &target.owner_type,
                    Some(selection),
                    &index.to_string(),
                    ty,
                )
            })
            .collect();

        let (operands, source_layouts): (Vec<ValuePlan>, Vec<ValueLayout>) = args
            .iter()
            .zip(&target.field_tys)
            .map(|(a, slot)| {
                let origin = self.facts.function_type_origin(slot, SlotOrigin::Lisette);
                let ctx = arg_ctx.with_function_slot_origin(origin);
                let source = self.queries().emitted_value_layout(a, ctx);
                (self.plan_composite_value(a, ctx), source)
            })
            .unzip();

        let sequenced = self.sequence_values(operands, CaptureBoundary::SiblingSequence, "arg");
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

        let function_layouts = self.queries().any_slot_function_layouts(&asserted);
        let target_ty = match &function_layouts {
            Some((stored, _)) => stored.go_type(&self.queries()),
            None => self.queries().go_type(&asserted),
        };
        let (mut setup, arguments) = match args.first() {
            Some(a) => {
                let plan = self.plan_composite_value(a, ExpressionContext::value());
                let (setup, value) = self.convert_to_recorded_slot(a, plan).into_parts();
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
        let change = CoercionPlan::change(
            &self.queries(),
            &option_layout(stored),
            &option_layout(lisette),
        );
        let (layout_change_setup, value) = change.lower(self, call);
        setup.extend(layout_change_setup);
        (setup, value)
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

pub(super) fn callee_go_name<'f>(
    facts: &'f EmitFacts<'_>,
    function: &Expression,
) -> Option<&'f str> {
    let symbol = facts.checked(function).callee().definition.as_ref()?;
    facts.definition(symbol)?.go_name()
}

pub(super) fn callee_is_go_builtin<'a>(facts: &EmitFacts<'a>, callee: &Expression) -> bool {
    go_builtin_name(facts, callee).is_some()
}

pub(super) fn go_builtin_name<'a>(facts: &EmitFacts<'a>, callee: &Expression) -> Option<&'a str> {
    facts
        .checked(callee)
        .callee()
        .definition
        .as_deref()
        .filter(|qualified| go_name::is_prelude_go_builtin(qualified))
        .and_then(strip_prelude_prefix)
}

fn is_constant_go_len(value: &GoExpression) -> bool {
    matches!(
        value.node(),
        GoExpressionNode::Call { arguments, .. }
            if matches!(arguments.as_slice(), [argument] if !argument.has_call())
    )
}

impl<'a> PlannerQueries<'_, 'a> {
    fn receiver_method_ufcs_go_name(
        &self,
        callee: &Expression,
        plan: &CallPlan<'_, '_>,
        export: GoExport,
    ) -> String {
        let method = extract_receiver_ufcs_method(callee);
        let receiver = plan
            .resolved
            .instantiated
            .as_function_type()
            .and_then(|f| f.params.first())
            .map_or(&Type::Never, |receiver| &receiver.ty);

        self.facts
            .receiver_method_go_name(receiver, &method, export, self.member_spelling(callee))
    }

    fn resolve_tuple_struct_target(
        &self,
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
}
