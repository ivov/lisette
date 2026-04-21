use crate::Emitter;
use crate::names::go_name;
use syntax::ast::{Expression, Generic, Pattern, Visibility};
use syntax::types::Type;

struct ImplContext<'a> {
    receiver_name: &'a str,
    ty: &'a Type,
    generics: &'a [Generic],
    qualified_type: String,
}

impl Emitter<'_> {
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
            qualified_type: format!("{}.{}", self.current_module, receiver_name),
        };

        methods
            .iter()
            .filter_map(|method| self.emit_impl_method(method, &ctx))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Emit one method of an `impl` block, producing either a receiver method
    /// (`func (r Recv) m(...)`) or a free function (`func Recv_m(r Recv, ...)`)
    /// depending on whether the method has `self` and whether it's UFCS-declared.
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
        if self.ctx.unused.is_unused_definition(name_span) {
            return None;
        }

        self.scope.reset_for_top_level();

        let function = method.to_function_definition();
        let is_public = matches!(visibility, Visibility::Public);

        let has_self = function.params.first().is_some_and(|p| {
            matches!(p.pattern, Pattern::Identifier { ref identifier, .. } if identifier == "self")
        });
        let is_ufcs = self
            .ctx
            .ufcs_methods
            .contains(&(ctx.qualified_type.clone(), function.name.to_string()));
        let should_export = is_public || self.method_needs_export(&function.name);
        let is_free_function = !has_self || is_ufcs;

        let code = if is_free_function {
            let mut free_function = function.clone();
            let method_name = if should_export {
                go_name::capitalize_first(&function.name)
            } else {
                function.name.to_string()
            };
            free_function.name = format!("{}_{}", ctx.receiver_name, method_name).into();
            let mut combined_generics = ctx.generics.to_vec();
            combined_generics.extend(free_function.generics.iter().cloned());
            free_function.generics = combined_generics;
            self.emit_function(&free_function, None, should_export)
        } else {
            self.emit_function(
                &function,
                Some((ctx.receiver_name.to_string(), ctx.ty.clone())),
                should_export,
            )
        };

        if code.is_empty() {
            return None;
        }
        let method_doc_comment = self.emit_doc(doc);
        Some(format!("{}{}", method_doc_comment, code))
    }
}
