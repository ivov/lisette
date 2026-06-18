use diagnostics::LocalSink;
use syntax::ast::{Annotation, Attribute, AttributeArg, Expression};
use syntax::attributes::test_attribute;
use syntax::program::TestFunction;

use super::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    /// Collect and validate a module's `#[test]` functions into `facts`
    /// (merge-safe, since this runs during parallel registration).
    pub(super) fn register_module_tests(&mut self, store: &Store, module_id: &str) {
        let module = store.get_module(module_id).expect("module must exist");
        let mut records: Vec<TestFunction> = Vec::new();
        for file in module.files.values().chain(module.typedefs.values()) {
            let in_test_file = file.is_test();
            for item in &file.items {
                collect_test_candidates(item, module_id, in_test_file, &mut records, self.sink);
            }
        }
        self.facts.test_functions.extend(records);
    }

    pub(crate) fn collect_cached_module_tests(&mut self, store: &Store, module_id: &str) {
        let Some(module) = store.get_module(module_id) else {
            return;
        };
        let discard = LocalSink::new();
        let mut records: Vec<TestFunction> = Vec::new();
        for file in module.files.values() {
            if !file.is_test() {
                continue;
            }
            let parsed = syntax::build_ast(&file.source, file.id);
            for item in &parsed.ast {
                collect_test_candidates(item, module_id, true, &mut records, &discard);
            }
        }
        self.facts.test_functions.extend(records);
    }

    pub fn finalize_tests(&mut self, store: &mut Store) {
        for test in std::mem::take(&mut self.facts.test_functions) {
            store.test_index.push(test);
        }
    }
}

fn flag_misplaced(attributes: &[Attribute], sink: &LocalSink) {
    if let Some(attribute) = test_attribute(attributes) {
        sink.push(diagnostics::attribute::test_not_on_function(
            &attribute.span,
        ));
    }
}

fn flag_misplaced_methods(methods: &[Expression], sink: &LocalSink) {
    for method in methods {
        if let Expression::Function { attributes, .. } = method {
            flag_misplaced(attributes, sink);
        }
    }
}

fn is_unit_return(annotation: &Annotation) -> bool {
    match annotation {
        Annotation::Unknown => true,
        Annotation::Tuple { elements, .. } => elements.is_empty(),
        _ => false,
    }
}

fn parse_title(args: &[AttributeArg]) -> Result<Option<String>, ()> {
    match args {
        [] => Ok(None),
        [AttributeArg::String(title)] => Ok(Some(title.clone())),
        _ => Err(()),
    }
}

fn collect_test_candidates(
    item: &Expression,
    module_id: &str,
    in_test_file: bool,
    records: &mut Vec<TestFunction>,
    sink: &LocalSink,
) {
    match item {
        Expression::Function {
            attributes,
            name,
            name_span,
            doc,
            generics,
            params,
            return_annotation,
            ..
        } => {
            let Some(attribute) = test_attribute(attributes) else {
                return;
            };
            if !in_test_file {
                sink.push(diagnostics::attribute::test_outside_test_file(
                    &attribute.span,
                ));
                return;
            }
            let Ok(title) = parse_title(&attribute.args) else {
                sink.push(diagnostics::attribute::test_invalid_argument(
                    &attribute.span,
                ));
                return;
            };
            if !generics.is_empty() || !params.is_empty() || !is_unit_return(return_annotation) {
                sink.push(diagnostics::attribute::test_unsupported_signature(
                    name_span,
                ));
                return;
            }
            records.push(TestFunction {
                module_id: module_id.to_string(),
                qualified_name: format!("{}.{}", module_id, name),
                title,
                doc: doc.clone(),
                span: *name_span,
            });
        }
        Expression::Struct {
            attributes, fields, ..
        } => {
            flag_misplaced(attributes, sink);
            for field in fields {
                flag_misplaced(&field.attributes, sink);
            }
        }
        Expression::Enum { attributes, .. } => flag_misplaced(attributes, sink),
        Expression::TypeAlias { attributes, .. } => flag_misplaced(attributes, sink),
        Expression::Interface {
            method_signatures, ..
        } => flag_misplaced_methods(method_signatures, sink),
        Expression::ImplBlock { methods, .. } => flag_misplaced_methods(methods, sink),
        _ => {}
    }
}
