use rustc_hash::FxHashSet as HashSet;

use crate::output::imports::ImportBuilder;
use crate::types::go_type::GoType;

#[derive(Debug, Clone, Default)]
pub(crate) struct EmitEffects {
    pub needs_stdlib: bool,
    pub needs_fmt: bool,
    pub needs_errors: bool,
    pub needs_slices: bool,
    pub needs_strings: bool,
    pub needs_maps: bool,
    pub go_imports: Vec<String>,
}

impl EmitEffects {
    pub(crate) fn require_stdlib(&mut self) {
        self.needs_stdlib = true;
    }

    pub(crate) fn require_fmt(&mut self) {
        self.needs_fmt = true;
    }

    pub(crate) fn require_errors(&mut self) {
        self.needs_errors = true;
    }

    pub(crate) fn require_slices(&mut self) {
        self.needs_slices = true;
    }

    pub(crate) fn require_strings(&mut self) {
        self.needs_strings = true;
    }

    pub(crate) fn require_maps(&mut self) {
        self.needs_maps = true;
    }

    pub(crate) fn require_go_import(&mut self, path: impl Into<String>) {
        self.go_imports.push(path.into());
    }

    pub(crate) fn merge_from_go_type(&mut self, go_type: &GoType) {
        self.needs_stdlib |= go_type.needs_stdlib;
        self.go_imports.extend(go_type.go_imports.iter().cloned());
    }

    pub(crate) fn extend(&mut self, other: &EmitEffects) {
        self.needs_stdlib |= other.needs_stdlib;
        self.needs_fmt |= other.needs_fmt;
        self.needs_errors |= other.needs_errors;
        self.needs_slices |= other.needs_slices;
        self.needs_strings |= other.needs_strings;
        self.needs_maps |= other.needs_maps;
        self.go_imports.extend(other.go_imports.iter().cloned());
    }

    pub(crate) fn drain_into(&self, builder: &mut ImportBuilder) {
        let modules: HashSet<String> = self.go_imports.iter().cloned().collect();
        builder.extend_with_modules(&modules);
        if self.needs_stdlib {
            builder.require_stdlib();
        }
        for (flag, path) in [
            (self.needs_fmt, "fmt"),
            (self.needs_errors, "errors"),
            (self.needs_slices, "slices"),
            (self.needs_strings, "strings"),
            (self.needs_maps, "maps"),
        ] {
            if flag {
                builder.require_path(path);
            }
        }
    }
}
