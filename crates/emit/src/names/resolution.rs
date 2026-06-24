use syntax::types::unqualified_name;

use crate::EmitEffects;
use crate::Planner;
use crate::names::go_name;

impl Planner<'_> {
    pub(crate) fn resolve_go_name(&mut self, name: &str) -> String {
        if !name.contains('.')
            && let Some(remapped) = self.module.escape_remap(name)
        {
            return remapped.to_string();
        }

        if let Some(go_call) = self.try_resolve_cross_module_static_method(name) {
            return go_call;
        }

        let name = if let Some((type_part, method)) = name.split_once('.')
            && !type_part.contains('.')
            && let Some(real_type) = self.resolve_alias_type_name(type_part)
        {
            format!("{}.{}", real_type, method)
        } else {
            name.to_string()
        };

        let name = if let Some((type_part, _method)) = name.split_once('.')
            && !type_part.contains('.')
            && !name.starts_with(go_name::PRELUDE_PREFIX)
            && self
                .facts
                .definition(format!("{}.{}", go_name::PRELUDE_MODULE, type_part).as_str())
                .is_some()
        {
            format!("{}.{}", go_name::PRELUDE_MODULE, name)
        } else {
            name
        };

        let resolved = go_name::resolve(&name);
        if resolved.needs_stdlib {
            self.require_stdlib();
        }
        resolved.name
    }

    pub(crate) fn resolve_alias_type_name(&self, type_part: &str) -> Option<String> {
        let qualified = self.facts.qualified_current(type_part);
        let id = self.peel_alias_id(&qualified);
        if id == qualified {
            return None;
        }
        let type_module = self.facts.module_for_qualified_name(&id).unwrap_or(&id);
        if self.facts.is_current_module(type_module) {
            return Some(unqualified_name(&id).to_string());
        }
        Some(id)
    }

    pub(crate) fn capitalize_static_method_if_public(&self, name: &str) -> String {
        let Some((type_part, method_part)) = name.split_once('.') else {
            return name.to_string();
        };

        if method_part.contains('.') {
            return name.to_string();
        }

        let method_key = self.facts.qualified_current_member(type_part, method_part);
        let found = self.facts.definition(method_key.as_str()).or_else(|| {
            let real_type = self.resolve_alias_type_name(type_part)?;
            let alias_key = self.facts.qualified_current_member(&real_type, method_part);
            self.facts.definition(alias_key.as_str())
        });
        let is_public = if let Some(d) = found {
            d.visibility().is_public() || self.method_needs_export(method_part)
        } else {
            self.method_needs_export(method_part)
        };

        if is_public {
            format!("{}.{}", type_part, go_name::snake_to_camel(method_part))
        } else {
            name.to_string()
        }
    }

    /// Record `module`'s Go import into `effects` and return the package
    /// qualifier exactly as the import renders it: `format_import` sanitizes
    /// default package names and prints explicit aliases verbatim, so
    /// references must follow the same rule.
    pub(crate) fn record_module_import(&self, module: &str, effects: &mut EmitEffects) -> String {
        let path = self.go_import_path_for_module(module);
        effects.require_go_import(path.clone());
        let qualifier = self.go_pkg_qualifier(module);
        if qualifier == go_name::go_package_name(&path) {
            go_name::sanitize_package_name(&qualifier).into_owned()
        } else {
            qualifier
        }
    }

    /// `record_module_import` writing into the file effects sink.
    pub(crate) fn require_module_import(&self, module: &str) -> String {
        self.record_module_import(module, &mut self.effects.borrow_mut())
    }

    pub(crate) fn go_import_path_for_module(&self, module: &str) -> String {
        let canonical = self.module.module_for_alias(module).unwrap_or(module);
        match canonical.strip_prefix(go_name::GO_IMPORT_PREFIX) {
            Some(rest) => rest.to_string(),
            None => self.facts.go_import_path(canonical),
        }
    }

    pub(crate) fn go_pkg_qualifier(&self, module: &str) -> String {
        if module == go_name::TEST_PRELUDE_MODULE {
            return go_name::TESTKIT_PKG.to_string();
        }
        if let Some(alias) = self.module.module_alias(module) {
            return alias.to_string();
        }
        if let Some(pkg_name) = self.facts.go_package_name(module) {
            return pkg_name.to_string();
        }
        match module.strip_prefix(go_name::GO_IMPORT_PREFIX) {
            Some(go_path) => syntax::program::go_import_default_name(go_path).to_string(),
            None => go_name::go_package_name(module).to_string(),
        }
    }

    pub(crate) fn qualify_method_call(
        &mut self,
        type_id: &str,
        method: &str,
        is_public: bool,
    ) -> String {
        let module = self
            .facts
            .module_for_qualified_name(type_id)
            .map(str::to_string);
        let type_name = unqualified_name(type_id);
        let computed_alias = match module.as_deref() {
            Some(m) if self.facts.is_foreign_module(m) => Some(self.require_module_import(m)),
            _ => None,
        };
        let resolved = go_name::qualify_method(
            module.as_deref(),
            type_name,
            method,
            self.facts.current_module(),
            is_public,
            computed_alias.as_deref(),
        );
        if resolved.needs_stdlib {
            self.require_stdlib();
        }
        resolved.name
    }

    pub(crate) fn resolve_variant(&mut self, identifier: &str, enum_id: &str) -> String {
        let enum_module = self
            .facts
            .module_for_qualified_name(enum_id)
            .unwrap_or(enum_id);
        let computed_alias = if self.facts.is_foreign_module(enum_module) {
            Some(self.require_module_import(enum_module))
        } else {
            None
        };
        let resolved = go_name::variant_by_id(
            identifier,
            enum_id,
            enum_module,
            self.facts.current_module(),
            computed_alias.as_deref(),
        );
        if resolved.needs_stdlib {
            self.require_stdlib();
        }
        resolved.name
    }
}
