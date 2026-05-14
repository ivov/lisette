use syntax::types::{module_part, unqualified_name};

use crate::Emitter;
use crate::names::go_name;

impl Emitter<'_> {
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
            self.requirements.require_stdlib();
        }
        resolved.name
    }

    pub(crate) fn resolve_alias_type_name(&self, type_part: &str) -> Option<String> {
        let qualified = self.facts.qualified_current(type_part);
        let id = self.peel_alias_id(&qualified);
        if id == qualified {
            return None;
        }
        let type_module = module_part(&id);
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

    pub(crate) fn require_module_import(&mut self, module: &str) -> String {
        self.requirements
            .require_go_import(self.facts.go_import_path(module));
        self.go_pkg_qualifier(module)
    }

    pub(crate) fn go_pkg_qualifier(&self, module: &str) -> String {
        if let Some(alias) = self.module.module_alias(module) {
            return alias.to_string();
        }
        if let Some(pkg_name) = self.facts.go_package_name(module) {
            return pkg_name.to_string();
        }
        let path = module
            .strip_prefix(go_name::GO_IMPORT_PREFIX)
            .unwrap_or(module);
        go_name::go_package_name(path).to_string()
    }

    pub(crate) fn qualify_method_call(
        &mut self,
        type_id: &str,
        method: &str,
        is_public: bool,
    ) -> String {
        let module = type_id.split_once('.').map(|(m, _)| m);
        let computed_alias = match module {
            Some(m) if !self.facts.is_current_module(m) && m != go_name::PRELUDE_MODULE => {
                Some(self.go_pkg_qualifier(m))
            }
            _ => None,
        };
        let resolved = go_name::qualify_method(
            type_id,
            method,
            self.facts.current_module(),
            is_public,
            computed_alias.as_deref(),
        );
        if resolved.needs_stdlib {
            self.requirements.require_stdlib();
        }
        resolved.name
    }

    pub(crate) fn resolve_variant(&mut self, identifier: &str, enum_id: &str) -> String {
        let enum_module = module_part(enum_id);
        let computed_alias = if !self.facts.is_current_module(enum_module)
            && enum_module != go_name::PRELUDE_MODULE
        {
            Some(self.go_pkg_qualifier(enum_module))
        } else {
            None
        };
        let resolved = go_name::variant_by_id(
            identifier,
            enum_id,
            self.facts.current_module(),
            computed_alias.as_deref(),
        );
        if resolved.needs_stdlib {
            self.requirements.require_stdlib();
        }
        resolved.name
    }
}
