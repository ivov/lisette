use crate::Planner;
use crate::expressions::top_items::emit_doc;
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, FunctionDefinitionView, Generic, Pattern, Visibility};
use syntax::program::DefinitionBody;
use syntax::types::{Type, build_substitution_map, substitute, type_args_match_params};

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
            .filter_map(|method| self.emit_impl_method(method, &ctx))
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
        if self.facts.is_unused_definition(name_span) {
            return None;
        }
        let function = method.function_definition_view();

        self.scope.reset_for_top_level();

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
            let method_name = if should_export {
                go_name::snake_to_camel(function.name)
            } else {
                function.name.to_string()
            };
            let free_name = format!("{}_{}", ctx.receiver_name, method_name).into();
            let mut combined_generics = ctx.generics.to_vec();
            combined_generics.extend(function.generics.iter().cloned());
            let generic_bounds = self.free_function_generic_bounds(ctx, function.generics);
            let free_function = FunctionDefinitionView {
                name: &free_name,
                generics: &combined_generics,
                ..function
            };
            self.emit_function(free_function, None, false, generic_bounds.as_deref())
        } else {
            self.emit_function(
                function,
                Some((ctx.receiver_name.to_string(), ctx.ty.clone())),
                should_export,
                None,
            )
        };

        if code.is_empty() {
            return None;
        }
        let method_doc_comment = emit_doc(doc);
        Some(format!("{}{}", method_doc_comment, code))
    }

    fn free_function_generic_bounds(
        &self,
        ctx: &ImplContext<'_>,
        method_generics: &[Generic],
    ) -> Option<Vec<(EcoString, Vec<Type>)>> {
        let receiver_generics = self.facts.definition(&ctx.qualified_type).and_then(
            |definition| match &definition.body {
                DefinitionBody::Struct { generics, .. } | DefinitionBody::Enum { generics, .. } => {
                    Some(generics)
                }
                _ => None,
            },
        )?;
        if receiver_generics.len() != ctx.generics.len()
            || !type_args_match_params(
                ctx.ty.get_type_params().unwrap_or_default(),
                ctx.generics.iter().map(|generic| &generic.name),
            )
        {
            return None;
        }

        let substitution = build_substitution_map(
            receiver_generics,
            ctx.ty.get_type_params().unwrap_or_default(),
        );
        let mut generic_bounds = receiver_generics
            .iter()
            .zip(ctx.generics)
            .map(|(receiver_generic, impl_generic)| {
                let bounds = receiver_generic
                    .bounds
                    .iter()
                    .map(|bound| {
                        let resolved = self
                            .facts
                            .resolved_bound_type(bound.get_span())
                            .expect("checker records every receiver bound");
                        substitute(resolved, &substitution)
                    })
                    .collect();
                (impl_generic.name.clone(), bounds)
            })
            .collect::<Vec<_>>();
        generic_bounds.extend(method_generics.iter().map(|generic| {
            let bounds = generic
                .bounds
                .iter()
                .map(|bound| {
                    self.facts
                        .resolved_bound_type(bound.get_span())
                        .cloned()
                        .expect("checker records every method bound")
                })
                .collect();
            (generic.name.clone(), bounds)
        }));
        Some(generic_bounds)
    }
}
