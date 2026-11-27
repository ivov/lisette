use syntax::types::Type;

#[derive(Clone, Default)]
pub(crate) struct AdapterRegistry {
    adapters: Vec<Adapter>,
}

#[derive(Clone)]
struct Adapter {
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
