use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::{Expression, ImportAlias, Pattern, Span, StructKind, Visibility};
use syntax::program::File;

use crate::Planner;
use crate::definitions::enum_layout::{
    ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD, ENUM_TAG_FIELD,
};
use crate::definitions::structs::{should_synthesize_stringer, struct_field_go_name};
use crate::names::go_name;

type SpanMap = HashMap<String, Vec<Span>>;

impl Planner<'_> {
    pub(crate) fn detect_name_collisions(&self, files: &[&File]) -> Vec<LisetteDiagnostic> {
        let mut package_block: SpanMap = HashMap::default();
        let mut selectors: HashMap<String, SpanMap> = HashMap::default();
        let mut interfaces: HashMap<String, SpanMap> = HashMap::default();
        let mut diagnostics = Vec::new();

        for file in files {
            for item in &file.items {
                self.collect_item(
                    item,
                    &mut package_block,
                    &mut selectors,
                    &mut interfaces,
                    &mut diagnostics,
                );
            }
        }

        self.collect_import_aliases(files, &mut package_block);

        report_collisions(package_block, &mut diagnostics);
        for (_, members) in sort_by_key(selectors) {
            report_collisions(members, &mut diagnostics);
        }
        for (_, methods) in sort_by_key(interfaces) {
            report_collisions(methods, &mut diagnostics);
        }

        diagnostics
    }

    fn collect_item(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        selectors: &mut HashMap<String, SpanMap>,
        interfaces: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        match item {
            Expression::Function {
                name,
                name_span,
                visibility,
                ..
            } => {
                if self.facts.is_unused_definition(name_span) {
                    return;
                }
                let go = self.free_function_go_name(name, visibility);
                self.check_reserved_prefix(&go, name_span, diagnostics);
                package_block.entry(go).or_default().push(*name_span);
            }
            Expression::Const {
                identifier,
                identifier_span,
                ..
            } => {
                let go = self.const_go_name(identifier);
                self.check_reserved_prefix(&go, identifier_span, diagnostics);
                package_block.entry(go).or_default().push(*identifier_span);
            }
            Expression::TypeAlias {
                name, name_span, ..
            } => {
                let go = go_name::escape_keyword(name).into_owned();
                self.check_reserved_prefix(&go, name_span, diagnostics);
                package_block.entry(go).or_default().push(*name_span);
            }
            Expression::Struct {
                name,
                name_span,
                fields,
                kind,
                attributes,
                generics,
                ..
            } => {
                let type_go = go_name::escape_keyword(name).into_owned();
                self.check_reserved_prefix(&type_go, name_span, diagnostics);
                package_block
                    .entry(type_go.clone())
                    .or_default()
                    .push(*name_span);

                let members = selectors.entry(type_go).or_default();
                match kind {
                    StructKind::Record => {
                        for field in fields {
                            let field_go = struct_field_go_name(field, attributes);
                            members.entry(field_go).or_default().push(field.name_span);
                        }
                    }
                    StructKind::Tuple => {
                        // A single-field tuple with no generics is a newtype (a Go
                        // scalar type, no struct fields); empty tuples have none.
                        let is_newtype = fields.len() == 1 && generics.is_empty();
                        if !fields.is_empty() && !is_newtype {
                            for (index, field) in fields.iter().enumerate() {
                                members
                                    .entry(format!("F{}", index))
                                    .or_default()
                                    .push(field.name_span);
                            }
                        }
                    }
                }
                if let Some(stringer) = self.stringer_method_name(name, attributes) {
                    members
                        .entry(stringer.to_string())
                        .or_default()
                        .push(*name_span);
                }
            }
            Expression::Enum {
                name,
                name_span,
                variants,
                attributes,
                visibility,
                ..
            } => {
                let type_go = go_name::escape_keyword(name).into_owned();
                self.check_reserved_prefix(&type_go, name_span, diagnostics);

                if attributes
                    .iter()
                    .any(|attribute| attribute.name == "iterable")
                {
                    let variants_fn =
                        self.variants_go_name(name, matches!(visibility, Visibility::Public));
                    package_block
                        .entry(variants_fn)
                        .or_default()
                        .push(*name_span);
                }

                package_block
                    .entry(type_go.clone())
                    .or_default()
                    .push(*name_span);
                package_block
                    .entry(format!("{}Tag", name))
                    .or_default()
                    .push(*name_span);
                for variant in variants {
                    let tag_constant = if variant.name.as_str() == ENUM_TAG_FIELD {
                        format!("{}Tag_", name)
                    } else {
                        format!("{}{}", name, variant.name)
                    };
                    package_block
                        .entry(tag_constant)
                        .or_default()
                        .push(variant.name_span);
                    let constructor =
                        format!("Make{}{}", go_name::escape_keyword(name), variant.name);
                    package_block
                        .entry(constructor)
                        .or_default()
                        .push(variant.name_span);
                }

                let members = selectors.entry(type_go).or_default();
                members
                    .entry(ENUM_TAG_FIELD.to_string())
                    .or_default()
                    .push(*name_span);
                let synthesize = should_synthesize_stringer(attributes);
                let (has_user_string, has_user_go_string) = self.stringer_overrides(name);
                if synthesize && !has_user_string {
                    members
                        .entry(ENUM_STRINGER_METHOD.to_string())
                        .or_default()
                        .push(*name_span);
                }
                if synthesize && !has_user_go_string {
                    members
                        .entry(ENUM_GO_STRINGER_METHOD.to_string())
                        .or_default()
                        .push(*name_span);
                }
                if attributes.iter().any(|attribute| attribute.name == "json") {
                    members
                        .entry("MarshalJSON".to_string())
                        .or_default()
                        .push(*name_span);
                    members
                        .entry("UnmarshalJSON".to_string())
                        .or_default()
                        .push(*name_span);
                }
                // Enum payload fields share the type's selector namespace. They
                // are coalesced by Go name across variants, so each distinct
                // name is recorded once (coalescing is intentional, not a
                // collision); per-variant duplicates are out of scope here.
                let qualified = self.facts.qualified_current(name);
                if let Some(layout) = self.module.enum_layout(&qualified) {
                    let mut seen: HashSet<&str> = HashSet::default();
                    for (variant, layout_variant) in variants.iter().zip(&layout.variants) {
                        for field in &layout_variant.fields {
                            if seen.insert(field.go_name.as_str()) {
                                members
                                    .entry(field.go_name.clone())
                                    .or_default()
                                    .push(variant.name_span);
                            }
                        }
                    }
                }
            }
            Expression::Interface {
                name,
                name_span,
                method_signatures,
                visibility,
                ..
            } => {
                let type_go = go_name::escape_keyword(name).into_owned();
                self.check_reserved_prefix(&type_go, name_span, diagnostics);
                package_block
                    .entry(type_go.clone())
                    .or_default()
                    .push(*name_span);

                let is_public = matches!(visibility, Visibility::Public);
                let methods = interfaces.entry(type_go).or_default();
                for signature in method_signatures {
                    if let Expression::Function {
                        name: method_name,
                        name_span: method_span,
                        ..
                    } = signature
                    {
                        let method_go = if is_public || self.method_needs_export(method_name) {
                            go_name::snake_to_camel(method_name)
                        } else {
                            go_name::escape_keyword(method_name).into_owned()
                        };
                        methods.entry(method_go).or_default().push(*method_span);
                    }
                }
            }
            Expression::ImplBlock {
                receiver_name,
                methods,
                ..
            } => {
                let type_go = go_name::escape_keyword(receiver_name).into_owned();
                let qualified_type = self.facts.qualified_current(receiver_name);
                for method in methods {
                    let Expression::Function {
                        name,
                        name_span,
                        visibility,
                        params,
                        ..
                    } = method
                    else {
                        continue;
                    };
                    if self.facts.is_unused_definition(name_span) {
                        continue;
                    }
                    let has_self = params.first().is_some_and(|binding| {
                        matches!(&binding.pattern, Pattern::Identifier { identifier, .. } if identifier == "self")
                    });
                    let is_ufcs = self.facts.is_ufcs_method(&qualified_type, name);
                    let should_export =
                        matches!(visibility, Visibility::Public) || self.method_needs_export(name);
                    if has_self && !is_ufcs {
                        // Go receiver method: lives in the type's selector set.
                        let method_go = if should_export {
                            go_name::snake_to_camel(name)
                        } else {
                            go_name::escape_keyword(name).into_owned()
                        };
                        self.check_reserved_prefix(&method_go, name_span, diagnostics);
                        selectors
                            .entry(type_go.clone())
                            .or_default()
                            .entry(method_go)
                            .or_default()
                            .push(*name_span);
                    } else {
                        // self-less or UFCS method: package-level free function.
                        let method_go = if should_export {
                            go_name::snake_to_camel(name)
                        } else {
                            name.to_string()
                        };
                        let base = format!("{}_{}", receiver_name, method_go);
                        let go = self
                            .module
                            .escape_remap(&base)
                            .map(str::to_string)
                            .unwrap_or_else(|| go_name::escape_reserved(&base).into_owned());
                        self.check_reserved_prefix(&go, name_span, diagnostics);
                        package_block.entry(go).or_default().push(*name_span);
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_import_aliases(&self, files: &[&File], package_block: &mut SpanMap) {
        let go_package_names = self.facts.go_package_names();
        let unused = self.facts.unused_imports_for_current_module();
        for file in files {
            for import in file.imports() {
                if matches!(import.alias, Some(ImportAlias::Blank(_))) {
                    continue;
                }
                let Some(alias) = import.effective_alias(go_package_names) else {
                    continue;
                };
                if unused.contains(alias.as_str()) {
                    continue;
                }
                let qualifier = go_name::sanitize_package_name(&alias);
                if let Some(spans) = package_block.get_mut(qualifier.as_ref()) {
                    spans.push(import.name_span);
                }
            }
        }
    }

    fn free_function_go_name(&self, name: &str, visibility: &Visibility) -> String {
        if matches!(visibility, Visibility::Public) {
            go_name::snake_to_camel(name)
        } else {
            self.module
                .escape_remap(name)
                .map(str::to_string)
                .unwrap_or_else(|| go_name::escape_reserved(name).into_owned())
        }
    }

    fn const_go_name(&self, identifier: &str) -> String {
        self.module
            .escape_remap(identifier)
            .map(str::to_string)
            .unwrap_or_else(|| identifier.to_string())
    }

    fn check_reserved_prefix(&self, go: &str, span: &Span, out: &mut Vec<LisetteDiagnostic>) {
        if go.starts_with(go_name::ADAPTER_TYPE_PREFIX) {
            out.push(emit_diag::reserved_go_prefix(
                go,
                go_name::ADAPTER_TYPE_PREFIX,
                span,
            ));
        }
    }
}

fn report_collisions(map: SpanMap, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let mut groups: Vec<(String, Vec<Span>)> = map.into_iter().collect();
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    for (go, mut spans) in groups {
        spans.sort_by_key(|span| (span.file_id, span.byte_offset));
        spans.dedup();
        if spans.len() > 1 {
            diagnostics.push(emit_diag::go_name_collision(&go, &spans, None));
        }
    }
}

fn sort_by_key(map: HashMap<String, SpanMap>) -> Vec<(String, SpanMap)> {
    let mut entries: Vec<(String, SpanMap)> = map.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}
