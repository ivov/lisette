mod extract;
mod reference_graph;
mod visibility_constraints;

use rayon::prelude::*;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use crate::passes::PARALLEL_THRESHOLD;
use crate::passes::lints::span_edit::statement_deletion;
use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use diagnostics::{Edit, Fix};
use semantics::context::AnalysisContext;
use semantics::facts::Facts;
use semantics::store::Store;
use syntax::ast::{
    Attribute, AttributeArg, Expression, ImportAlias, Span, StructFieldDefinition, Visibility,
};
use syntax::program::EqualityIndex;
use syntax::program::Module;
use syntax::program::UnusedInfo;
use syntax::program::{File, FileImport};

use extract::{AliasMap, is_upper, walk_expression};
use reference_graph::{EnumVariantId, ItemKind, ModuleItemId, ReferenceGraph, StructFieldId};
use syntax::attributes::SERIALIZATION_KEYS;
use visibility_constraints::check_visibility_constraints;

struct RefLintResult {
    diagnostics: Vec<LisetteDiagnostic>,
    unused_import_aliases: HashSet<String>,
    unused_definition_spans: Vec<Span>,
}

pub(crate) fn run(
    analysis: &AnalysisContext,
    facts: &Facts,
) -> (Vec<LisetteDiagnostic>, UnusedInfo) {
    let store = analysis.store;
    let mut modules: Vec<&Module> = store
        .modules
        .values()
        .map(Arc::as_ref)
        .filter(|m| !m.is_internal())
        .collect();
    modules.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    let mut unused = UnusedInfo::default();

    if modules.len() < PARALLEL_THRESHOLD {
        let sink = LocalSink::new();
        for module in &modules {
            apply_ref_lints(module, facts, store, &mut unused, &sink);
        }
        return (sink.into_diagnostics(), unused);
    }

    type WorkerOutput = (LocalSink, UnusedInfo);
    let outputs: Vec<WorkerOutput> = modules
        .par_iter()
        .map(|module| {
            let local_sink = LocalSink::new();
            let mut local_unused = UnusedInfo::default();
            apply_ref_lints(module, facts, store, &mut local_unused, &local_sink);
            (local_sink, local_unused)
        })
        .collect();

    let mut worker_sinks = Vec::with_capacity(outputs.len());
    for (worker_sink, worker_unused) in outputs {
        worker_sinks.push(worker_sink);
        unused.merge(worker_unused);
    }
    (LocalSink::merge(worker_sinks), unused)
}

fn apply_ref_lints(
    module: &Module,
    facts: &Facts,
    store: &Store,
    unused: &mut UnusedInfo,
    sink: &LocalSink,
) {
    let result = run_ref_lints(module, facts, store);
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
    if !diagnostics.is_empty() {
        let allows: Vec<_> = module
            .source_files()
            .flat_map(|file| super::suppression::collect_declaration_allows(&file.items))
            .collect();
        diagnostics = super::suppression::filter_unused_allowed(diagnostics, &allows);
    }
    diagnostics.sort_by(LisetteDiagnostic::sort_key);
    sink.extend(diagnostics);
}

fn run_ref_lints(module: &Module, facts: &Facts, store: &Store) -> RefLintResult {
    let files = &module.files;
    let equality_index = &store.equality_index;
    let mut diagnostics = Vec::new();
    let mut unused_import_aliases = HashSet::default();
    let mut unused_definition_spans = Vec::new();
    let mut graph = ReferenceGraph::new();

    collect_items(
        files,
        &module.id,
        &store.go_package_names,
        equality_index,
        &mut graph,
    );

    let alias_map = AliasMap::build(files, store);
    for file in files.values().filter(|file| !file.is_d_lis()) {
        for item in &file.items {
            walk_expression(module, item, &mut graph, &alias_map, None);
        }
    }

    for ((method_module_id, method_name), satisfactions) in &facts.interface_satisfied_methods {
        if method_module_id != &module.id {
            continue;
        }
        if method_name == "equals" {
            for satisfaction in satisfactions {
                graph.mark_as_used(ModuleItemId::equals_method(&satisfaction.impl_type_name));
            }
        } else {
            graph.mark_as_used(ModuleItemId::new(method_name));
        }
    }

    for item_id in graph.get_unreachable() {
        if let Some(info) = graph.get_item(item_id) {
            if matches!(info.kind, ItemKind::Import { .. }) {
                unused_import_aliases.insert(item_id.name.to_string());
            }
            if info.kind == ItemKind::Function {
                unused_definition_spans.push(info.span);
            }
            let mut diagnostic = create_unused_diagnostic(info.kind, &info.span);
            if let ItemKind::Import { statement_span } = info.kind
                && let Some(file) = files.get(&statement_span.file_id)
            {
                let deletion = statement_deletion(&file.source, statement_span);
                diagnostic = diagnostic.with_fix(Fix::new(
                    "Remove the unused import",
                    Edit::deletion(deletion),
                ));
            }
            diagnostics.push(diagnostic);
        }
    }

    check_visibility_constraints(module, files, &mut diagnostics);

    for span in graph.get_unused_struct_fields() {
        diagnostics.push(diagnostics::lint::unused_field(span));
    }

    for span in graph.get_unused_enum_variants() {
        diagnostics.push(diagnostics::lint::unused_variant(span));
    }

    RefLintResult {
        diagnostics,
        unused_import_aliases,
        unused_definition_spans,
    }
}

