use syntax::ast::Expression;
use syntax::program::{Definition, DefinitionBody};

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(super) fn infer_struct_definition(&self, mut expression: Expression) -> Expression {
        let Expression::Struct {
            name,
            name_span,
            generics,
            fields,
            ..
        } = &mut expression
        else {
            unreachable!()
        };
        let qualified_name = self.qualify_name(name);
        if let Some(Definition {
            name_span: definition_name_span,
            body:
                DefinitionBody::Struct {
                    generics: definition_generics,
                    fields: definition_fields,
                    ..
                },
            ..
        }) = self.store.get_definition(&qualified_name)
        {
            *name_span = definition_name_span.expect("struct definition has a name span");
            *generics = definition_generics.clone();
            *fields = definition_fields.clone();
        }
        expression
    }

    pub(super) fn infer_type_alias_definition(&self, mut expression: Expression) -> Expression {
        let Expression::TypeAlias {
            name,
            generics,
            annotation,
            ty,
            ..
        } = &mut expression
        else {
            unreachable!()
        };
        let qualified_name = self.qualify_name(name);
        if let Some(Definition {
            ty: definition_ty,
            body:
                DefinitionBody::TypeAlias {
                    generics: definition_generics,
                    alias,
                    ..
                },
            ..
        }) = self.store.get_definition(&qualified_name)
        {
            *generics = definition_generics.clone();
            *annotation = alias.annotation().clone();
            *ty = definition_ty.clone();
        }
        expression
    }
}
