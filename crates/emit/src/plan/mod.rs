pub(crate) mod bodies;
pub(crate) mod calls;
pub(crate) mod cleanup;
pub(crate) mod evaluation;
pub(crate) mod go_expression;
pub(crate) mod local;
pub(crate) mod lower;
pub(crate) mod placement;
pub(crate) mod source_reuse;
pub(crate) mod values;
pub(crate) mod verify;
pub(crate) mod visit;

use crate::Planner;
use crate::names::go_name;
use crate::output::imports::ImportPlan;
use diagnostics::LisetteDiagnostic;
use syntax::ast::Span;
use syntax::program::File;

pub(crate) struct PackagePlan<'a> {
    pub(crate) package_name: String,
    pub(crate) collision_diagnostics: Vec<LisetteDiagnostic>,
    pub(crate) files: Vec<FilePlan<'a>>,
}

pub(crate) struct FilePlan<'a> {
    pub(crate) file: &'a File,
    pub(crate) imports: ImportPlan,
}

impl Planner<'_> {
    /// Resolve package-wide names and collisions before any item is rendered.
    pub(crate) fn build_package_plan<'f>(&mut self, files: &[&'f File]) -> PackagePlan<'f> {
        self.collect_escape_remap(files);
        let collected = self.collect_names(files);
        let file_plans: Vec<_> = files
            .iter()
            .map(|file| FilePlan {
                file,
                imports: ImportPlan::build(
                    file,
                    self.facts.go_module(),
                    self.facts.go_package_names(),
                ),
            })
            .collect();
        let import_qualifiers: Vec<(String, Span)> = file_plans
            .iter()
            .flat_map(|plan| {
                plan.imports
                    .used_qualifiers(|span| self.facts.is_unused(span))
            })
            .collect();
        self.package.record_package_block_names(
            collected.declared_names(),
            import_qualifiers.iter().map(|(q, _)| q.clone()).collect(),
        );
        self.derive_package_go_consts(files);
        self.collect_generic_renames(files);
        let collision_diagnostics =
            self.name_collision_diagnostics(files, collected, &import_qualifiers);

        let package_id = self.facts.current_package();
        let package_name = if self.facts.is_entry_package(package_id) {
            self.facts.entry_package_name().to_string()
        } else {
            let raw = package_id.rsplit('/').next().unwrap_or(package_id);
            go_name::sanitize_package_name(raw).into_owned()
        };

        PackagePlan {
            package_name,
            collision_diagnostics,
            files: file_plans,
        }
    }
}
