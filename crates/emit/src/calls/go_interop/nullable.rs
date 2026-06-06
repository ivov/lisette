use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::calls::go_interop::build_tuple_literal;
use crate::calls::go_interop::wrappers::{
    TupleReturnLayout, WrapperOutcome, WrapperTarget, leaf_block,
};
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::{Fallible, FalliblePlanner, OPTION_SOME_TAG};
use crate::plan::bodies::{ElseArm, IfPlan, LoopPlan, LoweredBlock, LoweredStatement};
use crate::types::shape::{CollectionKind, NullableCollectionElement, NullableCollectionShape};
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(super) fn lower_go_option_call_wrapped(
        &mut self,
        call_expression: &Expression,
        option_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);
        let (wrap_setup, outcome) = self.lower_comma_ok_wrapping(
            &call_str,
            option_ty,
            TupleReturnLayout::Flattened,
            WrapperTarget::FreshSlot,
            fx,
        );
        setup.extend(wrap_setup);
        (setup, outcome.expect("wrapper produced no slot"))
    }

    pub(super) fn lower_go_sentinel_call_wrapped(
        &mut self,
        call_expression: &Expression,
        option_ty: &Type,
        sentinel: i64,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);
        let (wrap_setup, outcome) = self.lower_sentinel_wrapping(
            &call_str,
            option_ty,
            sentinel,
            WrapperTarget::FreshSlot,
            fx,
        );
        setup.extend(wrap_setup);
        (setup, outcome.expect("wrapper produced no slot"))
    }

    pub(crate) fn emit_sentinel_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        option_ty: &Type,
        sentinel: i64,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> WrapperOutcome {
        let (statements, outcome) =
            self.lower_sentinel_wrapping(call_str, option_ty, sentinel, target, fx);
        output.push_str(&Renderer.render_setup(&statements));
        outcome
    }

    /// Wrap a sentinel-call via `OptionFromCommaOk` with `raw != sentinel`.
    pub(crate) fn lower_sentinel_wrapping(
        &mut self,
        call_str: &str,
        option_ty: &Type,
        sentinel: i64,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        fx.require_stdlib();
        let mut statements = Vec::new();
        let raw = self.hoist_tmp_value_statement(&mut statements, "ret", call_str);
        let inner_ty_str = self.go_type_string(&option_ty.ok_type(), fx);
        let value_expr = format!(
            "lisette.OptionFromCommaOk[{}]({}, {} != {})",
            inner_ty_str, raw, raw, sentinel
        );
        let outcome =
            self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
        (statements, outcome)
    }

    pub(crate) fn emit_comma_ok_wrapping(
        &mut self,
        output: &mut String,
        call_str: &str,
        option_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> WrapperOutcome {
        let (statements, outcome) =
            self.lower_comma_ok_wrapping(call_str, option_ty, layout, target, fx);
        output.push_str(&Renderer.render_setup(&statements));
        outcome
    }

    /// Wrap a comma-ok-returning call into a tagged `Option`. A `Flattened`
    /// tuple inner type comes from a Go-imported `(T1, ..., Tn, bool)`; a
    /// `Packed` one from a Lisette `(Tuple_n[...], bool)`.
    pub(crate) fn lower_comma_ok_wrapping(
        &mut self,
        call_str: &str,
        option_ty: &Type,
        layout: TupleReturnLayout,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        fx.require_stdlib();
        let mut statements = Vec::new();

        let inner_ty = option_ty.ok_type();
        let inner_tuple_arity = inner_ty.tuple_arity();
        let needs_nilable_validation = self.facts.is_nullable_option(option_ty);

        let needs_complex =
            needs_nilable_validation || (layout.is_flattened() && inner_tuple_arity.is_some());

        if !needs_complex {
            let inner_ty_str = self.go_type_string(&inner_ty, fx);
            let value_expr = format!("lisette.OptionFromCommaOk[{}]({})", inner_ty_str, call_str);
            let outcome =
                self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
            return (statements, outcome);
        }

        let fallible = Fallible::from_type(option_ty).expect("Option type expected");

        let val_vars = if layout.is_flattened()
            && let Some(arity) = inner_tuple_arity
        {
            self.create_temp_vars("ret", arity)
        } else {
            self.create_temp_vars("ret", 1)
        };
        let ok_var = self.fresh_var(Some("ret"));
        self.declare(&ok_var);

        let all_vars: Vec<&str> = val_vars
            .iter()
            .map(|s| s.as_str())
            .chain(std::iter::once(ok_var.as_str()))
            .collect();
        statements.push(LoweredStatement::RawGo(format!(
            "{} := {}\n",
            all_vars.join(", "),
            call_str
        )));

        let val_expression = if layout.is_flattened() && inner_tuple_arity.is_some() {
            build_tuple_literal(&val_vars, &inner_ty, fx)
        } else {
            val_vars[0].clone()
        };

        let option_ty_str = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.full_type_string()
        };

        let condition = if self.is_interface_option(option_ty) {
            format!("{} && !lisette.IsNilInterface({})", ok_var, val_vars[0])
        } else if needs_nilable_validation {
            format!("{} && {} != nil", ok_var, val_vars[0])
        } else {
            ok_var.clone()
        };

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &option_ty_str, "option");

        let some_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.emit_success(&val_expression)
        };
        let none_wrapper = {
            let mut fe = FalliblePlanner::new(self, &fallible, fx);
            fe.emit_failure(None)
        };

        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition,
            then_body: leaf_block(&sink, &some_wrapper),
            else_arm: ElseArm::Else {
                body: leaf_block(&sink, &none_wrapper),
                inline: false,
            },
        }));
        (statements, outcome)
    }

    pub(crate) fn emit_nil_check_option_wrap(
        &mut self,
        output: &mut String,
        raw_value: &str,
        option_ty: &Type,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> WrapperOutcome {
        let (statements, outcome) =
            self.lower_nil_check_option_wrap(raw_value, option_ty, target, fx);
        output.push_str(&Renderer.render_setup(&statements));
        outcome
    }

    /// Wrap a nilable Go value into a tagged `Option` via `OptionFromNilable`.
    pub(crate) fn lower_nil_check_option_wrap(
        &mut self,
        raw_value: &str,
        option_ty: &Type,
        target: WrapperTarget<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, WrapperOutcome) {
        fx.require_stdlib();
        let mut statements = Vec::new();
        let inner_ty = option_ty.ok_type();
        let inner_ty_str = self.go_type_string(&inner_ty, fx);
        let is_nil_check = if self.is_interface_option(option_ty) {
            format!("lisette.IsNilInterface({})", raw_value)
        } else {
            format!("{} == nil", raw_value)
        };
        let value_expr = format!(
            "lisette.OptionFromNilable[{}]({}, {})",
            inner_ty_str, raw_value, is_nil_check
        );
        let outcome =
            self.push_simple_wrapper_value(&mut statements, target, "option", &value_expr);
        (statements, outcome)
    }

    pub(super) fn lower_go_single_return_option_wrapped(
        &mut self,
        call_expression: &Expression,
        option_ty: &Type,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let (mut setup, call_str) =
            self.lower_call(call_expression, None, ExpressionContext::value(), fx);
        let raw_var = self.hoist_tmp_value_statement(&mut setup, "raw", &call_str);
        let (wrap_setup, outcome) =
            self.lower_nil_check_option_wrap(&raw_var, option_ty, WrapperTarget::FreshSlot, fx);
        setup.extend(wrap_setup);
        (setup, outcome.expect("wrapper produced no slot"))
    }

    pub(crate) fn emit_option_projection(
        &mut self,
        output: &mut String,
        option_value: &str,
        slot_hint: &str,
        slot_ty: &str,
        address: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let mut statements = Vec::new();
        let slot_var = self.plan_option_projection(
            &mut statements,
            option_value,
            slot_hint,
            slot_ty,
            address,
            fx,
        );
        output.push_str(&Renderer.render_setup(&statements));
        slot_var
    }

    pub(crate) fn plan_option_projection(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        option_value: &str,
        slot_hint: &str,
        slot_ty: &str,
        address: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let opt_var = self.hoist_tmp_value_statement(statements, "opt", option_value);
        let slot_var = self.fresh_var(Some(slot_hint));
        self.declare(&slot_var);
        statements.push(LoweredStatement::RawGo(format!(
            "var {} {}\n",
            slot_var, slot_ty
        )));

        fx.require_stdlib();
        let amp = if address { "&" } else { "" };
        let body = LoweredBlock {
            statements: vec![LoweredStatement::RawGo(format!(
                "{} = {}{}.SomeVal\n",
                slot_var, amp, opt_var
            ))],
        };
        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: format!("{}.Tag == {}", opt_var, OPTION_SOME_TAG),
            then_body: body,
            else_arm: ElseArm::None,
        }));
        slot_var
    }

    /// Wrap a Go `*T` (T value-typed) into Lisette `Option<T>`.
    pub(crate) fn plan_pointer_to_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        ptr_value: &str,
        option_ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        fx.require_stdlib();
        let inner_ty_str = self.go_type_string(&option_ty.ok_type(), fx);
        let value_expr = format!("lisette.OptionFromPointer[{}]({})", inner_ty_str, ptr_value);
        self.hoist_tmp_value_statement(statements, "option", &value_expr)
    }

    /// `FreshSlot` form of `lower_nil_check_option_wrap` (extends `statements`
    /// and returns the option var).
    pub(crate) fn plan_nil_check_option_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: &str,
        option_ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let (wrap_statements, outcome) =
            self.lower_nil_check_option_wrap(raw_value, option_ty, WrapperTarget::FreshSlot, fx);
        statements.extend(wrap_statements);
        outcome.expect("FreshSlot produces a slot")
    }

    pub(crate) fn plan_collection_nullable_wrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        raw_value: &str,
        collection_ty: &Type,
        shape: &NullableCollectionShape,
        fx: &mut EmitEffects,
    ) -> String {
        fx.require_stdlib();

        let lisette_collection_ty = self.go_type_string(collection_ty, fx);
        let src_var = self.hoist_tmp_value_statement(statements, "src", raw_value);
        let wrapped_var = self.hoist_tmp_value_statement(
            statements,
            "wrapped",
            &format!("make({}, len({}))", lisette_collection_ty, src_var),
        );
        let index_var = self.fresh_var(Some("i"));
        self.declare(&index_var);
        let val_var = self.fresh_var(Some("v"));
        self.declare(&val_var);

        let loop_body = match &shape.element {
            NullableCollectionElement::Option(opt_ty) => {
                let fallible = Fallible::from_type(opt_ty).expect("Option type expected");
                let is_pointer_bridged = self.is_non_nilable_option(opt_ty);
                let is_interface = self.is_interface_option(opt_ty);
                let some_input = if is_pointer_bridged {
                    format!("*{}", val_var)
                } else {
                    val_var.clone()
                };
                let some_wrapper = {
                    let mut fe = FalliblePlanner::new(self, &fallible, fx);
                    fe.emit_success(&some_input)
                };
                let none_wrapper = {
                    let mut fe = FalliblePlanner::new(self, &fallible, fx);
                    fe.emit_failure(None)
                };
                let condition = if is_interface {
                    format!("!lisette.IsNilInterface({})", val_var)
                } else {
                    format!("{} != nil", val_var)
                };
                let then_body = LoweredBlock {
                    statements: vec![LoweredStatement::RawGo(format!(
                        "{}[{}] = {}\n",
                        wrapped_var, index_var, some_wrapper
                    ))],
                };
                let else_body = LoweredBlock {
                    statements: vec![LoweredStatement::RawGo(format!(
                        "{}[{}] = {}\n",
                        wrapped_var, index_var, none_wrapper
                    ))],
                };
                let if_plan = IfPlan {
                    directive: String::new(),
                    condition_setup: String::new(),
                    condition,
                    then_body,
                    else_arm: ElseArm::Else {
                        body: else_body,
                        inline: false,
                    },
                };
                LoweredBlock {
                    statements: vec![LoweredStatement::If(if_plan)],
                }
            }
            NullableCollectionElement::Nested(inner_shape) => {
                let inner_lisette_ty = self.collection_element_ty(collection_ty, shape);
                let mut inner_statements = Vec::new();
                let inner_var = self.plan_collection_nullable_wrap(
                    &mut inner_statements,
                    &val_var,
                    &inner_lisette_ty,
                    inner_shape,
                    fx,
                );
                inner_statements.push(LoweredStatement::RawGo(format!(
                    "{}[{}] = {}\n",
                    wrapped_var, index_var, inner_var
                )));
                LoweredBlock {
                    statements: inner_statements,
                }
            }
        };

        statements.push(LoweredStatement::Loop(LoopPlan {
            directive: String::new(),
            prologue: String::new(),
            label: None,
            header: format!("for {}, {} := range {} {{\n", index_var, val_var, src_var),
            body: loop_body,
        }));

        wrapped_var
    }

    pub(crate) fn plan_collection_nullable_unwrap(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        lisette_value: &str,
        collection_ty: &Type,
        shape: &NullableCollectionShape,
        fx: &mut EmitEffects,
    ) -> String {
        fx.require_stdlib();
        let raw_collection_ty = self.shape_raw_collection_ty(shape, fx);

        let src_var = self.hoist_tmp_value_statement(statements, "src", lisette_value);
        let unwrapped_var = self.hoist_tmp_value_statement(
            statements,
            "unwrapped",
            &format!("make({}, len({}))", raw_collection_ty, src_var),
        );
        let index_var = self.fresh_var(Some("i"));
        self.declare(&index_var);
        let val_var = self.fresh_var(Some("v"));
        self.declare(&val_var);

        let loop_body = match &shape.element {
            NullableCollectionElement::Option(opt_ty) => {
                let is_pointer_bridged = self.is_non_nilable_option(opt_ty);
                let is_map = matches!(shape.kind, CollectionKind::Map);
                let emit_nil_else = is_map || is_pointer_bridged;
                let some_assignment = if is_pointer_bridged {
                    format!("&{}.SomeVal", val_var)
                } else {
                    format!("{}.SomeVal", val_var)
                };
                let then_body = LoweredBlock {
                    statements: vec![LoweredStatement::RawGo(format!(
                        "{}[{}] = {}\n",
                        unwrapped_var, index_var, some_assignment
                    ))],
                };
                let else_arm = if emit_nil_else {
                    ElseArm::Else {
                        body: LoweredBlock {
                            statements: vec![LoweredStatement::RawGo(format!(
                                "{}[{}] = nil\n",
                                unwrapped_var, index_var
                            ))],
                        },
                        inline: false,
                    }
                } else {
                    ElseArm::None
                };
                let if_plan = IfPlan {
                    directive: String::new(),
                    condition_setup: String::new(),
                    condition: format!("{}.Tag == {}", val_var, OPTION_SOME_TAG),
                    then_body,
                    else_arm,
                };
                LoweredBlock {
                    statements: vec![LoweredStatement::If(if_plan)],
                }
            }
            NullableCollectionElement::Nested(inner_shape) => {
                let inner_lisette_ty = self.collection_element_ty(collection_ty, shape);
                let mut inner_statements = Vec::new();
                let inner_var = self.plan_collection_nullable_unwrap(
                    &mut inner_statements,
                    &val_var,
                    &inner_lisette_ty,
                    inner_shape,
                    fx,
                );
                inner_statements.push(LoweredStatement::RawGo(format!(
                    "{}[{}] = {}\n",
                    unwrapped_var, index_var, inner_var
                )));
                LoweredBlock {
                    statements: inner_statements,
                }
            }
        };

        statements.push(LoweredStatement::Loop(LoopPlan {
            directive: String::new(),
            prologue: String::new(),
            label: None,
            header: format!("for {}, {} := range {} {{\n", index_var, val_var, src_var),
            body: loop_body,
        }));

        unwrapped_var
    }

    /// Lisette element type of a collection: `Slice<X>` to `X`, `Map<K, V>` to `V`.
    fn collection_element_ty(&self, collection_ty: &Type, shape: &NullableCollectionShape) -> Type {
        let resolved = self.emit_shape_ty(collection_ty);
        let params = resolved
            .get_type_params()
            .expect("native collection has type params");
        let index = if matches!(shape.kind, CollectionKind::Map) {
            1
        } else {
            0
        };
        params[index].clone()
    }

    /// Raw Go type for an unwrapped collection; walks the shape recursively so
    /// nested options lower past the outer `[]` (e.g. to `[][]*T`).
    fn shape_raw_collection_ty(
        &mut self,
        shape: &NullableCollectionShape,
        fx: &mut EmitEffects,
    ) -> String {
        let raw_element_ty = match &shape.element {
            NullableCollectionElement::Option(opt_ty) => {
                let is_pointer_bridged = self.is_non_nilable_option(opt_ty);
                let inner_ty = opt_ty.ok_type();
                let inner_ty_str = self.go_type_string(&inner_ty, fx);
                if is_pointer_bridged {
                    format!("*{}", inner_ty_str)
                } else {
                    inner_ty_str
                }
            }
            NullableCollectionElement::Nested(inner_shape) => {
                self.shape_raw_collection_ty(inner_shape, fx)
            }
        };
        if let Some(key_ty) = shape.key_ty.as_ref() {
            let key_ty_str = self.go_type_string(key_ty, fx);
            format!("map[{}]{}", key_ty_str, raw_element_ty)
        } else {
            format!("[]{}", raw_element_ty)
        }
    }
}
