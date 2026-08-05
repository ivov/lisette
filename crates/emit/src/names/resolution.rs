use syntax::types::unqualified_name;

use crate::Planner;
use crate::names::go_name;
use crate::names::packages::{PackageRequirements, PackageUse};

impl Planner<'_> {
    /// A `locally_bound` name must not be rewritten by a package-level remap
    /// of the same text.
    pub(crate) fn resolve_go_name(
        &mut self,
        name: &str,
        qualified: Option<&str>,
        locally_bound: bool,
    ) -> String {
        if !locally_bound
            && !name.contains('.')
            && let Some(remapped) = self.package.escape_remap(name)
        {
            return remapped.to_string();
        }

        if let Some(go_call) = self.try_resolve_cross_package_static_method(qualified) {
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
                .definition(format!("{}.{}", go_name::PRELUDE_PACKAGE, type_part).as_str())
                .is_some()
        {
            format!("{}.{}", go_name::PRELUDE_PACKAGE, name)
        } else {
            name
        };

        let resolved = go_name::resolve(&name);
        if let Some(package) = resolved.package {
            self.require_generated_package(package);
        }
        resolved.name
    }

    pub(crate) fn resolve_alias_type_name(&self, type_part: &str) -> Option<String> {
        let qualified = self.facts.qualified_current(type_part);
        let id = self.peel_alias_id(&qualified);
        if id == qualified {
            return None;
        }
        let type_package = self.facts.package_for_qualified_name(&id).unwrap_or(&id);
        if self.facts.is_current_package(type_package) {
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
            d.visibility.is_public() || self.method_needs_export(method_part)
        } else {
            self.method_needs_export(method_part)
        };

        if is_public {
            format!("{}.{}", type_part, go_name::snake_to_camel(method_part))
        } else {
            format!(
                "{}.{}",
                type_part,
                go_name::snake_to_lower_camel(method_part)
            )
        }
    }

    pub(crate) fn reference_go_name(&self, lisette_name: &str) -> String {
        if let Some(bound) = self.scope.resolve_binding_go_name(lisette_name) {
            return bound.to_string();
        }
        self.package
            .escape_remap(lisette_name)
            .map(str::to_string)
            .unwrap_or_else(|| go_name::escape_reserved(lisette_name).into_owned())
    }

    /// Record `package`'s Go import and return the package
    /// qualifier exactly as the import renders it: `format_import` sanitizes
    /// default package names and prints explicit aliases verbatim, so
    /// references must follow the same rule.
    pub(crate) fn record_package_import(
        &self,
        package: &str,
        requirements: &mut PackageRequirements,
    ) -> String {
        let package = self.package_use_for_package(package);
        let qualifier = package.qualifier().to_string();
        requirements.require(package);
        qualifier
    }

    /// Record a package reference in the current file namespace.
    pub(crate) fn require_package_import(&self, package: &str) -> String {
        let package = self.package_use_for_package(package);
        self.file_namespace_mut().reference(package)
    }

    pub(crate) fn canonical_package(&self, package: &str) -> String {
        self.file_namespace()
            .package_for_alias(package)
            .unwrap_or(package)
            .to_string()
    }

    pub(crate) fn package_use_for_package(&self, package: &str) -> PackageUse {
        if package == go_name::TEST_PRELUDE_PACKAGE {
            return PackageUse::generated(go_name::GeneratedPackage::TestKit);
        }
        let path = match package.strip_prefix(go_name::GO_IMPORT_PREFIX) {
            Some(rest) => rest.to_string(),
            None => self.facts.go_import_path(package),
        };
        let qualifier = self
            .file_namespace()
            .package_alias(package)
            .map(str::to_string)
            .or_else(|| self.facts.go_package_name(package).map(str::to_string))
            .unwrap_or_else(|| match package.strip_prefix(go_name::GO_IMPORT_PREFIX) {
                Some(go_path) => syntax::program::go_import_default_name(go_path).to_string(),
                None => go_name::go_package_name(package).to_string(),
            });
        let qualifier = if qualifier == go_name::go_package_name(&path) {
            go_name::sanitize_package_name(&qualifier).into_owned()
        } else {
            qualifier
        };
        PackageUse::new(path, qualifier)
    }

    pub(crate) fn qualify_method_call(
        &mut self,
        type_id: &str,
        method: &str,
        is_public: bool,
    ) -> String {
        let package = self
            .facts
            .package_for_qualified_name(type_id)
            .map(str::to_string);
        let type_name = unqualified_name(type_id);
        let computed_alias = match package.as_deref() {
            Some(m) if self.facts.is_foreign_package(m) => Some(self.require_package_import(m)),
            _ => None,
        };
        let resolved = go_name::qualify_method(
            package.as_deref(),
            type_name,
            method,
            self.facts.current_package(),
            is_public,
            computed_alias.as_deref(),
        );
        if let Some(package) = resolved.package {
            self.require_generated_package(package);
        }
        resolved.name
    }

    pub(crate) fn resolve_variant(&mut self, identifier: &str, enum_id: &str) -> String {
        let enum_package = self
            .facts
            .package_for_qualified_name(enum_id)
            .unwrap_or(enum_id);
        let computed_alias = if self.facts.is_foreign_package(enum_package) {
            Some(self.require_package_import(enum_package))
        } else {
            None
        };
        let resolved = go_name::variant_by_id(
            identifier,
            enum_id,
            enum_package,
            self.facts.current_package(),
            computed_alias.as_deref(),
        );
        if let Some(package) = resolved.package {
            self.require_generated_package(package);
        }
        resolved.name
    }
}
