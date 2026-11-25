use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::adapter_registry::AdapterRegistry;
use crate::lookup::state::conversion_record::ConversionRecord;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lookup::state::imports::ImportPlan;
use crate::lowering::planner::collectors::{collect_escape_remap, collect_generic_renames};
use crate::target::names::go_name;
use diagnostics::LisetteDiagnostic;
use rustc_hash::FxHashSet;
use syntax::ast::Span;
use syntax::program::File;

pub(in crate::lowering) struct PackagePlan<'a> {
    pub(in crate::lowering) package_name: String,
    pub(in crate::lowering) names: PackageNames,
    pub(in crate::lowering) collision_diagnostics: Vec<LisetteDiagnostic>,
    pub(in crate::lowering) files: Vec<FilePlan<'a>>,
}

pub(in crate::lowering) struct FilePlan<'a> {
    pub(in crate::lowering) file: &'a File,
    pub(in crate::lowering) imports: ImportPlan,
}

pub(in crate::lowering) fn build_package_plan<'f>(
    facts: &EmitFacts<'_>,
    files: &[&'f File],
) -> PackagePlan<'f> {
    let file_plans: Vec<_> = files
        .iter()
        .map(|file| FilePlan {
            file,
            imports: ImportPlan::build(file, facts.go_module(), facts.go_package_names()),
        })
        .collect();

    let import_qualifiers: Vec<(String, Span)> = file_plans
        .iter()
        .flat_map(|plan| {
            plan.imports
                .used_qualifiers(|key| facts.is_unused_import(key))
        })
        .collect();

    let qualifier_names: FxHashSet<String> = file_plans
        .iter()
        .flat_map(|plan| plan.imports.qualifiers())
        .collect();

    let mut conversions = ConversionRecord::default();
    let mut adapters = AdapterRegistry::default();
    let mut namespace = FileNamespace::default();
    let mut names = PackageNames::default();

    let fixed_names = Planner::new(
        facts.clone(),
        &names,
        &mut conversions,
        &mut adapters,
        &mut namespace,
    )
    .collect_fixed_names(files);

    names.set_block_names(fixed_names, qualifier_names.clone());
    let escape_remap = collect_escape_remap(&names, files);
    names.set_escape_remap(escape_remap);
    let collected = Planner::new(
        facts.clone(),
        &names,
        &mut conversions,
        &mut adapters,
        &mut namespace,
    )
    .collect_names(files);

    names.set_block_names(collected.declared_names(), qualifier_names);
    let generic_renames = collect_generic_renames(&names, files);
    names.set_generic_renames(generic_renames);
    let collision_diagnostics = Planner::new(
        facts.clone(),
        &names,
        &mut conversions,
        &mut adapters,
        &mut namespace,
    )
    .name_collision_diagnostics(files, collected, &import_qualifiers);

    let package_id = facts.current_package();
    let package_name = if facts.is_entry_package(package_id) {
        facts.entry_package_name().to_string()
    } else {
        go_name::sanitize_package_name(go_name::go_package_name(package_id)).into_owned()
    };

    PackagePlan {
        package_name,
        names,
        collision_diagnostics,
        files: file_plans,
    }
}
