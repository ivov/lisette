use crate::Planner;
use crate::expressions::top_items::emit_item_doc;
use crate::names::go_name;
use crate::plan::declarations::GoDeclaration;
use syntax::ast::{Expression, FunctionDefinitionView, Generic, Visibility};
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
    ) -> Vec<GoDeclaration> {
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
            .flatten()
            .collect()
    }

    /// Emit one impl method as a receiver method or a UFCS free function.
    fn emit_impl_method(
        &mut self,
        method: &Expression,
        context: &ImplContext<'_>,
    ) -> Option<Vec<GoDeclaration>> {
        let Expression::Function {
            doc, visibility, ..
        } = method
        else {
            return None;
        };
        let function = method.function_definition_view();

        let is_public = matches!(visibility, Visibility::Public);

        let has_self = self
            .facts
            .method(&context.qualified_type, function.name)
            .is_some();
        let is_ufcs = self
            .facts
            .is_ufcs_method(&context.qualified_type, function.name);
        let should_export = is_public || self.method_needs_export(function.name);
        let is_free_function = !has_self || is_ufcs;

        let (go_name, mut declarations) = if is_free_function {
            let free_name = go_name::associated_function_go_name(
                context.receiver_name,
                function.name,
                is_public,
            )
            .into();
            let mut combined_generics = context.generics.to_vec();
            combined_generics.extend(function.generics.iter().cloned());
            let (generics, owner) = if self.impl_generics_match_receiver(context) {
                (function.generics, Some(context.ty))
            } else {
                (combined_generics.as_slice(), None)
            };
            let free_function = FunctionDefinitionView {
                name: &free_name,
                generics,
                ..function
            };
            let declarations = self.emit_function(free_function, None, &free_name, owner);
            (free_name.to_string(), declarations)
        } else {
            let go_name = self.pick_go_function_name(function, true, should_export);
            let declarations = self.emit_function(function, Some(context.ty), &go_name, None);
            (go_name, declarations)
        };

        if declarations.is_empty() {
            return None;
        }
        let method_doc_comment = emit_item_doc(doc, function.name, &go_name);
        declarations[0].documentation = method_doc_comment.into();
        Some(declarations)
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
