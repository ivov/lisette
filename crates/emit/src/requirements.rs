use rustc_hash::FxHashSet as HashSet;

use crate::imports::ImportBuilder;

#[derive(Default)]
struct EmitFlags {
    needs_fmt: bool,
    needs_stdlib: bool,
    needs_errors: bool,
    needs_slices: bool,
    needs_strings: bool,
    needs_maps: bool,
}

#[derive(Default)]
pub(crate) struct EmitRequirements {
    flags: EmitFlags,
    go_imports: HashSet<String>,
}

impl EmitRequirements {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn require_stdlib(&mut self) {
        self.flags.needs_stdlib = true;
    }

    pub(crate) fn require_fmt(&mut self) {
        self.flags.needs_fmt = true;
    }

    pub(crate) fn require_errors(&mut self) {
        self.flags.needs_errors = true;
    }

    pub(crate) fn require_slices(&mut self) {
        self.flags.needs_slices = true;
    }

    pub(crate) fn require_strings(&mut self) {
        self.flags.needs_strings = true;
    }

    pub(crate) fn require_maps(&mut self) {
        self.flags.needs_maps = true;
    }

    pub(crate) fn require_go_import(&mut self, module_id: impl Into<String>) {
        self.go_imports.insert(module_id.into());
    }

    pub(crate) fn apply_effects(&mut self, effects: &EmitEffects) {
        self.flags.needs_stdlib |= effects.needs_stdlib;
        self.flags.needs_fmt |= effects.needs_fmt;
        self.flags.needs_errors |= effects.needs_errors;
        self.flags.needs_slices |= effects.needs_slices;
        self.flags.needs_strings |= effects.needs_strings;
        self.flags.needs_maps |= effects.needs_maps;
        for go_import in &effects.go_imports {
            self.go_imports.insert(go_import.clone());
        }
    }

    pub(crate) fn drain_into(&mut self, builder: &mut ImportBuilder) {
        let drained = std::mem::take(self);
        builder.extend_with_modules(&drained.go_imports);
        if drained.flags.needs_stdlib {
            builder.require_stdlib();
        }
        for (flag, path) in [
            (drained.flags.needs_fmt, "fmt"),
            (drained.flags.needs_errors, "errors"),
            (drained.flags.needs_slices, "slices"),
            (drained.flags.needs_strings, "strings"),
            (drained.flags.needs_maps, "maps"),
        ] {
            if flag {
                builder.require_path(path);
            }
        }
    }
}

/// Pure-analysis effects: stdlib gates and Go imports accumulated while a
/// `&Emitter` helper computes a result, applied later at the renderer
/// boundary via `Emitter::apply_effects`.
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
    pub(crate) fn require_go_import(&mut self, path: impl Into<String>) {
        self.go_imports.push(path.into());
    }

    pub(crate) fn merge_from_go_type(&mut self, go_type: &crate::types::go_type::GoType) {
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
}
