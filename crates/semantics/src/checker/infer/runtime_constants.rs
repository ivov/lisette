use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::program::{Definition, DefinitionBody, DotAccessResolution, ValueKind};
use syntax::types::Symbol;

use crate::store::Store;

pub(crate) fn mark_runtime_constants(store: &mut Store, package_id: &str) {
    let Some(package) = store.get_package(package_id) else {
        return;
    };
    let initializers: Vec<(Symbol, &Expression)> = package
        .files
        .values()
        .filter(|file| !file.is_d_lis())
        .flat_map(|file| &file.items)
        .filter_map(|item| {
            let Expression::Const {
                identifier,
                expression,
                ..
            } = item
            else {
                return None;
            };
            Some((
                Symbol::from_parts(package_id, identifier),
                expression.value()?,
            ))
        })
        .collect();

    let mut constants: HashSet<Symbol> = HashSet::default();
    loop {
        let newly_constant: Vec<Symbol> = initializers
            .iter()
            .filter(|(name, initializer)| {
                !constants.contains(name) && is_constant_expression(initializer, store, &constants)
            })
            .map(|(name, _)| name.clone())
            .collect();
        if newly_constant.is_empty() {
            break;
        }
        constants.extend(newly_constant);
    }

    let runtime: Vec<Symbol> = initializers
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !constants.contains(name))
        .collect();
    let Some(package) = store.edit_package(package_id) else {
        return;
    };
    for name in runtime {
        if let Some(Definition {
            body: DefinitionBody::Value { kind, .. },
            ..
        }) = package.definitions.get_mut(name.as_str())
        {
            *kind = ValueKind::RuntimeConstant;
        }
    }
}

fn is_constant_expression(
    expression: &Expression,
    store: &Store,
    constants: &HashSet<Symbol>,
) -> bool {
    match expression {
        Expression::Literal { literal, .. } => {
            !matches!(literal, Literal::FormatString(_) | Literal::Slice(_))
        }
        Expression::Identifier { resolution, .. } => resolution
            .definition()
            .is_some_and(|symbol| constants.contains(symbol)),
        Expression::DotAccess {
            resolution:
                DotAccessResolution::PackageMember {
                    definition: Some(definition),
                    ..
                }
                | DotAccessResolution::StaticMethod { definition, .. },
            ..
        } => store
            .get_definition(definition)
            .is_some_and(Definition::is_compile_time_const),
        Expression::Paren { expression, .. } => {
            is_constant_expression(expression, store, constants)
        }
        Expression::Unary {
            operator: UnaryOperator::Negative | UnaryOperator::Not | UnaryOperator::BitwiseNot,
            expression,
            ..
        } => is_constant_expression(expression, store, constants),
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => {
            !matches!(operator, BinaryOperator::Pipeline)
                && is_constant_expression(left, store, constants)
                && is_constant_expression(right, store, constants)
        }
        _ => false,
    }
}
