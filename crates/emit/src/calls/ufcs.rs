use crate::write_line;
use rustc_hash::FxHashMap as HashMap;

use crate::Emitter;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use crate::utils::Staged;
use syntax::ast::{Annotation, Expression};
use syntax::program::ReceiverCoercion;
use syntax::types::Type;

impl Emitter<'_> {
    fn infer_ufcs_return_only_type_args(
        &mut self,
        function: &Expression,
        qualified_name: &str,
        member: &str,
        receiver_ty: &Type,
    ) -> Option<String> {
        let method_key = format!("{}.{}", qualified_name, member);
        let definition_ty = self.ctx.definitions.get(method_key.as_str())?.ty().clone();

        let Type::Forall { vars, body } = &definition_ty else {
            return None;
        };
        let Type::Function {
            params: generic_params,
            ..
        } = body.as_ref()
        else {
            return None;
        };

        let all_inferable = vars.iter().all(|var| {
            let param_ty = Type::Parameter(var.clone());
            generic_params.iter().any(|pt| pt.contains_type(&param_ty))
        });
        if all_inferable {
            return None;
        }

        let instantiated_ty = function.get_type();
        let mut mapping: HashMap<String, Type> = HashMap::default();
        extract_type_mapping(body, &instantiated_ty, &mut mapping);

        let mut go_type_strs = Vec::new();
        if let Type::Nominal { params, .. } = receiver_ty {
            for param in params {
                go_type_strs.push(self.go_type_as_string(param));
            }
        }
        let base_generics_count = if let Type::Nominal { params, .. } = receiver_ty {
            params.len()
        } else {
            0
        };
        for var in vars.iter().skip(base_generics_count) {
            if let Some(resolved) = mapping.get(var.as_str()) {
                go_type_strs.push(self.go_type_as_string(resolved));
            } else {
                return None;
            }
        }

        if go_type_strs.is_empty() {
            return None;
        }

        Some(format!("[{}]", go_type_strs.join(", ")))
    }

    pub(super) fn emit_ufcs_call(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        type_args: &[Annotation],
        spread: Option<&Expression>,
    ) -> String {
        let Expression::DotAccess {
            expression: receiver,
            member,
            receiver_coercion: coercion,
            ..
        } = function
        else {
            unreachable!("emit_ufcs_call called on non-DotAccess");
        };

        let receiver_ty = receiver.get_type().strip_refs().clone();
        let Type::Nominal {
            id: qualified_name, ..
        } = &receiver_ty
        else {
            unreachable!("UFCS receiver must be a constructor type");
        };

        let coercion = *coercion;

        // Stage receiver + args together for eval-order sequencing
        let mut all_stages: Vec<Staged> =
            Vec::with_capacity(1 + args.len() + spread.is_some() as usize);
        all_stages.push(self.stage_operand(receiver));
        for arg in args {
            all_stages.push(self.stage_composite(arg));
        }
        let all_values = self.sequence_with_spread(output, all_stages, spread, false, "_arg");
        let receiver_arg = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();

        let receiver_arg = match coercion {
            Some(ReceiverCoercion::AutoAddress) => {
                if matches!(receiver.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, receiver_arg);
                    format!("&{}", tmp)
                } else {
                    format!("&{}", receiver_arg)
                }
            }
            Some(ReceiverCoercion::AutoDeref) => format!("*{}", receiver_arg),
            None => receiver_arg,
        };

        if let Some(native_type) = NativeGoType::from_type(&receiver.get_type())
            && let Some((inlined, extra_import)) = super::native::try_inline_native_method(
                &native_type,
                member,
                &receiver_arg,
                &emitted_args,
            )
        {
            self.apply_inline_import(extra_import);
            return inlined;
        }

        let mut new_args = vec![receiver_arg];
        new_args.extend(emitted_args);

        let type_args_string = if !type_args.is_empty() {
            self.format_type_args_with_receiver(&receiver_ty, type_args)
        } else {
            self.infer_ufcs_return_only_type_args(function, qualified_name, member, &receiver_ty)
                .unwrap_or_default()
        };

        let method_key = format!("{}.{}", qualified_name, member);
        let is_public = self
            .ctx
            .definitions
            .get(method_key.as_str())
            .map(|d| d.visibility().is_public())
            .unwrap_or(false)
            || self.method_needs_export(member);

        let qualified_method_name = self.qualify_method_call(qualified_name, member, is_public);
        let fn_name = format!("{}{}", qualified_method_name, type_args_string);

        format!("{}({})", fn_name, new_args.join(", "))
    }

    pub(super) fn emit_receiver_method_ufcs(
        &mut self,
        output: &mut String,
        args: &[Expression],
        type_args: &[Annotation],
        method: &str,
        is_public: bool,
        spread: Option<&Expression>,
    ) -> String {
        let go_method = if is_public {
            let mut chars = method.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => method.to_string(),
            }
        } else {
            go_name::escape_keyword(method).into_owned()
        };

        let stages: Vec<Staged> = args.iter().map(|a| self.stage_composite(a)).collect();
        let emitted_all = self.sequence_with_spread(output, stages, spread, false, "_arg");
        let receiver = emitted_all[0].clone();
        let emitted_rest: Vec<String> = emitted_all[1..].to_vec();

        let type_args_string = self.format_type_args_from_annotations(type_args);

        let receiver = if let Some(stripped) = receiver.strip_prefix('&') {
            stripped.to_string()
        } else if receiver.starts_with('*') {
            format!("({})", receiver)
        } else {
            receiver
        };

        format!(
            "{}.{}{}({})",
            receiver,
            go_method,
            type_args_string,
            emitted_rest.join(", ")
        )
    }
}
