use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::{
    EnumVariant, Expression, ImportAlias, Pattern, Span, StructKind, VariantFields, Visibility,
};
use syntax::program::File;

use crate::Planner;
use crate::definitions::enum_layout::{
    ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD, ENUM_TAG_FIELD,
};
use crate::definitions::structs::{
    DEBUG_STRING_METHOD, should_synthesize_stringer, struct_field_go_name,
};
use crate::names::go_name;

type SpanMap = HashMap<String, Vec<Span>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollectPayloadFields {
    Yes,
    No,
}

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
                    CollectPayloadFields::Yes,
                );
            }
        }

        self.collect_import_aliases(files, &mut package_block, &mut diagnostics);

        report_collisions(package_block, &mut diagnostics);
        for (_, members) in sort_by_key(selectors) {
            report_collisions(members, &mut diagnostics);
        }
        for (_, methods) in sort_by_key(interfaces) {
            report_collisions(methods, &mut diagnostics);
        }

        diagnostics
    }

    pub(crate) fn package_block_names(&self, files: &[&File]) -> HashSet<String> {
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
                    CollectPayloadFields::No,
                );
            }
        }

        let mut names: HashSet<String> = package_block.into_keys().collect();
        self.for_each_import_qualifier(files, |qualifier, _span| {
            names.insert(qualifier.to_string());
        });
        names
    }

    fn collect_item(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        selectors: &mut HashMap<String, SpanMap>,
        interfaces: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
        payload_fields: CollectPayloadFields,
    ) {
        match item {
            Expression::Function { .. } => self.collect_function(item, package_block, diagnostics),
            Expression::Const { .. } => self.collect_const(item, package_block, diagnostics),
            Expression::TypeAlias { .. } => {
                self.collect_type_alias(item, package_block, diagnostics)
            }
            Expression::Struct { .. } => {
                self.collect_struct(item, package_block, selectors, diagnostics)
            }
            Expression::Enum { .. } => {
                self.collect_enum(item, package_block, selectors, diagnostics, payload_fields)
            }
            Expression::Interface { .. } => {
                self.collect_interface(item, package_block, interfaces, diagnostics)
            }
            Expression::ImplBlock { .. } => {
                self.collect_impl_block(item, package_block, selectors, diagnostics)
            }
            _ => {}
        }
    }

    fn collect_function(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::Function {
            name,
            name_span,
            visibility,
            generics,
            attributes,
            ..
        } = item
        else {
            return;
        };
        if self.facts.is_unused_definition(name_span) {
            return;
        }
        self.check_reserved_qualifier_generics(generics, diagnostics);
        let go = self.free_function_go_name(name, visibility);
        self.check_reserved_prefix(&go, name_span, diagnostics);
        package_block.entry(go).or_default().push(*name_span);
        if syntax::attributes::has_test_attribute(attributes) {
            let wrapper = go_name::go_test_function_name(name);
            package_block.entry(wrapper).or_default().push(*name_span);
        }
    }

    fn collect_const(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::Const {
            identifier,
            identifier_span,
            ..
        } = item
        else {
            return;
        };
        let go = self.const_go_name(identifier);
        self.check_reserved_prefix(&go, identifier_span, diagnostics);
        package_block.entry(go).or_default().push(*identifier_span);
    }

    fn collect_type_alias(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::TypeAlias {
            name,
            name_span,
            generics,
            ..
        } = item
        else {
            return;
        };
        let go = go_name::escape_type_name(name).into_owned();
        self.check_reserved_prefix(&go, name_span, diagnostics);
        self.check_reserved_qualifier(&go, name_span, diagnostics);
        self.check_reserved_qualifier_generics(generics, diagnostics);
        package_block.entry(go).or_default().push(*name_span);
    }

    fn collect_struct(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        selectors: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::Struct {
            name,
            name_span,
            fields,
            kind,
            attributes,
            generics,
            ..
        } = item
        else {
            return;
        };
        let type_go = go_name::escape_type_name(name).into_owned();
        self.check_reserved_prefix(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier_generics(generics, diagnostics);
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
        if self.should_synthesize_to_string(name, attributes) {
            members
                .entry(self.to_string_method_go_name())
                .or_default()
                .push(*name_span);
        }
        if self.should_synthesize_equals(name) {
            members
                .entry(self.equals_method_go_name())
                .or_default()
                .push(*name_span);
        }
        if self.facts.emit_tests_enabled() && !self.debug_string_override(name) {
            members
                .entry(DEBUG_STRING_METHOD.to_string())
                .or_default()
                .push(*name_span);
        }
    }

    fn collect_enum(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        selectors: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
        payload_fields: CollectPayloadFields,
    ) {
        let Expression::Enum {
            name,
            name_span,
            variants,
            attributes,
            visibility,
            generics,
            ..
        } = item
        else {
            return;
        };
        let type_go = go_name::escape_type_name(name).into_owned();
        self.check_reserved_prefix(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier_generics(generics, diagnostics);

        if attributes
            .iter()
            .any(|attribute| attribute.name == "iterate")
        {
            let variants_fn = self.variants_go_name(name, matches!(visibility, Visibility::Public));
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
            let tag_constant = go_name::enum_tag_constant(name, &variant.name);
            package_block
                .entry(tag_constant)
                .or_default()
                .push(variant.name_span);
            let constructor = format!("Make{}{}", go_name::escape_type_name(name), variant.name);
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
        if self.should_synthesize_to_string(name, attributes) {
            members
                .entry(self.to_string_method_go_name())
                .or_default()
                .push(*name_span);
        }
        if self.should_synthesize_equals(name) {
            members
                .entry(self.equals_method_go_name())
                .or_default()
                .push(*name_span);
        }
        if self.facts.emit_tests_enabled() && !self.debug_string_override(name) {
            members
                .entry(DEBUG_STRING_METHOD.to_string())
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
        if payload_fields == CollectPayloadFields::Yes {
            self.collect_enum_payload_fields(name, variants, members, diagnostics);
        }
    }

    /// Record enum payload-field Go names in the type's selector namespace.
    /// Names are coalesced across variants (each distinct Go name once, which is
    /// intentional, not a collision). Within a single struct variant, two fields
    /// landing on the same Go name would assign one struct field twice, so each
    /// variant's own fields are additionally grouped and reported.
    fn collect_enum_payload_fields(
        &self,
        name: &str,
        variants: &[EnumVariant],
        members: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let qualified = self.facts.qualified_current(name);
        let Some(layout) = self.enum_layout(&qualified) else {
            return;
        };
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
            if let VariantFields::Struct(fields) = &variant.fields {
                let mut variant_fields: SpanMap = HashMap::default();
                for (field, layout_field) in fields.iter().zip(&layout_variant.fields) {
                    variant_fields
                        .entry(layout_field.go_name.clone())
                        .or_default()
                        .push(field.name_span);
                }
                report_collisions(variant_fields, diagnostics);
            }
        }
    }

    fn collect_interface(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        interfaces: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::Interface {
            name,
            name_span,
            method_signatures,
            visibility,
            generics,
            ..
        } = item
        else {
            return;
        };
        let type_go = go_name::escape_type_name(name).into_owned();
        self.check_reserved_prefix(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier(&type_go, name_span, diagnostics);
        self.check_reserved_qualifier_generics(generics, diagnostics);
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
                generics: method_generics,
                ..
            } = signature
            {
                self.check_reserved_qualifier_generics(method_generics, diagnostics);
                let method_go = if is_public || self.method_needs_export(method_name) {
                    go_name::snake_to_camel(method_name)
                } else {
                    go_name::escape_keyword(method_name).into_owned()
                };
                methods.entry(method_go).or_default().push(*method_span);
            }
        }
    }

    fn collect_impl_block(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        selectors: &mut HashMap<String, SpanMap>,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        let Expression::ImplBlock {
            receiver_name,
            methods,
            generics,
            ..
        } = item
        else {
            return;
        };
        self.check_reserved_qualifier_generics(generics, diagnostics);
        let type_go = go_name::escape_type_name(receiver_name).into_owned();
        let qualified_type = self.facts.qualified_current(receiver_name);
        for method in methods {
            let Expression::Function {
                name,
                name_span,
                visibility,
                params,
                generics: method_generics,
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
                self.check_reserved_qualifier_generics(method_generics, diagnostics);
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
                self.check_free_function_generics(generics, method_generics, diagnostics);
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

    fn collect_import_aliases(
        &self,
        files: &[&File],
        package_block: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
    ) {
        self.for_each_import_qualifier(files, |qualifier, span| {
            self.check_reserved_prefix(qualifier, &span, diagnostics);
            if let Some(spans) = package_block.get_mut(qualifier) {
                spans.push(span);
            }
        });
    }

    fn for_each_import_qualifier(&self, files: &[&File], mut visit: impl FnMut(&str, Span)) {
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
                let span = match &import.alias {
                    Some(ImportAlias::Named(_, span)) => *span,
                    _ => import.name_span,
                };
                visit(qualifier.as_ref(), span);
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
        if let Some(prefix) = go_name::reserved_prefix_of(go) {
            out.push(emit_diag::reserved_go_prefix(go, prefix, span));
        }
    }

    fn check_reserved_qualifier(&self, go: &str, span: &Span, out: &mut Vec<LisetteDiagnostic>) {
        if go_name::is_generated_import_qualifier(go) {
            out.push(emit_diag::reserved_go_qualifier(go, span));
        }
    }

    fn check_reserved_qualifier_generics(
        &self,
        generics: &[syntax::ast::Generic],
        out: &mut Vec<LisetteDiagnostic>,
    ) {
        let mut emitted: SpanMap = HashMap::default();
        for generic in generics {
            self.check_reserved_qualifier(&generic.name, &generic.span, out);
            emitted
                .entry(self.generic_go_name(&generic.name).into_owned())
                .or_default()
                .push(generic.span);
        }
        report_collisions(emitted, out);
    }

    fn check_free_function_generics(
        &self,
        impl_generics: &[syntax::ast::Generic],
        method_generics: &[syntax::ast::Generic],
        out: &mut Vec<LisetteDiagnostic>,
    ) {
        let mut emitted: SpanMap = HashMap::default();
        for generic in impl_generics {
            emitted
                .entry(self.generic_go_name(&generic.name).into_owned())
                .or_default()
                .push(generic.span);
        }
        let mut method_names: HashSet<String> = HashSet::default();
        for generic in method_generics {
            self.check_reserved_qualifier(&generic.name, &generic.span, out);
            let go = self.generic_go_name(&generic.name).into_owned();
            emitted.entry(go.clone()).or_default().push(generic.span);
            method_names.insert(go);
        }
        emitted.retain(|go, _| method_names.contains(go));
        report_collisions(emitted, out);
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
