use ecow::EcoString;
use rustc_hash::FxHashMap as HashMap;

#[derive(Default)]
pub(crate) struct AdapterRegistry {
    synthesized: HashMap<(EcoString, EcoString), String>,
    pending_declarations: Vec<String>,
    next_index: usize,
}

impl AdapterRegistry {
    pub(crate) fn lookup(&self, key: &(EcoString, EcoString)) -> Option<&str> {
        self.synthesized.get(key).map(String::as_str)
    }

    pub(crate) fn allocate_index(&mut self) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        index
    }

    pub(crate) fn insert(
        &mut self,
        key: (EcoString, EcoString),
        name: String,
        declaration: String,
    ) {
        self.synthesized.insert(key, name);
        self.pending_declarations.push(declaration);
    }

    pub(crate) fn push_declaration(&mut self, declaration: String) {
        self.pending_declarations.push(declaration);
    }

    pub(crate) fn drain_declarations(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_declarations)
    }
}
