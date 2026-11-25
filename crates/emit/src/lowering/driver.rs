use crate::OutputFile;
use crate::lookup::facts::context::LineIndex;
use crate::lookup::facts::context::{GlobalEmitData, SharedEmitContext};
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::state::adapter_registry::AdapterRegistry;
use crate::lookup::state::conversion_record::ConversionRecord;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lowering::abi::slot_coercion::check_applied_conversions;
use crate::lowering::planner::Planner;
use crate::lowering::planner::package::{PackagePlan, build_package_plan};
use crate::output::render::Renderer;
use crate::output::render::output::OutputCollector;
use crate::target::types::go_type::is_prelude_type;
use diagnostics::LisetteDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
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
        let cached_packages = &analysis.cached_packages;
        for file in analysis.files.values_mut() {
            if !is_emitted(file, cached_packages) {
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
        let analysis: &'a EmitInput = analysis;

        let line_indexes = options.sourcemap.then(|| {
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
        });

        let shared = SharedEmitContext {
            go_module,
            entry_package_name,
            emit_tests: options.emit_tests,
            line_indexes,
            globals: GlobalEmitData::compute(&analysis.definitions),
        };

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

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |(package_id, files): &(&str, Vec<&File>)| {
            emit_package(analysis, &shared, package_id, files)
        };

        let package_outputs: Vec<Result<Vec<OutputFile>, Vec<LisetteDiagnostic>>> =
            if work.len() < PARALLEL_THRESHOLD {
                work.iter().map(emit_one).collect()
            } else {
                use rayon::prelude::*;
                work.par_iter().map(emit_one).collect()
            };

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

    fn emit_files_with_facts(
        facts: EmitFacts<'a>,
        files: &[&File],
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let PackagePlan {
            package_name,
            names,
            collision_diagnostics,
            files: file_plans,
        } = build_package_plan(&facts, files);

        let mut conversions = ConversionRecord::default();
        let mut adapters = AdapterRegistry::default();
        let mut output_files = Vec::new();
        let mut all_diagnostics = collision_diagnostics;

        for plan in file_plans {
            let file = plan.file;
            let mut namespace = FileNamespace::new(plan.imports);
            let source = Planner::new(
                facts.clone(),
                &names,
                &mut conversions,
                &mut adapters,
                &mut namespace,
            )
            .render_file_source(file);

            let source = source.render();
            let (imports, mut diagnostics) = namespace.finish();
            all_diagnostics.append(&mut diagnostics);
            output_files.push(OutputFile {
                name: file.go_filename(),
                imports,
                source,
                package_name: package_name.clone(),
                file_comment: file.file_comment.clone(),
            });
        }

        let (applied, dropped_code) = conversions.recorded();
        check_applied_conversions(&facts, files, applied, dropped_code);
        if all_diagnostics.is_empty() {
            Ok(output_files)
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
            let enum_id = self.facts.qualified_current(name);
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
            source.collect_with_blank(Renderer.render_declaration(declaration));
        }
        source
    }
}

fn emit_package<'a>(
    analysis: &'a EmitInput,
    shared: &'a SharedEmitContext<'a>,
    package_id: &str,
    files: &[&'a File],
) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
    let facts = EmitFacts::new(analysis, shared, package_id.to_string().into());
    Planner::emit_files_with_facts(facts, files).map(|mut package_output| {
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
