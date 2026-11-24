use crate::lookup::facts::emit_facts::EmitFacts;
use rustc_hash::{FxHashMap, FxHashSet};

use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::{Attribute, Generic};
use syntax::ast::{EnumVariant, Expression, Span, StructFields, VariantFields, Visibility};
use syntax::attributes;
use syntax::go_names;
use syntax::program::File;

use crate::Planner;
use crate::lookup::facts::synthesized_methods::StructShape;
use crate::target::names::go_name;
use syntax::go_names::ENUM_TAG_FIELD;

#[derive(Default)]
struct SpanMap(FxHashMap<String, Vec<Span>>);

impl SpanMap {
    fn claim(&mut self, name: impl Into<String>, span: Span) {
        self.0.entry(name.into()).or_default().push(span);
    }

    fn report(self, diagnostics: &mut Vec<LisetteDiagnostic>) {
        let mut groups: Vec<(String, Vec<Span>)> = self.0.into_iter().collect();
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        for (go, mut spans) in groups {
            spans.sort_by_key(|span| (span.file_id, span.byte_offset));
            spans.dedup();
            let [first, second, rest @ ..] = spans.as_slice() else {
                continue;
            };
            diagnostics.push(emit_diag::go_name_collision(&go, first, second, rest, None));
        }
    }

