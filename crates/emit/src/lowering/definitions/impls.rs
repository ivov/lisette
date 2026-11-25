use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::definitions::top_items::item_doc_text;
use crate::target::ir::declarations::GoDeclaration;
use crate::target::names::go_name::{self, GoExport};
use syntax::EcoString;
use syntax::ast::{Expression, FunctionDefinitionView, Generic};
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
    pub(in crate::lowering) fn emit_impl_block(
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

        let export = GoExport::from_visibility(*visibility);

        let is_free_function = !self
            .facts
            .emits_go_method(&context.qualified_type, function.name);

        let (go_name, mut declarations) = if is_free_function {
            let free_name =
                go_name::associated_function_go_name(context.receiver_name, function.name, export)
                    .into();
            let (generics, owner) =
                free_function_type_parameters(&self.facts, context, function.generics);
            let free_function = FunctionDefinitionView {
                name: &free_name,
                generics: &generics,
                ..function
            };
            let declarations = self.emit_function(free_function, None, &free_name, owner);
            (free_name.to_string(), declarations)
        } else {
            let go_name = self.facts.method_go_name(function.name, export);
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

fn free_function_type_parameters<'c>(
    facts: &EmitFacts<'_>,
    context: &ImplContext<'c>,
    function_generics: &[Generic],
) -> (Vec<Generic>, Option<ReceiverGenerics<'c>>) {
    if impl_generics_match_receiver(facts, context) {
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

fn impl_generics_match_receiver(facts: &EmitFacts<'_>, ctx: &ImplContext<'_>) -> bool {
    facts
        .definition(&ctx.qualified_type)
        .and_then(|definition| definition.body.generics())
        .is_some_and(|receiver_generics| receiver_generics.len() == ctx.generics.len())
        && type_args_match_params(
            ctx.ty.get_type_params().unwrap_or_default(),
            ctx.generics.iter().map(|generic| &generic.name),
        )
}
