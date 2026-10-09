use crate::Planner;
use crate::expressions::top_items::emit_doc;
use syntax::ast::{Expression, FunctionDefinitionView, Generic, Pattern, Visibility};
use syntax::types::{Type, type_args_match_params};

struct ImplContext<'a> {
    receiver_name: &'a str,
    ty: &'a Type,
    generics: &'a [Generic],
    qualified_type: String,
}

impl Planner<'_> {
    pub(crate) fn emit_impl_block(
        &mut self,
        receiver_name: &str,
        ty: &Type,
        methods: &[Expression],
        generics: &[Generic],
    ) -> String {
        let ctx = ImplContext {
            receiver_name,
            ty,
            generics,
            qualified_type: self.facts.qualified_current(receiver_name),
        };

        methods
            .iter()
            .filter_map(|method| {
                self.with_declaration_scope(|this| this.emit_impl_method(method, &ctx))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Emit one impl method as a receiver method or a UFCS free function.
    fn emit_impl_method(&mut self, method: &Expression, ctx: &ImplContext<'_>) -> Option<String> {
        let Expression::Function {
            doc,
            visibility,
            name_span,
            ..
        } = method
        else {
            return None;
        };
        if self.facts.is_unused(name_span) {
            return None;
        }
        let function = method.function_definition_view();

        let is_public = matches!(visibility, Visibility::Public);

        let has_self = function.params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
        });
        let is_ufcs = self
            .facts
            .is_ufcs_method(&ctx.qualified_type, function.name);
        let should_export = is_public || self.method_needs_export(function.name);
        let is_free_function = !has_self || is_ufcs;

        let code = if is_free_function {
            let free_name = self
                .free_method_base_name(ctx.receiver_name, function.name, is_public)
                .into();
            let mut combined_generics = ctx.generics.to_vec();
            combined_generics.extend(function.generics.iter().cloned());
            let (generics, owner) = if self.impl_generics_match_receiver(ctx) {
                (function.generics, Some(ctx.ty))
            } else {
                (combined_generics.as_slice(), None)
            };
            let free_function = FunctionDefinitionView {
                name: &free_name,
                generics,
                ..function
            };
            self.emit_function(free_function, None, false, owner)
        } else {
            self.emit_function(function, Some(ctx.ty), should_export, None)
        };

        if code.is_empty() {
            return None;
        }
        let method_doc_comment = emit_doc(doc);
        Some(format!("{}{}", method_doc_comment, code))
    }

    /// Whether the impl declares exactly the receiver's generics, in order.
    fn impl_generics_match_receiver(&self, ctx: &ImplContext<'_>) -> bool {
        self.facts
            .definition(&ctx.qualified_type)
            .and_then(|definition| definition.body.generics())
            .is_some_and(|receiver_generics| receiver_generics.len() == ctx.generics.len())
            && type_args_match_params(
                ctx.ty.get_type_params().unwrap_or_default(),
                ctx.generics.iter().map(|generic| &generic.name),
            )
    }
}
