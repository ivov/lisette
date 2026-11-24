use crate::lookup::facts::emit_facts::EmitFacts;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{LisetteDiagnostic, emit as emit_diag};
use syntax::ast::Generic;
use syntax::ast::{EnumVariant, Expression, Span, StructFields, VariantFields, Visibility};
use syntax::attributes;
use syntax::go_names;
use syntax::program::File;

use crate::Planner;
use crate::lookup::facts::synthesized_methods::StructShape;
use crate::target::names::go_name;
use syntax::go_names::ENUM_TAG_FIELD;

type SpanMap = HashMap<String, Vec<Span>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrivateFunctions {
    Include,
    Skip,
}

#[derive(Default)]
pub(in crate::lowering) struct CollectedNames {
    package_block: SpanMap,
    selectors: HashMap<String, SpanMap>,
    interfaces: HashMap<String, SpanMap>,
    diagnostics: Vec<LisetteDiagnostic>,
}

impl CollectedNames {
    pub(in crate::lowering) fn declared_names(&self) -> HashSet<String> {
        self.package_block.keys().cloned().collect()
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

        report_collisions(package_block, &mut diagnostics);
        for (_, members) in sort_by_key(selectors) {
            report_collisions(members, &mut diagnostics);
        }
        for (_, methods) in sort_by_key(interfaces) {
            report_collisions(methods, &mut diagnostics);
        }
        diagnostics.extend(self.check_generic_names(files));

        diagnostics
    }

    pub(in crate::lowering) fn collect_names(&self, files: &[&File]) -> CollectedNames {
        self.collect_names_with(files, PrivateFunctions::Include)
    }

    pub(in crate::lowering) fn collect_fixed_names(&self, files: &[&File]) -> HashSet<String> {
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
        let CollectedNames {
            package_block,
            selectors,
            interfaces,
            diagnostics,
        } = collected;
        match item {
            Expression::Function { .. } => {
                self.collect_function(item, package_block, diagnostics, private_functions)
            }
            Expression::Const { .. } => collect_const(item, package_block, diagnostics),
            Expression::TypeAlias { .. } => {
                collect_type_alias(&self.facts, item, package_block, diagnostics)
            }
            Expression::Struct { .. } => {
                self.collect_struct(item, package_block, selectors, diagnostics)
            }
            Expression::Enum { .. } => {
                collect_enum(&self.facts, item, package_block, selectors, diagnostics)
            }
            Expression::Interface { .. } => {
                collect_interface(&self.facts, item, package_block, interfaces, diagnostics)
            }
            Expression::ImplBlock { .. } => {
                collect_impl_block(&self.facts, item, package_block, selectors, diagnostics)
            }
            _ => {}
        }
    }

    fn collect_function(
        &self,
        item: &Expression,
        package_block: &mut SpanMap,
        diagnostics: &mut Vec<LisetteDiagnostic>,
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
        let is_public = matches!(visibility, Visibility::Public);
        if is_public || private_functions == PrivateFunctions::Include {
            let go = self.free_function_go_name(name, is_public);
            check_reserved_prefix(name, name_span, diagnostics);
            check_reserved_prefix(&go, name_span, diagnostics);
            package_block.entry(go).or_default().push(*name_span);
        }
        if attributes::has_test_attribute(attributes) {
            let wrapper = go_name::go_test_function_name(name);
            package_block.entry(wrapper).or_default().push(*name_span);
        }
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
            attributes,
            ..
        } = item
        else {
            return;
        };
        let type_go = go_name::escape_type_name(name).into_owned();
        check_reserved_prefix(&type_go, name_span, diagnostics);
        check_reserved_qualifier(&type_go, name_span, diagnostics);
        package_block
            .entry(type_go.clone())
            .or_default()
            .push(*name_span);

