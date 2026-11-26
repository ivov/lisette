use crate::Planner;
use crate::lookup::state::imports::rendered_qualifier;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::{self, GoExport};
use crate::target::names::packages::{PackageOrigin, PackageUse};
use syntax::program::{self, PRELUDE_PACKAGE_ID, is_go_import, is_prelude_symbol};
use syntax::types::unqualified_name;

impl Planner<'_> {
    pub(in crate::lowering) fn canonical_package(&self, package: &str) -> String {
        self.namespace
            .package_for_alias(package)
            .unwrap_or(package)
            .to_string()
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn definition_reference(&self, symbol: &str) -> GoExpression {
        let package = self
            .facts
            .package_for_qualified_name(symbol)
            .expect("definition symbols are package-qualified");
        let name = &symbol[package.len() + 1..];
        if let Some((owner, method)) = name.rsplit_once('.') {
            let owner_id = format!("{package}.{owner}");
            let export = GoExport::from_visibility(*self.facts.callable(symbol).visibility());

            let mut expression = self.qualify_method_call(&owner_id, method, export);
            if let GoExpressionNode::Identifier(identifier) = expression.node_mut() {
                *identifier = GoIdentifier::external(identifier.spelling().to_string());
            }
            return expression;
        }
        if package == PRELUDE_PACKAGE_ID {
            return GoExpression::external_name(name.to_string());
        }
        let definition = self.facts.definition(symbol);
        let is_const = definition.is_const();
        if self.facts.is_current_package(package) {
            let go_name = if is_const {
                go_name::screaming_snake_to_camel(name)
            } else {
                let export = if definition.ty.as_function_type().is_some() {
                    GoExport::from_visibility(definition.visibility)
                } else {
                    GoExport::Unexported
                };

                self.names.free_function_go_name(name, export)
            };

            return self.package_member_reference(package, go_name);
        }
        self.package_member_reference(package, self.facts.package_member_go_name(package, name))
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

        let origin = if is_go_import(package) {
            PackageOrigin::GoImport
        } else {
            PackageOrigin::Lisette
        };
        PackageUse::new(path, qualifier, origin)
    }

    pub(in crate::lowering) fn package_member_reference(
        &self,
        package: &str,
        go_name: String,
    ) -> GoExpression {
        if package == PRELUDE_PACKAGE_ID {
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
        export: GoExport,
    ) -> GoExpression {
        let type_name = unqualified_name(type_id);
        match self.facts.package_for_qualified_name(type_id) {
            Some(PRELUDE_PACKAGE_ID) => self.package_member_reference(
                PRELUDE_PACKAGE_ID,
                format!("{}{}", type_name, go_name::snake_to_camel(method)),
            ),
            Some(package) if self.facts.is_foreign_package(package) => self
                .package_member_reference(
                    package,
                    go_name::associated_function_go_name(type_name, method, GoExport::Exported),
                ),
            _ => GoExpression::external_name(go_name::associated_function_go_name(
                type_name, method, export,
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
        let enum_package = if is_prelude_symbol(enum_id) {
            PRELUDE_PACKAGE_ID
        } else {
            self.facts
                .package_for_qualified_name(enum_id)
                .expect("an enum symbol is package-qualified")
        };

        self.package_member_reference(enum_package, tag_constant)
    }
}