    fn report_each(maps: FxHashMap<String, SpanMap>, diagnostics: &mut Vec<LisetteDiagnostic>) {
        let mut entries: Vec<(String, SpanMap)> = maps.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, map) in entries {
            map.report(diagnostics);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateFunctions {
    Include,
    Skip,
}

#[derive(Default)]
pub(in crate::lowering) struct CollectedNames {
    package_block: SpanMap,
    selectors: FxHashMap<String, SpanMap>,
    interfaces: FxHashMap<String, SpanMap>,
    diagnostics: Vec<LisetteDiagnostic>,
}

impl CollectedNames {
    pub(in crate::lowering) fn declared_names(&self) -> FxHashSet<String> {
        self.package_block.0.keys().cloned().collect()
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn name_collision_diagnostics(
        &self,
        files: &[&File],
        collected: CollectedNames,
        import_qualifiers: &[(String, Span)],
    ) -> Vec<LisetteDiagnostic> {
        let CollectedNames {
            mut package_block,
            selectors,
            interfaces,
            mut diagnostics,
        } = collected;

        collect_import_aliases(import_qualifiers, &mut package_block, &mut diagnostics);

        package_block.report(&mut diagnostics);
        SpanMap::report_each(selectors, &mut diagnostics);
        SpanMap::report_each(interfaces, &mut diagnostics);
        diagnostics.extend(self.check_generic_names(files));

        diagnostics
    }

    pub(in crate::lowering) fn collect_names(&self, files: &[&File]) -> CollectedNames {
        self.collect_names_with(files, PrivateFunctions::Include)
    }

    pub(in crate::lowering) fn collect_fixed_names(&self, files: &[&File]) -> FxHashSet<String> {
        self.collect_names_with(files, PrivateFunctions::Skip)
            .declared_names()
    }

    fn collect_names_with(
        &self,
        files: &[&File],
        private_functions: PrivateFunctions,
    ) -> CollectedNames {
        let mut collected = CollectedNames::default();
        for file in files {
            for item in &file.items {
                self.collect_item(item, &mut collected, private_functions);
            }
        }
        collected
    }

    fn collect_item(
        &self,
        item: &Expression,
        collected: &mut CollectedNames,
        private_functions: PrivateFunctions,
    ) {
        match item {
            Expression::Function { .. } => {
                self.collect_function(item, collected, private_functions)
            }
            Expression::Const {
                identifier,
                identifier_span,
                ..
            } => collect_const(identifier, *identifier_span, collected),
            Expression::TypeAlias {
                name, name_span, ..
            } => collect_type_alias(&self.facts, name, *name_span, collected),
            Expression::Struct {
                name,
                name_span,
                fields,
                attributes,
                ..
            } => self.collect_struct(name, *name_span, fields, attributes, collected),
            Expression::Enum { .. } => collect_enum(&self.facts, item, collected),
            Expression::Interface {
                name,
                name_span,
                method_signatures,
                visibility,
                ..
            } => collect_interface(
                &self.facts,
                name,
                *name_span,
                method_signatures,
                visibility,
                collected,
            ),
            Expression::ImplBlock {
                receiver_name,
                methods,
                ..
            } => collect_impl_block(&self.facts, receiver_name, methods, collected),
            _ => {}
        }
    }

    fn collect_function(
        &self,
        item: &Expression,
        collected: &mut CollectedNames,
        private_functions: PrivateFunctions,
    ) {
        let Expression::Function {
            name,
            name_span,
            visibility,
            attributes,
            ..
        } = item
        else {
            return;
        };
        let name_span = *name_span;
        let is_public = matches!(visibility, Visibility::Public);
        if is_public || private_functions == PrivateFunctions::Include {
            let go = self.free_function_go_name(name, is_public);
            check_reserved_prefix(name, &name_span, &mut collected.diagnostics);
            check_reserved_prefix(&go, &name_span, &mut collected.diagnostics);
            collected.package_block.claim(go, name_span);
        }
        if attributes::has_test_attribute(attributes) {
            let wrapper = go_name::go_test_function_name(name);
            collected.package_block.claim(wrapper, name_span);
        }
    }

    fn collect_struct(
        &self,
        name: &str,
        name_span: Span,
        fields: &StructFields,
        attributes: &[Attribute],
        collected: &mut CollectedNames,
    ) {
        let type_go = go_name::escape_type_name(name).into_owned();
        check_reserved_prefix(&type_go, &name_span, &mut collected.diagnostics);
        check_reserved_qualifier(&type_go, &name_span, &mut collected.diagnostics);
        collected.package_block.claim(type_go.clone(), name_span);

        let members = collected.selectors.entry(type_go).or_default();
        match fields {
            StructFields::Record(fields) => {
                let qualified = self.facts.qualified_current(name);
                for field in fields {
                    let field_go = self.facts.declared_field_go_name(&qualified, field);
                    members.claim(field_go, field.name_span);
                }
            }
            StructFields::Tuple(fields) => {
                let is_newtype = self.facts.is_newtype_definition(name);
                if !fields.is_empty() && !is_newtype {
                    for (index, field) in fields.iter().enumerate() {
                        members.claim(go_name::tuple_struct_field(index), field.name_span);
                    }
                }
            }
        }
        let shape = match fields {
            StructFields::Record(_) => StructShape::Record,
            StructFields::Tuple(_) => StructShape::Tuple,
        };
        for method in self
            .facts
            .struct_synthesized_methods(name, attributes, shape)
        {
            for go_name in self.facts.synthesized_method_go_names(method) {
                members.claim(go_name, name_span);
            }
        }
    }

    fn check_generic_names(&self, files: &[&File]) -> Vec<LisetteDiagnostic> {
        let mut diagnostics = Vec::new();
        for item in files.iter().flat_map(|file| &file.items) {
            match item {
                Expression::Function { generics, .. }
                | Expression::TypeAlias { generics, .. }
                | Expression::Struct { generics, .. }
                | Expression::Enum { generics, .. } => {
                    self.check_reserved_qualifier_generics(generics, &mut diagnostics);
                }
                Expression::Interface {
                    generics,
                    method_signatures,
                    ..
                } => {
                    self.check_reserved_qualifier_generics(generics, &mut diagnostics);
                    for signature in method_signatures {
                        if let Expression::Function { generics, .. } = signature {
                            self.check_reserved_qualifier_generics(generics, &mut diagnostics);
                        }
                    }
                }
                Expression::ImplBlock {
                    receiver_name,
                    methods,
                    generics,
                    ..
                } => {
                    self.check_reserved_qualifier_generics(generics, &mut diagnostics);
                    let qualified_type = self.facts.qualified_current(receiver_name);
                    for method in methods {
                        let Expression::Function {
                            name,
                            generics: method_generics,
                            ..
                        } = method
                        else {
                            continue;
                        };
                        if self.facts.emits_go_method(&qualified_type, name) {
                            self.check_reserved_qualifier_generics(
                                method_generics,
                                &mut diagnostics,
                            );
                        } else {
                            self.check_free_function_generics(
                                generics,
                                method_generics,
                                &mut diagnostics,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        diagnostics
    }

    fn check_reserved_qualifier_generics(
        &self,
        generics: &[Generic],
        out: &mut Vec<LisetteDiagnostic>,
    ) {
        let mut emitted = SpanMap::default();
        for generic in generics {
            check_reserved_qualifier(&generic.name, &generic.span, out);
            emitted.claim(self.generic_go_name(&generic.name), generic.span);
        }
        emitted.report(out);
    }

    fn check_free_function_generics(
        &self,
        impl_generics: &[Generic],
        method_generics: &[Generic],
        out: &mut Vec<LisetteDiagnostic>,
    ) {
        let mut emitted = SpanMap::default();
        for generic in impl_generics {
            emitted.claim(self.generic_go_name(&generic.name), generic.span);
        }
        let mut method_names: FxHashSet<String> = FxHashSet::default();
        for generic in method_generics {
            check_reserved_qualifier(&generic.name, &generic.span, out);
            let go = self.generic_go_name(&generic.name).into_owned();
            emitted.claim(go.clone(), generic.span);
            method_names.insert(go);
        }
        emitted.0.retain(|go, _| method_names.contains(go));
        emitted.report(out);
    }
}

fn collect_enum_payload_fields(
    name: &str,
    variants: &[EnumVariant],
    members: &mut SpanMap,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let slots = go_names::enum_field_slots(name, variants);
    let mut seen: FxHashSet<String> = FxHashSet::default();
    for (variant_index, variant) in variants.iter().enumerate() {
        let field_names = &slots[variant_index];
        for field_name in field_names {
            if seen.insert(field_name.clone()) {
                members.claim(field_name.clone(), variant.name_span);
            }
        }
        if let VariantFields::Struct(fields) = &variant.fields {
            let mut variant_fields = SpanMap::default();
            for (field, field_name) in fields.iter().zip(field_names) {
                variant_fields.claim(field_name.clone(), field.name_span);
            }
            variant_fields.report(diagnostics);
        }
    }
}

fn check_reserved_prefix(go: &str, span: &Span, out: &mut Vec<LisetteDiagnostic>) {
    if let Some(prefix) = go_name::reserved_prefix_of(go) {
        out.push(emit_diag::reserved_go_prefix(go, prefix, span));
    }
}

fn check_reserved_qualifier(go: &str, span: &Span, out: &mut Vec<LisetteDiagnostic>) {
    if go_name::is_generated_import_qualifier(go) {
        out.push(emit_diag::reserved_go_qualifier(go, span));
    }
}

fn collect_const(identifier: &str, identifier_span: Span, collected: &mut CollectedNames) {
    let go = go_name::screaming_snake_to_camel(identifier);
    check_reserved_prefix(identifier, &identifier_span, &mut collected.diagnostics);
    check_reserved_prefix(&go, &identifier_span, &mut collected.diagnostics);
    collected.package_block.claim(go, identifier_span);
}

fn collect_type_alias(
    facts: &EmitFacts<'_>,
    name: &str,
    name_span: Span,
    collected: &mut CollectedNames,
) {
    if facts.alias_emits_nothing(name) {
        return;
    }
    let go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&go, &name_span, &mut collected.diagnostics);
    check_reserved_qualifier(&go, &name_span, &mut collected.diagnostics);
    collected.package_block.claim(go, name_span);
}

fn collect_import_aliases(
    import_qualifiers: &[(String, Span)],
    package_block: &mut SpanMap,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    for (qualifier, span) in import_qualifiers {
        check_reserved_prefix(qualifier, span, diagnostics);
        if let Some(spans) = package_block.0.get_mut(qualifier)
            && spans
                .iter()
                .any(|declared| declared.file_id == span.file_id)
        {
            spans.push(*span);
        }
    }
}

fn collect_impl_block(
    facts: &EmitFacts<'_>,
    receiver_name: &str,
    methods: &[Expression],
    collected: &mut CollectedNames,
) {
    let type_go = go_name::escape_type_name(receiver_name).into_owned();
    let qualified_type = facts.qualified_current(receiver_name);
    for method in methods {
        let Expression::Function {
            name,
            name_span,
            visibility,
            ..
        } = method
        else {
            continue;
        };
        let is_public = matches!(visibility, Visibility::Public);
        let go = if facts.emits_go_method(&qualified_type, name) {
            let method_go = facts.method_go_name(name, is_public);
            check_reserved_prefix(name, name_span, &mut collected.diagnostics);
            check_reserved_prefix(&method_go, name_span, &mut collected.diagnostics);
            collected
                .selectors
                .entry(type_go.clone())
                .or_default()
                .claim(method_go, *name_span);
            continue;
        } else {
            go_name::associated_function_go_name(receiver_name, name, is_public)
        };
        check_reserved_prefix(name, name_span, &mut collected.diagnostics);
        check_reserved_prefix(&go, name_span, &mut collected.diagnostics);
        collected.package_block.claim(go, *name_span);
    }
}

fn collect_interface(
    facts: &EmitFacts<'_>,
    name: &str,
    name_span: Span,
    method_signatures: &[Expression],
    visibility: &Visibility,
    collected: &mut CollectedNames,
) {
    let type_go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&type_go, &name_span, &mut collected.diagnostics);
    check_reserved_qualifier(&type_go, &name_span, &mut collected.diagnostics);
    collected.package_block.claim(type_go.clone(), name_span);

    let is_public = matches!(visibility, Visibility::Public);
    let methods = collected.interfaces.entry(type_go).or_default();
    for signature in method_signatures {
        if let Expression::Function {
            name: method_name,
            name_span: method_span,
            ..
        } = signature
        {
            let method_go = facts.interface_method_go_name(method_name, is_public);
            methods.claim(method_go, *method_span);
        }
    }
}

fn collect_enum(facts: &EmitFacts<'_>, item: &Expression, collected: &mut CollectedNames) {
    let Expression::Enum {
        name,
        name_span,
        variants,
        attributes,
        visibility,
        ..
    } = item
    else {
        return;
    };
    let name_span = *name_span;
    let type_go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&type_go, &name_span, &mut collected.diagnostics);
    check_reserved_qualifier(&type_go, &name_span, &mut collected.diagnostics);

    let package_block = &mut collected.package_block;
    if attributes
        .iter()
        .any(|attribute| attribute.name == "iterate")
    {
        let variants_fn = go_name::associated_function_go_name(
            name,
            "variants",
            matches!(visibility, Visibility::Public),
        );
        package_block.claim(variants_fn, name_span);
    }

    package_block.claim(type_go.clone(), name_span);
    package_block.claim(go_name::enum_tag_type(name), name_span);
    for variant in variants {
        package_block.claim(
            go_name::enum_tag_constant(name, &variant.name),
            variant.name_span,
        );
        package_block.claim(
            go_name::enum_make_function(name, &variant.name),
            variant.name_span,
        );
    }

    let members = collected.selectors.entry(type_go).or_default();
    members.claim(ENUM_TAG_FIELD, name_span);
    for method in facts.enum_synthesized_methods(name, attributes) {
        for go_name in facts.synthesized_method_go_names(method) {
            members.claim(go_name, name_span);
        }
    }
    collect_enum_payload_fields(name, variants, members, &mut collected.diagnostics);
}
