use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ParseError;
use syntax::program::{Definition, EmitInput, File, ModuleInfo, MutationInfo, UnusedInfo};
use syntax::types::Symbol;

use crate::LisetteDiagnostic;

pub struct TypedefSource {
    pub source: String,
    pub filename: String,
}

pub struct SemanticResult {
    pub files: HashMap<u32, File>,
    pub definitions: HashMap<Symbol, Definition>,
    pub modules: HashMap<String, ModuleInfo>,
    pub errors: Vec<LisetteDiagnostic>,
    pub lints: Vec<LisetteDiagnostic>,
    pub entry_module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub cached_modules: HashSet<String>,
    pub ufcs_methods: HashSet<(String, String)>,
    pub typedef_sources: HashMap<u32, TypedefSource>,
    pub go_package_names: HashMap<String, String>,
}

impl SemanticResult {
    pub fn with_parse_errors(errors: Vec<ParseError>, entry_module_id: &str) -> Self {
        Self {
            files: HashMap::default(),
            definitions: HashMap::default(),
            modules: HashMap::default(),
            errors: errors.into_iter().map(Into::into).collect(),
            lints: vec![],
            entry_module_id: entry_module_id.to_string(),
            unused: UnusedInfo::default(),
            mutations: MutationInfo::default(),
            cached_modules: HashSet::default(),
            ufcs_methods: HashSet::default(),
            typedef_sources: HashMap::default(),
            go_package_names: HashMap::default(),
        }
    }

    pub fn failed(&self) -> bool {
        self.errors.iter().any(|e| e.is_error())
    }

    pub fn into_emit_input(self) -> EmitInput {
        EmitInput {
            files: self.files,
            definitions: self.definitions,
            modules: self.modules,
            entry_module_id: self.entry_module_id,
            unused: self.unused,
            mutations: self.mutations,
            cached_modules: self.cached_modules,
            ufcs_methods: self.ufcs_methods,
            go_package_names: self.go_package_names,
        }
    }
}
