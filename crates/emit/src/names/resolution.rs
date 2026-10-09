use syntax::ast::IdentifierResolution;
use syntax::types::{Type, unqualified_name};

use crate::Planner;
use crate::names::go_name::{self, ResolvedName};
use crate::names::packages::PackageUse;
use crate::output::imports::rendered_qualifier;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use syntax::program::{self, Definition};

impl Planner<'_> {
    /// A name the checker did not resolve to a definition.
    pub(crate) fn resolve_go_name(&self, name: &str, locally_bound: bool) -> GoExpression {
        let mut expression = go_name::resolve(name).into_expression();
        if let GoExpressionNode::Identifier(identifier) = expression.node_mut() {
            *identifier = if locally_bound {
                GoIdentifier::name(identifier.spelling().to_string())
            } else {
                GoIdentifier::external(identifier.spelling().to_string())
            };
        }
        expression
    }

    pub(crate) fn definition_reference(&self, symbol: &str) -> GoExpression {
        let package = self
            .facts
            .package_for_qualified_name(symbol)
            .expect("definition symbols are package-qualified");
        let name = &symbol[package.len() + 1..];
        if let Some((owner, method)) = name.rsplit_once('.') {
            let owner_id = self.peel_alias_id(&format!("{package}.{owner}"));
            let is_public = self
                .facts
                .method(&owner_id, method)
                .is_some_and(|method| method.visibility.is_public())
                || self.method_needs_export(method);
            let mut expression = self.qualify_method_call(&owner_id, method, is_public);
            if let GoExpressionNode::Identifier(identifier) = expression.node_mut() {
                *identifier = GoIdentifier::external(identifier.spelling().to_string());
            }
            return expression;
        }
        // Prelude functions re-expose the Go builtins under the same name.
        if package == go_name::PRELUDE_PACKAGE {
            return GoExpression::external_name(name.to_string());
        }
        let definition = self.facts.definition(symbol);
        let is_const = definition.is_some_and(Definition::is_const);
        if self.facts.is_current_package(package) {
            let go_name = if is_const {
                go_name::screaming_snake_to_camel(name)
            } else {
                let is_public = definition.is_some_and(|definition| {
                    definition.visibility.is_public()
                        && matches!(definition.ty.unwrap_forall(), Type::Function(_))
                });
                self.free_function_go_name(name, is_public)
            };
            return GoExpression::external_name(go_name);
        }
        let member = if go_name::is_go_import(package) {
            name.to_string()
        } else if is_const {
            go_name::screaming_snake_to_camel(name)
        } else {
            go_name::snake_to_camel(name)
        };
        GoExpression::qualified(self.package_use_for_package(package), member)
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

    pub(crate) fn reference_go_name(
        &self,
        lisette_name: &str,
        resolution: &IdentifierResolution,
    ) -> String {
        if let IdentifierResolution::Definition { name, .. } = resolution {
            return self.definition_reference(name).to_string();
        }
        if let Some(bound) = self.scope.resolve_binding_go_name(lisette_name) {
            return bound.to_string();
        }
        go_name::escape_reserved(lisette_name).into_owned()
    }

    pub(crate) fn canonical_package(&self, package: &str) -> String {
        self.namespace
            .package_for_alias(package)
            .unwrap_or(package)
            .to_string()
    }

    /// The qualifier is the one the import renders: sanitized default names, aliases as written.
    pub(crate) fn package_use_for_package(&self, package: &str) -> PackageUse {
        if package == go_name::TEST_PRELUDE_PACKAGE {
            return PackageUse::generated(go_name::GeneratedPackage::TestKit);
        }
        let path = match package.strip_prefix(go_name::GO_IMPORT_PREFIX) {
            Some(rest) => rest.to_string(),
            None => self.facts.go_import_path(package),
        };
        let qualifier = match self.namespace.import_qualifier(package) {
            Some(qualifier) => qualifier.to_string(),
            None => {
                let name = self
                    .facts
                    .go_package_name(package)
                    .map(str::to_string)
                    .unwrap_or_else(|| match package.strip_prefix(go_name::GO_IMPORT_PREFIX) {
                        Some(go_path) => program::go_import_default_name(go_path).to_string(),
                        None => go_name::go_package_name(package).to_string(),
                    });
                rendered_qualifier(&path, name)
            }
        };
        let qualifier = if self.scope.has_binding_for_go_name(&qualifier)
            || self.scope.is_go_name_declared(&qualifier)
            || self.package.declares_name(&qualifier)
        {
            format!("{qualifier}_pkg")
        } else {
            qualifier
        };
        PackageUse::new(path, qualifier)
    }

    pub(crate) fn qualify_method_call(
        &self,
        type_id: &str,
        method: &str,
        is_public: bool,
    ) -> GoExpression {
        let type_name = unqualified_name(type_id);
        let resolved = match self.facts.package_for_qualified_name(type_id) {
            Some(go_name::PRELUDE_PACKAGE) => {
                ResolvedName::stdlib(format!("{}{}", type_name, go_name::snake_to_camel(method)))
            }
            Some(package) if self.facts.is_foreign_package(package) => ResolvedName::foreign(
                format!("{}_{}", type_name, go_name::snake_to_camel(method)),
                self.package_use_for_package(package),
            ),
            _ => ResolvedName::local(format!(
                "{}_{}",
                type_name,
                go_name::free_method_part(method, is_public)
            )),
        };
        resolved.into_expression()
    }

    pub(crate) fn resolve_variant(&self, identifier: &str, enum_id: &str) -> GoExpression {
        let enum_name = unqualified_name(enum_id);
        let variant_name = unqualified_name(identifier);
        if enum_id.starts_with(go_name::PRELUDE_PREFIX) {
            return ResolvedName::stdlib(format!("{enum_name}{variant_name}")).into_expression();
        }
        let enum_package = self
            .facts
            .package_for_qualified_name(enum_id)
            .unwrap_or(enum_id);
        let tag_constant = go_name::enum_tag_constant(enum_name, variant_name);
        let resolved = if self.facts.is_current_package(enum_package) {
            ResolvedName::local(tag_constant)
        } else {
            ResolvedName::foreign(tag_constant, self.package_use_for_package(enum_package))
        };
        resolved.into_expression()
    }
}
