mod extract;
mod reference_graph;
mod visibility_constraints;

use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::context::AnalysisContext;
use crate::facts::Facts;
use crate::passes::PARALLEL_THRESHOLD;
use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use syntax::ast::{AttributeArg, Expression, ImportAlias, Span, Visibility};
use syntax::program::Module;
use syntax::program::UnusedInfo;
use syntax::program::{File, FileImport};

use super::Lint as LintEnum;
use super::ast_walk::attributes::SERIALIZATION_KEYS;
use super::from_facts::LintConfig;
use extract::{AliasMap, extract_references, is_upper};
use reference_graph::{
    EnumVariantId, EnumVariantInfo, ItemKind, ModuleItemId, ReferenceGraph, StructFieldId,
    StructFieldInfo,
};
use visibility_constraints::check_visibility_constraints;

struct RefLintResult {
    diagnostics: Vec<LisetteDiagnostic>,
    unused_import_aliases: HashSet<String>,
    unused_definition_spans: Vec<Span>,
}

pub(crate) fn run(
    analysis: &AnalysisContext,
    facts: &Facts,
    unused: &mut UnusedInfo,
    sink: &LocalSink,
) {
    let store = analysis.store;
    let go_package_names = &store.go_package_names;
    let config = LintConfig::default();

    let mut modules: Vec<&Module> = store
        .modules
        .values()
        .filter(|m| !m.is_internal())
        .collect();
    modules.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    if modules.len() < PARALLEL_THRESHOLD {
        for module in &modules {
            apply_ref_lints(module, go_package_names, &config, facts, unused, sink);
        }
        return;
    }

    type WorkerOutput = (LocalSink, UnusedInfo);
    let outputs: Vec<WorkerOutput> = modules
        .par_iter()
        .map(|module| {
            let local_sink = LocalSink::new();
            let mut local_unused = UnusedInfo::default();
            apply_ref_lints(
                module,
                go_package_names,
                &config,
                facts,
                &mut local_unused,
                &local_sink,
            );
            (local_sink, local_unused)
        })
        .collect();

    let mut worker_sinks = Vec::with_capacity(outputs.len());
    for (worker_sink, worker_unused) in outputs {
        worker_sinks.push(worker_sink);
        unused.merge(worker_unused);
    }
    sink.extend(LocalSink::merge(worker_sinks));
}

fn apply_ref_lints(
    module: &Module,
    go_package_names: &HashMap<String, String>,
    config: &LintConfig,
    facts: &Facts,
    unused: &mut UnusedInfo,
    sink: &LocalSink,
) {
    let result = run_ref_lints(module, &module.files, go_package_names, config, facts);
    if !result.unused_import_aliases.is_empty() {
        unused.imports_by_module.insert(
            module.id.clone().into(),
            result
                .unused_import_aliases
                .into_iter()
                .map(|s| s.into())
                .collect(),
        );
    }
    for span in result.unused_definition_spans {
        unused.mark_definition_unused(span);
    }
    let mut diagnostics = result.diagnostics;
    diagnostics.sort_by(LisetteDiagnostic::sort_key);
    sink.extend(diagnostics);
}

fn run_ref_lints(
    module: &Module,
    files: &HashMap<u32, File>,
    go_package_names: &HashMap<String, String>,
    config: &LintConfig,
    facts: &Facts,
) -> RefLintResult {
    let mut diagnostics = Vec::new();
    let mut unused_import_aliases = HashSet::default();
    let mut unused_definition_spans = Vec::new();
    let mut graph = ReferenceGraph::new();

    collect_items(module, files, go_package_names, &mut graph);

    let alias_map = AliasMap::build(module, files, go_package_names);
    for file in files.values() {
        for item in &file.items {
            extract_references(module, item, &mut graph, &alias_map);
        }
    }

    for (method_module_id, method_name) in facts.interface_satisfied_methods.keys() {
        if method_module_id == &module.id {
            let method_id = ModuleItemId::new(method_module_id, method_name);
            graph.mark_as_used(method_id);
        }
    }

    for item_id in graph.get_unreachable() {
        if let Some(info) = graph.get_item(item_id) {
            if info.kind == ItemKind::Import {
                unused_import_aliases.insert(item_id.name.clone());
            }
            if info.kind == ItemKind::Function {
                unused_definition_spans.push(info.span);
            }
            if let Some(diagnostic) = create_unused_diagnostic(info.kind, &info.span, config) {
                diagnostics.push(diagnostic);
            }
        }
    }

    if config.is_enabled(LintEnum::InternalTypeLeak) {
        check_visibility_constraints(module, files, &mut diagnostics);
    }

    if config.is_enabled(LintEnum::UnusedStructField) {
        for (_, field_info) in graph.get_unused_struct_fields() {
            diagnostics.push(diagnostics::lint::unused_field(&field_info.span));
        }
    }

    if config.is_enabled(LintEnum::UnusedEnumVariant) {
        for (_, variant_info) in graph.get_unused_enum_variants() {
            diagnostics.push(diagnostics::lint::unused_variant(&variant_info.span));
        }
    }

    RefLintResult {
        diagnostics,
        unused_import_aliases,
        unused_definition_spans,
    }
}

