use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;

use crate::ast::{BindingId as AstBindingId, Pattern, RestPattern, Span};
use crate::types::Symbol;

use super::{Definition, File, ModuleInfo};

#[derive(Debug, Clone, Default)]
pub struct UnusedInfo {
    bindings: HashSet<Span>,
    definitions: HashSet<Span>,
    pub imports_by_module: HashMap<EcoString, HashSet<EcoString>>,
}

impl UnusedInfo {
    pub fn mark_binding_unused(&mut self, span: Span) {
        self.bindings.insert(span);
    }

    pub fn is_unused_binding(&self, pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier { span, .. } => self.bindings.contains(span),
            Pattern::AsBinding { span, name, .. } => {
                let name_span = Span::new(
                    span.file_id,
                    span.byte_offset + span.byte_length - name.len() as u32,
                    name.len() as u32,
                );
                self.bindings.contains(&name_span)
            }
            _ => false,
        }
    }

    pub fn is_unused_rest_binding(&self, rest: &RestPattern) -> bool {
        match rest {
            RestPattern::Bind { span, .. } => self.bindings.contains(span),
            _ => false,
        }
    }

    pub fn mark_definition_unused(&mut self, span: Span) {
        self.definitions.insert(span);
    }

    pub fn is_unused_definition(&self, span: &Span) -> bool {
        self.definitions.contains(span)
    }

    pub fn merge(&mut self, other: UnusedInfo) {
        let UnusedInfo {
            bindings,
            definitions,
            imports_by_module,
        } = other;
        self.bindings.extend(bindings);
        self.definitions.extend(definitions);
        for (module, imports) in imports_by_module {
            self.imports_by_module
                .entry(module)
                .or_default()
                .extend(imports);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MutationInfo {
    bindings: HashSet<AstBindingId>,
}

impl MutationInfo {
    pub fn mark_binding_mutated(&mut self, id: AstBindingId) {
        self.bindings.insert(id);
    }

    pub fn is_mutated(&self, id: AstBindingId) -> bool {
        self.bindings.contains(&id)
    }
}

pub struct EmitInput {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub modules: HashMap<String, ModuleInfo>,
    pub entry_module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub cached_modules: HashSet<String>,
    pub ufcs_methods: HashSet<(String, String)>,
    pub go_package_names: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(offset: u32) -> Span {
        Span::new(0, offset, 1)
    }

    #[test]
    fn merge_extends_bindings_definitions_and_imports() {
        let mut a = UnusedInfo::default();
        a.mark_binding_unused(span(0));
        a.mark_definition_unused(span(1));
        a.imports_by_module
            .insert("m1".into(), HashSet::from_iter(["x".into()]));

        let mut b = UnusedInfo::default();
        b.mark_binding_unused(span(2));
        b.mark_definition_unused(span(3));
        b.imports_by_module
            .insert("m1".into(), HashSet::from_iter(["y".into()]));
        b.imports_by_module
            .insert("m2".into(), HashSet::from_iter(["z".into()]));

        a.merge(b);

        assert_eq!(a.bindings.len(), 2);
        assert_eq!(a.definitions.len(), 2);
        assert_eq!(a.imports_by_module["m1"].len(), 2);
        assert_eq!(a.imports_by_module["m2"].len(), 1);
    }
}
