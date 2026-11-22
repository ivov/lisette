use crate::EmitFacts;
use crate::Planner;
use crate::expressions::top_items::item_doc_text;
use crate::ir::declarations::GoDeclaration;
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, FunctionDefinitionView, Generic, Visibility};
use syntax::types::{Type, type_args_match_params};

type ReceiverGenerics<'a> = &'a [(EcoString, Vec<Type>)];

struct ImplContext<'a> {
    receiver_name: &'a str,
    ty: &'a Type,
    generics: &'a [Generic],
    receiver_generics: ReceiverGenerics<'a>,
    qualified_type: String,
}

impl Planner<'_> {
    pub(crate) fn emit_impl_block(
        &mut self,
        receiver_name: &str,
        ty: &Type,
        methods: &[Expression],
        generics: &[Generic],
        receiver_generics: &[(EcoString, Vec<Type>)],
    ) -> Vec<GoDeclaration> {
        let ctx = ImplContext {
            receiver_name,
            ty,
            generics,
            receiver_generics,
            qualified_type: self.facts.qualified_current(receiver_name),
        };

        methods
            .iter()
            .filter_map(|method| {
                self.with_declaration_scope(|this| this.try_emit_impl_method(method, &ctx))
            })
            .flatten()
            .collect()
    }

    fn try_emit_impl_method(
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

        let is_free_function = !self
            .facts
            .emits_go_method(&context.qualified_type, function.name);

        let (go_name, mut declarations) = if is_free_function {
            let free_name = go_name::associated_function_go_name(
                context.receiver_name,
                function.name,
                is_public,
            )
            .into();
            let (generics, owner) = self
                .facts
                .free_function_type_parameters(context, function.generics);
            let free_function = FunctionDefinitionView {
                name: &free_name,
                generics: &generics,
                ..function
            };
            let declarations = self.emit_function(free_function, None, &free_name, owner);
            (free_name.to_string(), declarations)
        } else {
            let go_name = self.pick_go_function_name(function, true, is_public);
            let declarations = self.emit_function(
                function,
                Some(context.ty),
                &go_name,
                Some(context.receiver_generics),
            );
            (go_name, declarations)
        };

        if declarations.is_empty() {
            return None;
        }
        let method_doc_comment = item_doc_text(doc, function.name, &go_name);
        declarations[0].documentation = method_doc_comment.into();
        Some(declarations)
    }
}

impl EmitFacts<'_> {
    fn free_function_type_parameters<'c>(
        &self,
        context: &ImplContext<'c>,
        function_generics: &[Generic],
    ) -> (Vec<Generic>, Option<ReceiverGenerics<'c>>) {
        if self.impl_generics_match_receiver(context) {
            return (function_generics.to_vec(), Some(context.receiver_generics));
        }
        let combined = context
            .generics
            .iter()
            .chain(function_generics)
            .cloned()
            .collect();
        (combined, None)
    }

    fn impl_generics_match_receiver(&self, ctx: &ImplContext<'_>) -> bool {
        self.definition(&ctx.qualified_type)
            .and_then(|definition| definition.body.generics())
            .is_some_and(|receiver_generics| receiver_generics.len() == ctx.generics.len())
            && type_args_match_params(
                ctx.ty.get_type_params().unwrap_or_default(),
                ctx.generics.iter().map(|generic| &generic.name),
            )
    }
}