fn collect_items(
    module: &Module,
    files: &HashMap<u32, File>,
    go_package_names: &HashMap<String, String>,
    graph: &mut ReferenceGraph,
) {
    for file in files.values() {
        for item in &file.items {
            match item {
                Expression::ModuleImport {
                    name,
                    alias,
                    name_span,
                    span,
                } => {
                    if matches!(alias, Some(ImportAlias::Blank(_))) {
                        continue;
                    }

                    let file_import = FileImport {
                        name: name.clone(),
                        name_span: *name_span,
                        alias: alias.clone(),
                        span: *span,
                    };

                    if let Some(effective) = file_import.effective_alias(go_package_names) {
                        let id = ModuleItemId::new(&module.id, &effective);
                        graph.add_import(id, *name_span);
                    }
                }
                Expression::Function {
                    name,
                    name_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, name);
                    let is_entry = *visibility == Visibility::Public || name == "main";
                    graph.add_item(id, *name_span, ItemKind::Function, is_entry);
                }
                Expression::Const {
                    identifier,
                    identifier_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, identifier);
                    graph.add_item(
                        id,
                        *identifier_span,
                        ItemKind::Constant,
                        *visibility == Visibility::Public,
                    );
                }
                Expression::Enum {
                    name,
                    name_span,
                    variants,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, name);
                    let is_public = *visibility == Visibility::Public;
                    graph.add_item(id, *name_span, ItemKind::Type, is_public);

                    for enum_variant in variants {
                        let variant_id = EnumVariantId::new(name, &enum_variant.name);
                        graph.add_enum_variant(
                            variant_id,
                            EnumVariantInfo {
                                span: enum_variant.name_span,
                                parent_is_public: is_public,
                            },
                        );
                    }
                }
                Expression::Struct {
                    name,
                    name_span,
                    fields,
                    attributes,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, name);
                    let is_public = *visibility == Visibility::Public;
                    graph.add_item(id, *name_span, ItemKind::Type, is_public);

                    let has_serialization_attr = attributes.iter().any(|a| {
                        if SERIALIZATION_KEYS.contains(&a.name.as_str()) {
                            return true;
                        }
                        if a.name == "tag" {
                            return match a.args.first() {
                                Some(AttributeArg::String(key)) => {
                                    SERIALIZATION_KEYS.contains(&key.as_str())
                                }
                                Some(AttributeArg::Raw(raw)) => raw
                                    .split(':')
                                    .next()
                                    .is_some_and(|k| SERIALIZATION_KEYS.contains(&k)),
                                _ => false,
                            };
                        }
                        false
                    });

                    for struct_field in fields {
                        let field_id = StructFieldId::new(name, &struct_field.name);
                        let has_tag_attribute =
                            struct_field.attributes.iter().any(|a| a.name == "tag");
                        graph.add_struct_field(
                            field_id,
                            StructFieldInfo {
                                span: struct_field.name_span,
                                parent_is_public: is_public,
                                parent_has_serialization_attr: has_serialization_attr,
                                has_tag_attribute,
                            },
                        );
                    }
                }
                Expression::TypeAlias {
                    name,
                    name_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, name);
                    graph.add_item(
                        id,
                        *name_span,
                        ItemKind::Type,
                        *visibility == Visibility::Public,
                    );
                }
                Expression::Interface {
                    name,
                    name_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(&module.id, name);
                    graph.add_item(
                        id,
                        *name_span,
                        ItemKind::Type,
                        *visibility == Visibility::Public,
                    );
                }
                Expression::ImplBlock { methods, .. } => {
                    for method in methods {
                        if let Expression::Function {
                            name,
                            name_span,
                            visibility,
                            ..
                        } = method
                        {
                            let id = ModuleItemId::new(&module.id, name);
                            let is_entry = *visibility == Visibility::Public
                                || is_upper(name)
                                || matches!(name.as_str(), "string" | "goString" | "error");
                            graph.add_item(id, *name_span, ItemKind::Function, is_entry);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn create_unused_diagnostic(
    kind: ItemKind,
    span: &Span,
    config: &LintConfig,
) -> Option<LisetteDiagnostic> {
    let (lint, diagnostic_fn): (LintEnum, fn(&Span) -> LisetteDiagnostic) = match kind {
        ItemKind::Import => (LintEnum::UnusedImport, diagnostics::lint::unused_import),
        ItemKind::Type => (LintEnum::UnusedType, diagnostics::lint::unused_type),
        ItemKind::Function => (LintEnum::UnusedFunction, diagnostics::lint::unused_function),
        ItemKind::Constant => (LintEnum::UnusedConstant, diagnostics::lint::unused_constant),
    };

    if !config.is_enabled(lint) {
        return None;
    }

    Some(diagnostic_fn(span))
}
