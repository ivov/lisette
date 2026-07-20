use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;

#[derive(Default)]
pub(crate) struct AdapterRegistry {
    synthesized: HashMap<(EcoString, EcoString), String>,
    declarations: Vec<String>,
    emitted_count: usize,
}

impl AdapterRegistry {
    pub(crate) fn lookup(&self, key: &(EcoString, EcoString)) -> Option<&str> {
        self.synthesized.get(key).map(String::as_str)
    }

    pub(crate) fn next_index(&self) -> usize {
        self.declarations.len()
    }

    pub(crate) fn insert(
        &mut self,
        key: (EcoString, EcoString),
        name: String,
        declaration: String,
    ) {
        self.synthesized.insert(key, name);
        self.declarations.push(declaration);
    }

    pub(crate) fn push_declaration(&mut self, declaration: String) {
        self.declarations.push(declaration);
    }

    pub(crate) fn flush_new_declarations(&mut self) -> Vec<String> {
        let new_declarations = self.declarations[self.emitted_count..].to_vec();
        self.emitted_count = self.declarations.len();
        new_declarations
    }
}