fn collect_items(
    files: &HashMap<u32, File>,
    module_id: &str,
    go_package_names: &HashMap<String, String>,
    equality_index: &EqualityIndex,
    graph: &mut ReferenceGraph,
) {
    for file in files.values().filter(|file| !file.is_d_lis()) {
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
                        let id = ModuleItemId::new(&effective);
                        graph.add_import(id, *name_span, *span);
                    }
                }
                Expression::Function {
                    name,
                    name_span,
                    visibility,
                    attributes,
                    ..
                } => {
                    let id = ModuleItemId::new(name);
                    let is_entry = function_is_entry(name, *visibility, attributes);
                    graph.add_item(id, *name_span, ItemKind::Function, is_entry);
                }
                Expression::Const {
                    identifier,
                    identifier_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(identifier);
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
                    attributes,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(name);
                    let is_public = *visibility == Visibility::Public;
                    graph.add_item(id, *name_span, ItemKind::Type, is_public);

                    let has_serialization_attr = has_serialization_attr(attributes);

                    for enum_variant in variants {
                        let variant_id = EnumVariantId::new(name, &enum_variant.name);
                        if !is_public && !has_serialization_attr {
                            graph.add_enum_variant(variant_id, enum_variant.name_span);
                        }
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
                    let id = ModuleItemId::new(name);
                    let is_public = *visibility == Visibility::Public;
                    graph.add_item(id, *name_span, ItemKind::Type, is_public);

                    let qualified_name = format!("{module_id}.{name}");
                    let flags = StructLintFlags {
                        is_public,
                        has_serialization_attr: has_serialization_attr(attributes),
                        has_display_attr: attributes.iter().any(|a| a.name == "display"),
                        synthesizes_equals: equality_index.is_synthesized(&qualified_name),
                    };

                    for struct_field in fields {
                        if field_is_lint_candidate(struct_field, &flags) {
                            let field_id = StructFieldId::new(&qualified_name, &struct_field.name);
                            graph.add_struct_field(field_id, struct_field.name_span);
                        }
                    }
                }
                Expression::TypeAlias {
                    name,
                    name_span,
                    visibility,
                    ..
                }
                | Expression::Interface {
                    name,
                    name_span,
                    visibility,
                    ..
                } => {
                    let id = ModuleItemId::new(name);
                    graph.add_item(
                        id,
                        *name_span,
                        ItemKind::Type,
                        *visibility == Visibility::Public,
                    );
                }
                Expression::ImplBlock {
                    methods,
                    receiver_name,
                    ..
                } => {
                    for method in methods {
                        if let Expression::Function {
                            name,
                            name_span,
                            visibility,
                            ..
                        } = method
                        {
                            let id = ModuleItemId::method(name, receiver_name);
                            let is_entry = method_is_entry(name, *visibility);
                            graph.add_item(id, *name_span, ItemKind::Function, is_entry);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn function_is_entry(name: &str, visibility: Visibility, attributes: &[Attribute]) -> bool {
    let is_test = syntax::attributes::has_test_attribute(attributes);
    visibility == Visibility::Public || name == "main" || is_test
}

fn method_is_entry(name: &str, visibility: Visibility) -> bool {
    visibility == Visibility::Public
        || is_upper(name)
        || matches!(name, "string" | "goString" | "error")
}

struct StructLintFlags {
    is_public: bool,
    has_serialization_attr: bool,
    has_display_attr: bool,
    synthesizes_equals: bool,
}

fn field_is_lint_candidate(field: &StructFieldDefinition, flags: &StructLintFlags) -> bool {
    field.visibility != Visibility::Public
        && !flags.is_public
        && !flags.has_serialization_attr
        && !flags.has_display_attr
        && !flags.synthesizes_equals
        && !field.attributes().iter().any(|a| a.name == "tag")
        && !field.is_embedded()
        && !field.name.starts_with('_')
}

fn has_serialization_attr(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| {
        if SERIALIZATION_KEYS.contains(&a.name.as_str()) {
            return true;
        }
        if a.name == "tag" {
            return match a.args.first() {
                Some(AttributeArg::String(key)) => SERIALIZATION_KEYS.contains(&key.as_str()),
                Some(AttributeArg::Raw(raw)) => raw
                    .split(':')
                    .next()
                    .is_some_and(|k| SERIALIZATION_KEYS.contains(&k)),
                _ => false,
            };
        }
        false
    })
}

fn create_unused_diagnostic(kind: ItemKind, span: &Span) -> LisetteDiagnostic {
    let diagnostic_fn: fn(&Span) -> LisetteDiagnostic = match kind {
        ItemKind::Import { .. } => diagnostics::lint::unused_import,
        ItemKind::Type => diagnostics::lint::unused_type,
        ItemKind::Function => diagnostics::lint::unused_function,
        ItemKind::Constant => diagnostics::lint::unused_constant,
    };
    diagnostic_fn(span)
}
