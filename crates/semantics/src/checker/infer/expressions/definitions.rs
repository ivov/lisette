use syntax::ast::Expression;
use syntax::program::{Definition, DefinitionBody};

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(super) fn infer_struct_definition(&mut self, expression: Expression) -> Expression {
        let store = self.store;
        let Expression::Struct {
            doc,
            attributes,
            name,
            name_span,
            generics,
            fields,
            visibility,
            span,
        } = expression
        else {
            unreachable!()
        };

        let qualified_name = self.qualify_name(&name);
        if let Some(Definition {
            name: definition_name,
            name_span: definition_name_span,
            body:
                DefinitionBody::Struct {
                    generics: definition_generics,
                    fields: definition_fields,
                    ..
                },
            ..
        }) = store.get_definition(&qualified_name)
        {
            let definition_name = definition_name
                .clone()
                .expect("struct definition has a name");
            let definition_name_span =
                definition_name_span.expect("struct definition has a name span");
            let definition_generics = definition_generics.clone();
            let definition_fields = definition_fields.clone();

            Expression::Struct {
                doc,
                attributes,
                name: definition_name,
                name_span: definition_name_span,
                generics: definition_generics,
                fields: definition_fields,
                visibility,
                span,
            }
        } else {
            Expression::Struct {
                doc,
                attributes,
                name,
                name_span,
                generics,
                fields,
                visibility,
                span,
            }
        }
    }

    pub(super) fn infer_type_alias_definition(&mut self, expression: Expression) -> Expression {
        let store = self.store;
        let Expression::TypeAlias {
            doc,
            attributes,
            name,
            name_span,
            generics,
            annotation,
            ty,
            visibility,
            span,
        } = expression
        else {
            unreachable!()
        };

        let qualified_name = self.qualify_name(&name);
        if let Some(Definition {
            name: alias_name,
            ty: definition_ty,
            body:
                DefinitionBody::TypeAlias {
                    generics: definition_generics,
                    alias,
                    ..
                },
            ..
        }) = store.get_definition(&qualified_name)
        {
            Expression::TypeAlias {
                doc,
                attributes,
                name: alias_name
                    .clone()
                    .expect("type alias definition has a name"),
                name_span,
                generics: definition_generics.clone(),
                annotation: alias.annotation().clone(),
                ty: definition_ty.clone(),
                visibility,
                span,
            }
        } else {
            Expression::TypeAlias {
                doc,
                attributes,
                name,
                name_span,
                generics,
                annotation,
                ty,
                visibility,
                span,
            }
        }
    }
}
