use crate::Planner;
use crate::ir::go_expression::GoExpressionNode;
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::go_name::{self, ResolvedName};
use crate::names::packages::PackageUse;
use crate::render::imports::rendered_qualifier;
use crate::state::bindings::BindingValue;
use syntax::ast::IdentifierResolution;
use syntax::program::{self, Definition};
use syntax::types::{Type, unqualified_name};

impl Planner<'_> {
    pub(crate) fn resolve_go_name(&self, name: &str) -> GoExpression {
        go_name::resolve(name).into_expression()
    }

    pub(crate) fn definition_reference(&self, symbol: &str) -> GoExpression {
        let package = self
            .facts
            .package_for_qualified_name(symbol)
            .expect("definition symbols are package-qualified");
        let name = &symbol[package.len() + 1..];
        if let Some((owner, method)) = name.rsplit_once('.') {
            let owner_id = self.peel_alias_id(&format!("{package}.{owner}"));
            let is_public = match self.facts.method(&owner_id, method) {
                Some(method) => method.visibility.is_public(),
                None => self
                    .facts
                    .definition(&format!("{owner_id}.{method}"))
                    .is_some_and(|definition| definition.visibility.is_public()),
            };
            let mut expression = self.qualify_method_call(&owner_id, method, is_public);
            if let GoExpressionNode::Identifier(identifier) = expression.node_mut() {
                *identifier = GoIdentifier::external(identifier.spelling().to_string());
            }
            return expression;
        }
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
        GoExpression::qualified(
            self.package_use_for_package(package),
            self.package_member_go_name(package, name),
        )
    }

    pub(crate) fn package_member_go_name(&self, package: &str, member: &str) -> String {
        if go_name::is_go_import(package) {
            member.to_string()
        } else if self.facts.is_const(&format!("{package}.{member}")) {
            go_name::screaming_snake_to_camel(member)
        } else {
            go_name::snake_to_camel(member)
        }
    }

    pub(crate) fn reference_go_identifier(
        &self,
        lisette_name: &str,
        resolution: &IdentifierResolution,
    ) -> GoIdentifier {
        if let IdentifierResolution::Definition { name, .. } = resolution {
            return GoIdentifier::external(self.definition_reference(name).to_string());
        }
        match self
            .scope
            .resolve_identifier_with_resolution(lisette_name, resolution)
        {
            Some(BindingValue::GoName(name) | BindingValue::GoConst(name)) => name.clone(),
            _ => {
                assert!(
                    resolution.binding_id().is_none(),
                    "source binding has no emitted identifier: {lisette_name}"
                );
                GoIdentifier::external(go_name::escape_reserved(lisette_name).into_owned())
            }
        }
    }

    pub(crate) fn canonical_package(&self, package: &str) -> String {
        self.namespace
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
            self.package
                .fresh_package_name(&format!("{qualifier}_pkg"), |candidate| {
                    self.scope.has_binding_for_go_name(candidate)
                        || self.scope.is_go_name_declared(candidate)
                })
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
                go_name::associated_function_go_name(type_name, method, true),
                self.package_use_for_package(package),
            ),
            _ => ResolvedName::local(go_name::associated_function_go_name(
                type_name, method, is_public,
            )),
        };
        resolved.into_expression()
    }

    pub(crate) fn resolve_variant(&self, identifier: &str, enum_id: &str) -> GoExpression {
        let enum_name = unqualified_name(enum_id);
        let variant_name = unqualified_name(identifier);
        let tag_constant = go_name::enum_tag_constant(enum_name, variant_name);
        if go_name::is_prelude_id(enum_id) {
            return ResolvedName::stdlib(tag_constant).into_expression();
        }
        let enum_package = self
            .facts
            .package_for_qualified_name(enum_id)
            .unwrap_or(enum_id);
        let resolved = if self.facts.is_current_package(enum_package) {
            ResolvedName::local(tag_constant)
        } else {
            ResolvedName::foreign(tag_constant, self.package_use_for_package(enum_package))
        };
        resolved.into_expression()
    }
}

impl Planner<'_> {
    pub(crate) fn method_go_name(&self, method: &str, is_public: bool) -> String {
        if is_public || self.method_needs_export(method) {
            go_name::snake_to_camel(method)
        } else {
            go_name::unexported_method_go_name(method)
        }
    }

    pub(crate) fn interface_method_go_name(&self, method: &str, is_public: bool) -> String {
        let name = self.method_go_name(method, is_public);
        if self.facts.method_uses_tagged_return(method) {
            go_name::tagged_twin(&name)
        } else {
            name
        }
    }

    pub(crate) fn free_function_go_name(&self, name: &str, is_public: bool) -> String {
        if is_public {
            go_name::snake_to_camel(name)
        } else {
            self.package
                .escape_remap(name)
                .map(str::to_string)
                .unwrap_or_else(|| go_name::escape_reserved(name).into_owned())
        }
    }
}
