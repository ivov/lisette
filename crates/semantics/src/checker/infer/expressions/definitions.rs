use syntax::ast::Expression;
use syntax::program::Definition;

use super::super::super::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    pub(super) fn infer_struct_definition(
        &mut self,
        store: &Store,
        expression: Expression,
    ) -> Expression {
        let Expression::Struct {
            doc,
            attributes,
            name,
            name_span,
            generics,
            fields,
            kind,
            visibility,
            span,
        } = expression
        else {
            unreachable!()
        };

        let qualified_name = self.qualify_name(&name);
        if let Some(Definition::Struct {
            name: definition_name,
            name_span: definition_name_span,
            generics: definition_generics,
            fields: definition_fields,
            kind: definition_kind,
            ..
        }) = store.get_definition(&qualified_name)
        {
            let definition_name = definition_name.clone();
            let definition_name_span = *definition_name_span;
            let definition_generics = definition_generics.clone();
            let definition_fields = definition_fields.clone();
            let definition_kind = *definition_kind;

            Expression::Struct {
                doc,
                attributes,
                name: definition_name,
                name_span: definition_name_span,
                generics: definition_generics,
                fields: definition_fields,
                kind: definition_kind,
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
                kind,
                visibility,
                span,
            }
        }
    }

    pub(super) fn infer_type_alias_definition(
        &mut self,
        store: &Store,
        expression: Expression,
    ) -> Expression {
        let Expression::TypeAlias {
            doc,
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
        if let Some(Definition::TypeAlias {
            name: alias_name,
            generics: definition_generics,
            annotation: definition_annotation,
            ty: definition_ty,
            ..
        }) = store.get_definition(&qualified_name)
        {
            Expression::TypeAlias {
                doc,
                name: alias_name.clone(),
                name_span,
                generics: definition_generics.clone(),
                annotation: definition_annotation.clone(),
                ty: definition_ty.clone(),
                visibility,
                span,
            }
        } else {
            Expression::TypeAlias {
                doc,
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
