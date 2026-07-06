use syntax::ast::{Expression, Literal, Span, UnaryOperator};
use syntax::program::{CallKind, DefinitionBody};
use syntax::types::Type;

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::checker::infer::addressability::check_is_non_addressable;
use crate::checker::infer::carry_mut::{
    can_carry_mutation_across_fn_boundary, clone_recipe_is_known, clone_severs_alias,
};

struct AliasFinding {
    source: String,
    span: Span,
    via_non_severing_clone: bool,
    clone_severs: bool,
    addressable: bool,
}

impl InferCtx<'_, '_> {
    /// Reject a mutable binding that would share the backing store of a live one.
    pub(super) fn check_mut_binding_alias(&mut self, binding_name: &str, value: &Expression) {
        let Some(finding) = self.find_alias(value, true) else {
            return;
        };
        self.push_binding_alias(binding_name, finding);
    }

    pub(super) fn check_mut_reassignment_alias(&mut self, binding_name: &str, value: &Expression) {
        let Some(finding) = self.find_alias(value, false) else {
            return;
        };
        self.push_binding_alias(binding_name, finding);
    }

    fn push_binding_alias(&mut self, binding_name: &str, finding: AliasFinding) {
        let diagnostic = if finding.via_non_severing_clone {
            diagnostics::infer::mut_binding_clone_does_not_sever(
                binding_name,
                &finding.source,
                finding.addressable,
                finding.span,
            )
        } else {
            diagnostics::infer::mut_binding_aliases(
                binding_name,
                &finding.source,
                finding.addressable,
                finding.clone_severs,
                finding.span,
            )
        };
        self.sink.push(diagnostic);
    }

    fn find_alias(&self, value: &Expression, through_constructions: bool) -> Option<AliasFinding> {
        let expr = value.unwrap_parens();

        if let Some(receiver) = clone_call_receiver(expr) {
            return self.alias_leaf(receiver.unwrap_parens(), true, expr.get_span());
        }

        match expr {
            Expression::Identifier { .. }
            | Expression::DotAccess { .. }
            | Expression::IndexedAccess { .. }
            | Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => return self.alias_leaf(expr, false, expr.get_span()),
            _ => {}
        }

        if !through_constructions
            || !can_carry_mutation_across_fn_boundary(&expr.get_type(), &self.env, self.store)
        {
            return None;
        }

        match expr {
            Expression::Block { items, .. } => self.find_alias(items.last()?, true),
            Expression::If {
                consequence,
                alternative,
                ..
            } => self
                .find_alias(consequence, true)
                .or_else(|| self.find_alias(alternative, true)),
            Expression::Tuple { elements, .. } => {
                elements.iter().find_map(|e| self.find_alias(e, true))
            }
            Expression::StructCall {
                field_assignments,
                ty,
                ..
            } if self.is_struct_backed(ty) => field_assignments
                .iter()
                .find_map(|f| self.find_alias(&f.value, true)),
            Expression::Literal {
                literal: Literal::Slice(elements),
                ..
            } => elements.iter().find_map(|e| self.find_alias(e, true)),
            Expression::Call {
                args,
                call_kind: Some(CallKind::TupleStructConstructor),
                ..
            } => args.iter().find_map(|a| self.find_alias(a, true)),
            _ => None,
        }
    }

    fn is_struct_backed(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = self.store.peel_alias(&ty.resolve_in(&self.env)) else {
            return false;
        };
        matches!(
            self.store.get_definition(id.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Struct { .. })
        )
    }

