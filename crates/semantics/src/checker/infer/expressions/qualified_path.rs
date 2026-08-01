use ecow::EcoString;
use syntax::ast::{Expression, IdentifierResolution, Span};
use syntax::program::{DefinitionBody, DotAccessResolution};
use syntax::types::{Type, unqualified_name};

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(super) fn infer_dot_access_or_qualified_path(
        &mut self,
        expression: Box<Expression>,
        member: EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        {
            let mut inner = &*expression;
            while let Expression::Paren { expression: e, .. } = inner {
                inner = e;
            }
            if !std::ptr::eq(inner, &*expression)
                && let Some(path) = inner.as_dotted_path()
                && inner.root_identifier().is_some_and(|root| {
                    self.lookup_qualified_name(store, root).is_some()
                        || self.imports.namespace(root).is_some()
                })
            {
                self.sink.push(diagnostics::infer::parenthesized_qualifier(
                    &path,
                    &member,
                    expression.get_span(),
                ));
                return Expression::DotAccess {
                    expression,
                    member,
                    ty: expected_ty.clone(),
                    span,
                    resolution: DotAccessResolution::Unresolved,
                };
            }
        }

        if let Some(resolved) = self.resolve_qualified_path(&expression, &member, span, expected_ty)
        {
            return resolved;
        }

        if let Some(root) = expression.root_identifier()
            && let Some(qualified_root) = self.lookup_qualified_name(store, root)
            && let Some(def) = store.get_definition(&qualified_root)
            && let DefinitionBody::TypeAlias { generics, .. } = &def.body
            && !generics.is_empty()
        {
            if qualified_root == "prelude.Ref" {
                self.sink.push(diagnostics::infer::ref_qualifier(
                    &member,
                    expression.get_span(),
                ));
            } else {
                let target = store.peel_alias(&def.ty);
                let type_name = if let Type::Nominal { id, .. } = &target {
                    unqualified_name(id).to_string()
                } else {
                    "the original type".to_string()
                };
                self.sink.push(diagnostics::infer::type_alias_as_qualifier(
                    root,
                    &type_name,
                    &member,
                    expression.get_span(),
                ));
            }
            return Expression::DotAccess {
                expression,
                member,
                ty: expected_ty.clone(),
                span,
                resolution: DotAccessResolution::Unresolved,
            };
        }

        self.infer_dot_access(expression, member, span, expected_ty)
    }

    fn resolve_qualified_path(
        &mut self,
        expression: &Expression,
        member: &EcoString,
        span: Span,
        expected_ty: &Type,
    ) -> Option<Expression> {
        let store = self.store;
        let root = expression.root_identifier()?;
        let qualified_root = self.lookup_qualified_name(store, root)?;
        let base = expression.as_dotted_path()?;

        let direct = format!("{}.{}", base, member);
        let path = if self.lookup_type(store, &direct).is_some() {
            self.track_name_usage(store, &qualified_root, &span, root.len() as u32);
            direct
        } else {
            let resolved_id = self.nongeneric_alias_target(&qualified_root)?;
            if self.alias_member_is_enum_variant(&resolved_id, member) {
                return None;
            }
            let short_name = unqualified_name(&resolved_id);
            let mut candidates = Vec::with_capacity(2);
            if short_name != resolved_id {
                candidates.push(format!("{}.{}", short_name, member));
            }
            candidates.push(format!("{}.{}", resolved_id, member));
            candidates
                .into_iter()
                .find(|candidate| self.lookup_type(store, candidate).is_some())?
        };

        Some(self.infer_expression(
            Expression::Identifier {
                value: path.into(),
                ty: Type::uninferred(),
                span,
                resolution: IdentifierResolution::Unresolved,
            },
            expected_ty,
        ))
    }

    fn nongeneric_alias_target(&self, qualified_root: &str) -> Option<String> {
        let store = self.store;
        let definition = store.get_definition(qualified_root)?;
        let DefinitionBody::TypeAlias { generics, .. } = &definition.body else {
            return None;
        };
        if !generics.is_empty() {
            return None;
        }
        match store.peel_alias(&definition.ty) {
            Type::Nominal { id, params } if params.is_empty() && id.as_str() != qualified_root => {
                Some(id.to_string())
            }
            Type::Simple(kind) => Some(format!("prelude.{}", kind.leaf_name())),
            Type::Compound { kind, args } if args.is_empty() => {
                Some(format!("prelude.{}", kind.leaf_name()))
            }
            _ => None,
        }
    }

    fn alias_member_is_enum_variant(&self, type_id: &str, member: &str) -> bool {
        let store = self.store;
        store
            .get_definition(type_id)
            .is_some_and(|definition| match &definition.body {
                DefinitionBody::Enum { variants, .. } => {
                    variants.iter().any(|variant| variant.name == member)
                }
                _ => false,
            })
    }
}
