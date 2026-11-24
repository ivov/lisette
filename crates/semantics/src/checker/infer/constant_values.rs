use syntax::ast::Expression;
use syntax::constant;
use syntax::program::{ConstantValue, Definition, DefinitionBody, ValueKind};
use syntax::types::Symbol;

use crate::store::Store;

pub(crate) fn evaluate_package_constants(store: &mut Store, package_id: &str) {
    let Some(package) = store.get_package(package_id) else {
        return;
    };
    let items: Vec<&Expression> = package
        .files
        .values()
        .flat_map(|file| &file.items)
        .collect();
    let pending = pending_constants(store, package_id, items);
    settle(store, package_id, pending);
}

fn pending_constants(
    store: &Store,
    package_id: &str,
    items: Vec<&Expression>,
) -> Vec<(Symbol, Expression)> {
    items
        .into_iter()
        .filter_map(|item| {
            let Expression::Const {
                identifier,
                expression,
                ..
            } = item
            else {
                return None;
            };
            let name = Symbol::from_parts(package_id, identifier);
            let has_value = store
                .get_definition(name.as_str())
                .is_some_and(|definition| definition.const_value().is_some());
            let initializer = expression.value()?;
            (!has_value).then(|| (name, initializer.clone()))
        })
        .collect()
}

fn settle(store: &mut Store, package_id: &str, mut pending: Vec<(Symbol, Expression)>) {
    loop {
        let mut settled = Vec::new();
        pending.retain(
            |(name, initializer)| match value_in_own_type(store, name, initializer) {
                Some(value) => {
                    settled.push((name.clone(), value));
                    false
                }
                None => true,
            },
        );
        if settled.is_empty() {
            return;
        }
        let Some(package) = store.edit_package(package_id) else {
            return;
        };
        for (name, value) in settled {
            if let Some(Definition {
                body: DefinitionBody::Value { kind, .. },
                ..
            }) = package.definitions.get_mut(name.as_str())
            {
                *kind = ValueKind::Constant(ConstantValue::Evaluated(value));
            }
        }
    }
}

fn value_in_own_type(
    store: &Store,
    name: &Symbol,
    initializer: &Expression,
) -> Option<constant::Value> {
    let ty = &store.get_definition(name.as_str())?.ty;
    constant::local_constant(initializer, ty, store).map(|constant| constant.value)
}
