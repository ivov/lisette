mod extract;
mod redundant_import_alias;
mod reference_graph;
mod visibility_constraints;

use rayon::prelude::*;
use rustc_hash::FxHashSet as HashSet;
use std::sync::Arc;

use crate::passes::PARALLEL_THRESHOLD;
use crate::passes::lints::span_edit::statement_deletion;
use diagnostics::LisetteDiagnostic;
use diagnostics::LocalSink;
use diagnostics::{Edit, Fix};
use semantics::facts::Facts;
use semantics::store::Store;
use syntax::ast::{Attribute, AttributeArg, Expression, Span, StructFieldDefinition, Visibility};
use syntax::program::EqualityIndex;
use syntax::program::File;
use syntax::program::UnusedInfo;
use syntax::program::{Package, is_internal_package_id};

use extract::{AliasMap, is_upper, walk_expression};
use redundant_import_alias::check_redundant_aliases;
use reference_graph::{
    EnumVariantId, ItemKind, MemberKind, PackageItemId, ReferenceGraph, StructFieldId,
};
use syntax::attributes::SERIALIZATION_KEYS;
use visibility_constraints::check_visibility_constraints;

struct RefLintResult {
    diagnostics: Vec<LisetteDiagnostic>,
    unused_import_aliases: HashSet<String>,
    unused_definition_spans: Vec<Span>,
}

pub(crate) fn run(store: &Store, facts: &Facts) -> (Vec<LisetteDiagnostic>, UnusedInfo) {
    let mut packages: Vec<&Package> = store
        .packages
        .values()
        .map(Arc::as_ref)
        .filter(|m| !is_internal_package_id(&m.id))
        .collect();
    packages.sort_unstable_by(|a, b| a.id.cmp(&b.id));

    let mut unused = UnusedInfo::default();

    if packages.len() < PARALLEL_THRESHOLD {
        let sink = LocalSink::new();
        for package in &packages {
            apply_ref_lints(package, facts, store, &mut unused, &sink);
        }
        return (sink.into_diagnostics(), unused);
    }

    type WorkerOutput = (LocalSink, UnusedInfo);
    let outputs: Vec<WorkerOutput> = packages
        .par_iter()
        .map(|package| {
            let local_sink = LocalSink::new();
            let mut local_unused = UnusedInfo::default();
            apply_ref_lints(package, facts, store, &mut local_unused, &local_sink);
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
    package: &Package,
    facts: &Facts,
    store: &Store,
    unused: &mut UnusedInfo,
    sink: &LocalSink,
) {
    let result = run_ref_lints(package, facts, store);
    if !result.unused_import_aliases.is_empty() {
        unused.imports_by_package.insert(
            package.id.clone().into(),
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
        let allows: Vec<_> = package
            .source_files()
            .flat_map(|file| super::suppression::collect_declaration_allows(&file.items))
            .collect();
        diagnostics = super::suppression::filter_unused_allowed(diagnostics, &allows);
    }
    diagnostics.sort_by(LisetteDiagnostic::sort_key);
    sink.extend(diagnostics);
}

fn run_ref_lints(package: &Package, facts: &Facts, store: &Store) -> RefLintResult {
    let files = &package.files;
    let equality_index = &store.equality_index;
    let mut diagnostics = Vec::new();
    let mut unused_import_spans = HashSet::default();
    let mut unused_definition_spans = Vec::new();
    let mut graph = ReferenceGraph::new();

    let files_with_aliases: Vec<_> = files
        .values()
        .filter(|file| !file.is_d_lis())
        .map(|file| (file, AliasMap::build(file, store)))
        .collect();
    collect_items(&files_with_aliases, &package.id, equality_index, &mut graph);

    for (file, alias_map) in &files_with_aliases {
        for item in &file.items {
            walk_expression(package, item, &mut graph, alias_map, None);
        }
    }

    for ((method_package_id, method_name), satisfactions) in &facts.interface_satisfied_methods {
        if method_package_id != &package.id {
            continue;
        }
        if method_name == "equals" {
            for satisfaction in satisfactions {
                graph.mark_as_used(PackageItemId::equals_method(&satisfaction.impl_type_name));
            }
        } else {
            graph.mark_as_used(PackageItemId::new(method_name));
        }
    }

    let usage = graph.analyze();
    for (_, info) in usage.unreachable_items() {
        if matches!(info.kind, ItemKind::Import { .. }) {
            unused_import_spans.insert(info.span);
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

    // Emit drops an import from every file of the package at once.
    let unused_import_aliases = usage.unused_import_aliases();

    check_redundant_aliases(files, store, &unused_import_spans, &mut diagnostics);

    check_visibility_constraints(package, files, &mut diagnostics);

    for (kind, span) in graph.unused_members() {
        diagnostics.push(match kind {
            MemberKind::StructField => diagnostics::lint::unused_field(span),
            MemberKind::EnumVariant => diagnostics::lint::unused_variant(span),
        });
    }

    RefLintResult {
        diagnostics,
        unused_import_aliases,
        unused_definition_spans,
    }
}

fn collect_items(
    files: &[(&File, AliasMap<'_>)],
    package_id: &str,
    equality_index: &EqualityIndex,
    graph: &mut ReferenceGraph,
) {
    for (file, aliases) in files {
        for (alias, name_span, statement_span) in aliases.imports() {
            graph.add_import(
                PackageItemId::import(file.id, alias),
                name_span,
                statement_span,
            );
        }
        for item in &file.items {
            match item {
                Expression::PackageImport { .. } => {}
                Expression::Function {
                    name,
                    name_span,
                    visibility,
                    attributes,
                    ..
                } => {
                    let id = PackageItemId::new(name);
                    let is_entry = function_is_entry(name, *visibility, attributes);
                    graph.add_item(id, *name_span, ItemKind::Function, is_entry);
                }
                Expression::Const {
                    identifier,
                    identifier_span,
                    visibility,
                    ..
                } => {
                    let id = PackageItemId::new(identifier);
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
                    let id = PackageItemId::new(name);
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
                    let id = PackageItemId::new(name);
                    let is_public = *visibility == Visibility::Public;
                    graph.add_item(id, *name_span, ItemKind::Type, is_public);

                    let qualified_name = format!("{package_id}.{name}");
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
                    let id = PackageItemId::new(name);
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
                            let id = PackageItemId::method(name, receiver_name);
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