    fn alias_leaf(&self, place: &Expression, via_clone: bool, span: Span) -> Option<AliasFinding> {
        let Expression::Identifier {
            value,
            binding_id: Some(root_id),
            ..
        } = place_root(place)?
        else {
            return None;
        };
        if self.scopes.lookup_binding_id(value) != Some(*root_id) {
            return None;
        }
        let ty = place.get_type();
        if !can_carry_mutation_across_fn_boundary(&ty, &self.env, self.store) {
            return None;
        }
        if via_clone && !clone_recipe_is_known(&ty, &self.env, self.store) {
            return None;
        }
        let clone_severs = clone_severs_alias(&ty, &self.env, self.store);
        if via_clone && clone_severs {
            return None;
        }
        Some(AliasFinding {
            source: render_place(place),
            span,
            via_non_severing_clone: via_clone,
            clone_severs,
            addressable: check_is_non_addressable(place, &self.env).is_none(),
        })
    }

    pub(super) fn non_severing_clone_source(&self, value: &Expression) -> Option<String> {
        let expr = value.unwrap_parens();
        let receiver = clone_call_receiver(expr)?;
        let finding = self.alias_leaf(receiver.unwrap_parens(), true, expr.get_span())?;
        Some(finding.source)
    }
}

/// The receiver of a `clone()` call written as `x.clone()`, `Type.clone(x)`,
/// or `Slice.clone(x)`. `None` for a free function named `clone` (UFCS).
pub(super) fn clone_call_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        call_kind,
        ..
    } = expression
    else {
        return None;
    };
    match callee.unwrap_parens() {
        Expression::DotAccess {
            expression, member, ..
        } if member == "clone"
            && args.is_empty()
            && !matches!(call_kind, Some(CallKind::UfcsMethod)) =>
        {
            Some(expression)
        }
        Expression::Identifier { .. }
            if args.len() == 1
                && matches!(
                    call_kind,
                    Some(CallKind::NativeMethodIdentifier(_) | CallKind::ReceiverMethodUfcs { .. })
                )
                && callee
                    .get_var_name()
                    .is_some_and(|name| name.ends_with(".clone")) =>
        {
            args.first()
        }
        _ => None,
    }
}

fn place_root(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Identifier { .. } => Some(expression),
        Expression::DotAccess {
            expression: inner, ..
        }
        | Expression::IndexedAccess {
            expression: inner, ..
        }
        | Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => place_root(inner.unwrap_parens()),
        _ => None,
    }
}

/// Name of the identifier a place expression is rooted at.
pub(super) fn place_root_name(expression: &Expression) -> Option<String> {
    place_root(expression)?.get_var_name()
}

/// Reconstructed source text for a place expression. Indexes that are not
/// literals or identifiers render as `..` placeholders.
fn render_place(expression: &Expression) -> String {
    match expression {
        Expression::Identifier { .. } => expression.get_var_name().unwrap_or_default(),
        Expression::DotAccess {
            expression: inner,
            member,
            ..
        } => format!("{}.{member}", render_place(inner.unwrap_parens())),
        Expression::IndexedAccess {
            expression: inner,
            index,
            ..
        } => format!(
            "{}[{}]",
            render_place(inner.unwrap_parens()),
            render_index(index.unwrap_parens())
        ),
        Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } => format!("{}.*", render_place(inner.unwrap_parens())),
        _ => String::new(),
    }
}

fn render_index(index: &Expression) -> String {
    match index {
        Expression::Range {
            start,
            end,
            inclusive,
            ..
        } => {
            let endpoint = |e: &Option<Box<Expression>>| match e {
                None => Some(String::new()),
                Some(e) => render_scalar(e.unwrap_parens()),
            };
            match (endpoint(start), endpoint(end)) {
                (Some(s), Some(e)) => {
                    format!("{s}{}{e}", if *inclusive { "..=" } else { ".." })
                }
                _ => "..".to_string(),
            }
        }
        other => render_scalar(other).unwrap_or_else(|| "..".to_string()),
    }
}

fn render_scalar(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Identifier { .. } => expression.get_var_name(),
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { value, text } => {
                Some(text.clone().unwrap_or_else(|| value.to_string()))
            }
            Literal::String { value, .. } => Some(format!("{value:?}")),
            Literal::Boolean(value) => Some(value.to_string()),
            _ => None,
        },
        _ => None,
    }
}