        let members = selectors.entry(type_go).or_default();
        match fields {
            StructFields::Record(fields) => {
                let qualified = self.facts.qualified_current(name);
                for field in fields {
                    let field_go = self.facts.declared_field_go_name(&qualified, field);
                    members.entry(field_go).or_default().push(field.name_span);
                }
            }
            StructFields::Tuple(fields) => {
                let is_newtype = self.facts.is_newtype_definition(name);
                if !fields.is_empty() && !is_newtype {
                    for (index, field) in fields.iter().enumerate() {
                        members
                            .entry(go_name::tuple_struct_field(index))
                            .or_default()
                            .push(field.name_span);
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
                members.entry(go_name).or_default().push(*name_span);
            }
        }
    }

    fn check_generic_names(&self, files: &[&File]) -> Vec<LisetteDiagnostic> {
        let mut diagnostics = Vec::new();
        for item in files.iter().flat_map(|file| &file.items) {
            match item {
                Expression::Function { generics, .. } => {
                    self.check_reserved_qualifier_generics(generics, &mut diagnostics);
                }
                Expression::TypeAlias { generics, .. }
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
        let mut emitted: SpanMap = HashMap::default();
        for generic in generics {
            check_reserved_qualifier(&generic.name, &generic.span, out);
            emitted
                .entry(self.generic_go_name(&generic.name).into_owned())
                .or_default()
                .push(generic.span);
        }
        report_collisions(emitted, out);
    }

    fn check_free_function_generics(
        &self,
        impl_generics: &[Generic],
        method_generics: &[Generic],
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
            check_reserved_qualifier(&generic.name, &generic.span, out);
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
        let [first, second, rest @ ..] = spans.as_slice() else {
            continue;
        };
        diagnostics.push(emit_diag::go_name_collision(&go, first, second, rest, None));
    }
}

fn sort_by_key(map: HashMap<String, SpanMap>) -> Vec<(String, SpanMap)> {
    let mut entries: Vec<(String, SpanMap)> = map.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn collect_enum_payload_fields(
    name: &str,
    variants: &[EnumVariant],
    members: &mut SpanMap,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let slots = go_names::enum_field_slots(name, variants);
    let mut seen: HashSet<String> = HashSet::default();
    for (variant_index, variant) in variants.iter().enumerate() {
        let field_names = &slots[variant_index];
        for field_name in field_names {
            if seen.insert(field_name.clone()) {
                members
                    .entry(field_name.clone())
                    .or_default()
                    .push(variant.name_span);
            }
        }
        if let VariantFields::Struct(fields) = &variant.fields {
            let mut variant_fields: SpanMap = HashMap::default();
            for (field, field_name) in fields.iter().zip(field_names) {
                variant_fields
                    .entry(field_name.clone())
                    .or_default()
                    .push(field.name_span);
            }
            report_collisions(variant_fields, diagnostics);
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

fn collect_const(
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
    let go = go_name::screaming_snake_to_camel(identifier);
    check_reserved_prefix(identifier, identifier_span, diagnostics);
    check_reserved_prefix(&go, identifier_span, diagnostics);
    package_block.entry(go).or_default().push(*identifier_span);
}

fn collect_type_alias(
    facts: &EmitFacts<'_>,
    item: &Expression,
    package_block: &mut SpanMap,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::TypeAlias {
        name, name_span, ..
    } = item
    else {
        return;
    };
    if facts.alias_emits_nothing(name) {
        return;
    }
    let go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&go, name_span, diagnostics);
    check_reserved_qualifier(&go, name_span, diagnostics);
    package_block.entry(go).or_default().push(*name_span);
}

fn collect_import_aliases(
    import_qualifiers: &[(String, Span)],
    package_block: &mut SpanMap,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    for (qualifier, span) in import_qualifiers {
        check_reserved_prefix(qualifier, span, diagnostics);
        if let Some(spans) = package_block.get_mut(qualifier)
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
    item: &Expression,
    package_block: &mut SpanMap,
    selectors: &mut HashMap<String, SpanMap>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::ImplBlock {
        receiver_name,
        methods,
        ..
    } = item
    else {
        return;
    };
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
            check_reserved_prefix(name, name_span, diagnostics);
            check_reserved_prefix(&method_go, name_span, diagnostics);
            selectors
                .entry(type_go.clone())
                .or_default()
                .entry(method_go)
                .or_default()
                .push(*name_span);
            continue;
        } else {
            go_name::associated_function_go_name(receiver_name, name, is_public)
        };
        check_reserved_prefix(name, name_span, diagnostics);
        check_reserved_prefix(&go, name_span, diagnostics);
        package_block.entry(go).or_default().push(*name_span);
    }
}

fn collect_interface(
    facts: &EmitFacts<'_>,
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
        ..
    } = item
    else {
        return;
    };
    let type_go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&type_go, name_span, diagnostics);
    check_reserved_qualifier(&type_go, name_span, diagnostics);
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
            let method_go = facts.interface_method_go_name(method_name, is_public);
            methods.entry(method_go).or_default().push(*method_span);
        }
    }
}

fn collect_enum(
    facts: &EmitFacts<'_>,
    item: &Expression,
    package_block: &mut SpanMap,
    selectors: &mut HashMap<String, SpanMap>,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
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
    let type_go = go_name::escape_type_name(name).into_owned();
    check_reserved_prefix(&type_go, name_span, diagnostics);
    check_reserved_qualifier(&type_go, name_span, diagnostics);

    if attributes
        .iter()
        .any(|attribute| attribute.name == "iterate")
    {
        let variants_fn = go_name::associated_function_go_name(
            name,
            "variants",
            matches!(visibility, Visibility::Public),
        );
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
        .entry(go_name::enum_tag_type(name))
        .or_default()
        .push(*name_span);
    for variant in variants {
        let tag_constant = go_name::enum_tag_constant(name, &variant.name);
        package_block
            .entry(tag_constant)
            .or_default()
            .push(variant.name_span);
        let constructor = go_name::enum_make_function(name, &variant.name);
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
    for method in facts.enum_synthesized_methods(name, attributes) {
        for go_name in facts.synthesized_method_go_names(method) {
            members.entry(go_name).or_default().push(*name_span);
        }
    }
    collect_enum_payload_fields(name, variants, members, diagnostics);
}
