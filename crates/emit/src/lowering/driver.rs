use crate::OutputFile;
use crate::lookup::facts::context::LineIndex;
use crate::lookup::facts::context::{GlobalEmitData, SharedEmitContext};
use crate::lookup::facts::definition_key::DeclaredName;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::adapter_registry::AdapterRegistry;
use crate::lookup::state::conversion_record::ConversionRecord;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lookup::state::imports::generated_qualifier_collisions;
use crate::lowering::abi::slot_coercion::check_applied_conversions;
use crate::lowering::planner::Planner;
use crate::lowering::planner::package::{FilePlan, PackagePlan, build_package_plan};
use crate::output::render::declarations::render_declaration;
use crate::output::render::output_file::OutputCollector;
use crate::target::types::go_type::is_prelude_type;
use diagnostics::LisetteDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
use std::mem;
use std::path::PathBuf;
use syntax::ast::Expression;
use syntax::program::{EmitInput, File};

#[derive(Clone, Debug)]
pub struct EmitOptions {
    pub sourcemap: bool,
    pub sourcemap_base: Option<PathBuf>,
    pub emit_tests: bool,
}

impl<'a> Planner<'a> {
    pub fn emit(
        analysis: &'a mut EmitInput,
        go_module: &str,
        entry_package_name: &'a str,
        options: EmitOptions,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        Self::lower(
            analysis,
            go_module,
            entry_package_name,
            options,
            &Lowering::Everything,
        )
    }

    pub fn check(
        analysis: &'a mut EmitInput,
        go_module: &str,
        entry_package_name: &'a str,
    ) -> Result<(), Vec<LisetteDiagnostic>> {
        let files = analysis.files_with_unspellable_types.clone();
        let options = EmitOptions {
            sourcemap: false,
            sourcemap_base: None,
            emit_tests: false,
        };
        Self::lower(
            analysis,
            go_module,
            entry_package_name,
            options,
            &Lowering::Files(files),
        )
        .map(drop)
    }

    fn lower(
        analysis: &'a mut EmitInput,
        go_module: &str,
        entry_package_name: &'a str,
        options: EmitOptions,
        lowering: &Lowering,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        strip_emitted_wrappers(analysis, lowering);
        let analysis: &'a EmitInput = analysis;

        let shared = SharedEmitContext {
            go_module,
            entry_package_name,
            emit_tests: options.emit_tests,
            line_indexes: source_line_indexes(analysis, &options),
            globals: GlobalEmitData::new(&analysis.definitions, &analysis.go_method_policy),
        };
        let work = package_work(analysis);

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |(package_id, files): &(&str, Vec<&File>)| {
            emit_package(analysis, &shared, package_id, files, lowering)
        };

        let package_outputs: Vec<Result<Vec<OutputFile>, Vec<LisetteDiagnostic>>> =
            if work.len() < PARALLEL_THRESHOLD {
                work.iter().map(emit_one).collect()
            } else {
                use rayon::prelude::*;
                work.par_iter().map(emit_one).collect()
            };

        merge_package_outputs(package_outputs)
    }

    fn emit_files_with_facts(
        facts: EmitFacts<'a>,
        files: &[&File],
        lowering: &Lowering,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let PackagePlan {
            package_name,
            names,
            collision_diagnostics,
            files: mut file_plans,
        } = build_package_plan(&facts, files);
        file_plans.retain(|plan| lowering.lowers_file(plan.file.id));

        let no_collisions = vec![FxHashSet::default(); file_plans.len()];
        let mut lowered = lower_files(&facts, &names, &package_name, &file_plans, &no_collisions);
        if lowered
            .generated_collisions
            .iter()
            .any(|collisions| !collisions.is_empty())
        {
            let collisions = mem::take(&mut lowered.generated_collisions);
            lowered = lower_files(&facts, &names, &package_name, &file_plans, &collisions);
        }

        let (applied, dropped_code) = lowered.conversions.recorded();
        check_applied_conversions(&facts, files, applied, dropped_code);
        let mut all_diagnostics = collision_diagnostics;
        all_diagnostics.append(&mut lowered.diagnostics);
        if all_diagnostics.is_empty() {
            Ok(lowered.output_files)
        } else {
            Err(all_diagnostics)
        }
    }

    fn render_file_source(&mut self, file: &File) -> OutputCollector {
        let mut declarations = Vec::new();
        for expression in &file.items {
            let Expression::Enum { name, variants, .. } = expression else {
                continue;
            };
            if is_prelude_type(name) {
                continue;
            }
            let enum_id = self
                .facts
                .qualified_current(DeclaredName::declared(expression));
            for variant in variants {
                let mut declaration = self.create_make_function_code(&enum_id, &variant.name);
                declaration.position = self.facts.source_position(&variant.name_span);
                declarations.push(declaration);
            }
        }
        for expression in &file.items {
            declarations.extend(self.with_declaration_scope(|this| this.emit_top_item(expression)));
        }
        declarations.extend(self.namespace.take_adapters());
        for declaration in &declarations {
            self.collect_declaration_imports(declaration);
        }
        let mut source = OutputCollector::new();
        for declaration in &declarations {
            source.collect_with_blank(render_declaration(declaration));
        }
        source
    }
}

