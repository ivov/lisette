use ecow::EcoString;

#[derive(Default)]
pub(crate) struct AdapterRegistry {
    adapters: Vec<Adapter>,
}

struct Adapter {
    /// Generic adapters depend on their declaration's type parameters and are not reused.
    key: Option<(EcoString, EcoString)>,
    name: String,
}

impl AdapterRegistry {
    pub(crate) fn lookup(&self, key: &(EcoString, EcoString)) -> Option<&str> {
        self.adapters
            .iter()
            .rev()
            .find(|adapter| adapter.key.as_ref() == Some(key))
            .map(|adapter| adapter.name.as_str())
    }

    /// Reserve the name before emitting methods, which may themselves need adapters.
    pub(crate) fn register(
        &mut self,
        key: Option<(EcoString, EcoString)>,
        name: impl FnOnce(usize) -> String,
    ) -> String {
        let name = name(self.adapters.len());
        self.adapters.push(Adapter {
            key,
            name: name.clone(),
        });
        name
    }
}
