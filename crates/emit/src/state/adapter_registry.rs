use syntax::types::Type;

#[derive(Clone, Default)]
pub(crate) struct AdapterRegistry {
    adapters: Vec<Adapter>,
}

#[derive(Clone)]
struct Adapter {
    /// Generic adapters depend on their declaration's type parameters and are not reused.
    key: Option<(Type, Type)>,
    name: String,
}

impl AdapterRegistry {
    pub(crate) fn lookup(&self, key: &(Type, Type)) -> Option<&str> {
        let key = (key.0.without_permissions(), key.1.without_permissions());
        self.adapters
            .iter()
            .rev()
            .find(|adapter| adapter.key.as_ref() == Some(&key))
            .map(|adapter| adapter.name.as_str())
    }

    /// Reserve the name before emitting methods, which may themselves need adapters.
    pub(crate) fn register(
        &mut self,
        key: Option<(Type, Type)>,
        name: impl FnOnce(usize) -> String,
    ) -> String {
        let name = name(self.adapters.len());
        self.adapters.push(Adapter {
            key: key.map(|(source, target)| {
                (source.without_permissions(), target.without_permissions())
            }),
            name: name.clone(),
        });
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn nominal(id: &str, parameters: Vec<Type>) -> Type {
        Type::Nominal {
            id: id.into(),
            params: parameters,
            writable: false,
        }
    }
    #[test]
    fn reference_permissions_share_an_adapter() {
        use syntax::types::CompoundKind;
        let source = Type::Compound {
            kind: CompoundKind::Ref,
            args: vec![nominal("a.Item", vec![])],
            writable: false,
        };
        let target = nominal("api.View", vec![]);
        let mut registry = AdapterRegistry::default();
        registry.register(Some((source.clone(), target.clone())), |_| {
            "shared".to_string()
        });
        assert_eq!(
            registry.lookup(&(source.make_writable(), target)),
            Some("shared")
        );
    }

    #[test]
    fn interface_instantiations_and_qualified_owners_have_distinct_adapters() {
        let concrete = nominal("a.Item", vec![]);
        let first = (
            concrete.clone(),
            nominal("api.View", vec![nominal("a.Item", vec![])]),
        );
        let second = (
            concrete,
            nominal("api.View", vec![nominal("b.Item", vec![])]),
        );
        let mut registry = AdapterRegistry::default();
        registry.register(Some(first.clone()), |_| "first".to_string());
        assert_eq!(registry.lookup(&first), Some("first"));
        assert_eq!(registry.lookup(&second), None);
        registry.register(Some(second.clone()), |_| "second".to_string());
        assert_eq!(registry.lookup(&second), Some("second"));
        assert_eq!(registry.lookup(&first), Some("first"));
    }
}