fn strip_emitted_wrappers(analysis: &mut EmitInput, lowering: &Lowering) {
    let cached_packages = &analysis.cached_packages;
    for file in analysis.files.values_mut() {
        if !is_emitted(file, cached_packages) || !lowering.lowers_file(file.id) {
            continue;
        }
        for item in &mut file.items {
            item.strip_wrappers();
        }
        debug_assert!(
            !file.items.iter().any(Expression::contains_wrapper),
            "internal error: a parenthesized expression, a single-expression block or a reference read once reached emit"
        );
    }
}

fn source_line_indexes(
    analysis: &EmitInput,
    options: &EmitOptions,
) -> Option<FxHashMap<u32, LineIndex>> {
    options.sourcemap.then(|| {
        analysis
            .files
            .iter()
            .filter(|(_, file)| !file.is_d_lis())
            .map(|(file_id, file)| {
                let path = match &options.sourcemap_base {
                    Some(base) => base.join(&file.display_path).display().to_string(),
                    None => file.display_path.clone(),
                };
                (*file_id, LineIndex::from_source(path, &file.source))
            })
            .collect()
    })
}

fn package_work(analysis: &EmitInput) -> Vec<(&str, Vec<&File>)> {
    let mut files_by_package: FxHashMap<&str, Vec<&File>> = FxHashMap::default();
    for file in analysis.files.values() {
        if is_emitted(file, &analysis.cached_packages) {
            files_by_package
                .entry(file.package_id.as_str())
                .or_default()
                .push(file);
        }
    }
    for files in files_by_package.values_mut() {
        files.sort_unstable_by_key(|file| file.id);
    }
    let mut work: Vec<_> = files_by_package.into_iter().collect();
    work.sort_unstable_by_key(|(package_id, _)| *package_id);
    work
}

fn merge_package_outputs(
    package_outputs: Vec<Result<Vec<OutputFile>, Vec<LisetteDiagnostic>>>,
) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    for package_output in package_outputs {
        match package_output {
            Ok(mut files) => output.append(&mut files),
            Err(mut errors) => diagnostics.append(&mut errors),
        }
    }

    if diagnostics.is_empty() {
        output.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(output)
    } else {
        diagnostics.sort_by(LisetteDiagnostic::sort_key);
        Err(diagnostics)
    }
}

struct LoweredFiles {
    output_files: Vec<OutputFile>,
    diagnostics: Vec<LisetteDiagnostic>,
    conversions: ConversionRecord,
    generated_collisions: Vec<FxHashSet<String>>,
}

fn lower_files<'a>(
    facts: &EmitFacts<'a>,
    names: &PackageNames,
    package_name: &str,
    file_plans: &[FilePlan<'_>],
    generated_collisions: &[FxHashSet<String>],
) -> LoweredFiles {
    let mut adapters = AdapterRegistry::default();
    let mut lowered = LoweredFiles {
        output_files: Vec::new(),
        diagnostics: Vec::new(),
        conversions: ConversionRecord::default(),
        generated_collisions: Vec::new(),
    };
    for (plan, collisions) in file_plans.iter().zip(generated_collisions) {
        let file = plan.file;
        let mut imports = plan.imports.clone();
        imports.rename_generated_collisions(collisions, names);
        let mut namespace = FileNamespace::new(imports, collisions.clone());
        let source = Planner::new(
            facts.clone(),
            names,
            &mut lowered.conversions,
            &mut adapters,
            &mut namespace,
        )
        .render_file_source(file);

        let source = source.render();
        let (imports, mut diagnostics) = namespace.finish();
        lowered
            .generated_collisions
            .push(generated_qualifier_collisions(&imports));
        lowered.diagnostics.append(&mut diagnostics);
        lowered.output_files.push(OutputFile {
            name: file.go_filename(),
            imports,
            source,
            package_name: package_name.to_string(),
            file_comment: file.file_comment.clone(),
        });
    }
    lowered
}

enum Lowering {
    Everything,
    Files(FxHashSet<u32>),
}

impl Lowering {
    fn lowers_file(&self, file_id: u32) -> bool {
        match self {
            Self::Everything => true,
            Self::Files(files) => files.contains(&file_id),
        }
    }
}

fn emit_package<'a>(
    analysis: &'a EmitInput,
    shared: &'a SharedEmitContext<'a>,
    package_id: &str,
    files: &[&'a File],
    lowering: &Lowering,
) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
    let facts = EmitFacts::new(analysis, shared, package_id.to_string().into());
    let result = Planner::emit_files_with_facts(facts, files, lowering);
    if let Err(diagnostics) = &result {
        debug_assert!(
            diagnostics
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic.code_str(),
                    Some("emit.unnamable_go_type" | "emit.type_without_go_form")
                ))
                .all(
                    |diagnostic| diagnostic.file_id().is_some_and(|file_id| analysis
                        .files_with_unspellable_types
                        .contains(&file_id))
                ),
            "internal error: the checker did not flag a file that spells a type with no Go form"
        );
    }
    result.map(|mut package_output| {
        if package_id != analysis.entry_package_id.as_str() {
            for file in &mut package_output {
                file.name = format!("{}/{}", package_id, file.name);
            }
        }
        package_output
    })
}

fn is_emitted(file: &File, cached_packages: &FxHashSet<String>) -> bool {
    !file.is_d_lis() && !cached_packages.contains(&file.package_id)
}
