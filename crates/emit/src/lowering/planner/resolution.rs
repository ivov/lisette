use crate::Planner;
use crate::lookup::state::bindings::BindingValue;
use crate::lookup::state::imports::rendered_qualifier;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::packages::PackageUse;
use syntax::ast::IdentifierResolution;
use syntax::program::{self, Definition};
use syntax::types::{Type, unqualified_name};

impl Planner<'_> {
    pub(in crate::lowering) fn definition_reference(&self, symbol: &str) -> GoExpression {
        let package = self
            .facts
            .package_for_qualified_name(symbol)
            .expect("definition symbols are package-qualified");
        let name = &symbol[package.len() + 1..];
        if let Some((owner, method)) = name.rsplit_once('.') {
            let owner_id = format!("{package}.{owner}");
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
            return self.package_member_reference(package, go_name);
        }
        self.package_member_reference(package, self.facts.package_member_go_name(package, name))
    }

    pub(in crate::lowering) fn reference_go_identifier(
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

    pub(in crate::lowering) fn canonical_package(&self, package: &str) -> String {
        self.namespace
            .package_for_alias(package)
            .unwrap_or(package)
            .to_string()
    }

    pub(in crate::lowering) fn package_use_for_package(&self, package: &str) -> PackageUse {
        if package == go_name::TEST_PRELUDE_PACKAGE {
            return PackageUse::generated(go_name::GeneratedPackage::TestKit);
        }
        let path = self.facts.go_import_path(package);
        let qualifier = match self.namespace.import_qualifier(package) {
            Some(qualifier) => qualifier.to_string(),
            None => {
                let name = program::unaliased_binding_name(package, self.facts.go_package_names());
                rendered_qualifier(&path, name.to_string())
            }
        };
        let qualifier = if self.scope.has_binding_for_go_name(&qualifier)
            || self.scope.is_go_name_declared(&qualifier)
            || self.names.declares_name(&qualifier)
        {
            self.names
                .fresh_package_name(&format!("{qualifier}_pkg"), |candidate| {
                    self.scope.has_binding_for_go_name(candidate)
                        || self.scope.is_go_name_declared(candidate)
                })
        } else {
            qualifier
        };
        PackageUse::new(path, qualifier)
    }

    pub(in crate::lowering) fn package_member_reference(
        &self,
        package: &str,
        go_name: String,
    ) -> GoExpression {
        if package == go_name::PRELUDE_PACKAGE {
            return GoExpression::qualified(
                PackageUse::generated(go_name::GeneratedPackage::Prelude),
                go_name,
            );
        }
        if self.facts.is_current_package(package) {
            return GoExpression::external_name(go_name);
        }
        GoExpression::qualified(self.package_use_for_package(package), go_name)
    }

    pub(in crate::lowering) fn variant_make_reference(&self, symbol: &str) -> Option<GoExpression> {
        let (enum_id, make_function) = self.facts.variant_make_function(symbol)?;
        let package = self.facts.package_for_qualified_name(enum_id)?;
        Some(self.package_member_reference(package, make_function))
    }

    pub(in crate::lowering) fn qualify_method_call(
        &self,
        type_id: &str,
        method: &str,
        is_public: bool,
    ) -> GoExpression {
        let type_name = unqualified_name(type_id);
        match self.facts.package_for_qualified_name(type_id) {
            Some(go_name::PRELUDE_PACKAGE) => self.package_member_reference(
                go_name::PRELUDE_PACKAGE,
                format!("{}{}", type_name, go_name::snake_to_camel(method)),
            ),
            Some(package) if self.facts.is_foreign_package(package) => self
                .package_member_reference(
                    package,
                    go_name::associated_function_go_name(type_name, method, true),
                ),
            _ => GoExpression::external_name(go_name::associated_function_go_name(
                type_name, method, is_public,
            )),
        }
    }

    pub(in crate::lowering) fn resolve_variant(
        &self,
        identifier: &str,
        enum_id: &str,
    ) -> GoExpression {
        let enum_name = unqualified_name(enum_id);
        let variant_name = unqualified_name(identifier);
        let tag_constant = go_name::enum_tag_constant(enum_name, variant_name);
        let enum_package = if go_name::is_prelude_id(enum_id) {
            go_name::PRELUDE_PACKAGE
        } else {
            self.facts
                .package_for_qualified_name(enum_id)
                .unwrap_or(enum_id)
        };
        self.package_member_reference(enum_package, tag_constant)
    }

    pub(in crate::lowering) fn free_function_go_name(&self, name: &str, is_public: bool) -> String {
        if is_public {
            go_name::snake_to_camel(name)
        } else {
            self.names
                .escape_remap(name)
                .map(str::to_string)
                .unwrap_or_else(|| go_name::escape_reserved(name).into_owned())
        }
    }
}
