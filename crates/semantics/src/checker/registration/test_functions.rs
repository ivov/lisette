use diagnostics::LocalSink;
use syntax::ast::{Annotation, Attribute, AttributeArg, Binding, Expression};
use syntax::attributes::test_attribute;
use syntax::program::TestFunction;
use syntax::types::{Symbol, Type};

use super::{RegistrationFile, TaskState};
use crate::store::Store;

fn test_context_type() -> Type {
    Type::Nominal {
        id: Symbol::from_parts(crate::prelude::TEST_PRELUDE_PACKAGE_ID, "TestContext"),
        params: vec![],
    }
}

pub(crate) fn normalize_test_params(mut params: Vec<Binding>, is_test: bool) -> Vec<Binding> {
    if is_test
        && let [param] = params.as_mut_slice()
        && param.annotation.is_none()
    {
        param.ty = test_context_type();
    }
    params
}

impl TaskState {
    /// Collect and validate a package's `#[test]` functions for finalization.
    /// The pending records are merge-safe across parallel registration tasks.
    pub(super) fn register_package_tests(
        &mut self,
        store: &Store,
        package_id: &str,
        files: &[RegistrationFile],
    ) {
        let context_shadowed = package_shadows_test_context(store, package_id);
        let mut records: Vec<TestFunction> = Vec::new();
        for file in files {
            let in_test_file = store
                .get_file(file.id)
                .expect("registered file must remain in the store")
                .is_test();
            for item in &file.items {
                collect_test_candidates(
                    item,
                    package_id,
                    in_test_file,
                    context_shadowed,
                    &mut records,
                    &self.sink,
                );
            }
        }
        self.pending.test_functions.extend(records);
    }

    pub(crate) fn collect_cached_package_tests(&mut self, store: &Store, package_id: &str) {
        let Some(package) = store.get_package(package_id) else {
            return;
        };
        let context_shadowed = package_shadows_test_context(store, package_id);
        let discard = LocalSink::new();
        let mut records: Vec<TestFunction> = Vec::new();
        for file in package.files.values() {
            if !file.is_test() {
                continue;
            }
            let parsed = syntax::build_ast(&file.source, file.id);
            for item in &parsed.ast {
                collect_test_candidates(
                    item,
                    package_id,
                    true,
                    context_shadowed,
                    &mut records,
                    &discard,
                );
            }
        }
        self.pending.test_functions.extend(records);
    }

    pub(super) fn finalize_tests(&mut self, store: &mut Store) {
        for test in std::mem::take(&mut self.pending.test_functions) {
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

fn is_unit_annotation(annotation: &Annotation) -> bool {
    match annotation {
        Annotation::Unknown => true,
        Annotation::Tuple { elements, .. } => elements.is_empty(),
        Annotation::Constructor { name, params, .. } => name == "Unit" && params.is_empty(),
        _ => false,
    }
}

fn is_supported_return(annotation: &Annotation) -> bool {
    if is_unit_annotation(annotation) {
        return true;
    }
    matches!(
        annotation,
        Annotation::Constructor { name, params, .. }
            if name == "Result"
                && params.len() == 2
                && is_unit_annotation(&params[0])
                && matches!(&params[1], Annotation::Constructor { name, .. } if name == "error")
    )
}

fn package_shadows_test_context(store: &Store, package_id: &str) -> bool {
    let qualified = format!("{package_id}.TestContext");
    store
        .get_definition(&qualified)
        .is_some_and(|definition| !definition.is_value(&qualified))
}

fn params_supported(params: &[Binding], context_shadowed: bool) -> bool {
    match params {
        [] => true,
        [param] => match &param.annotation {
            None => true,
            Some(Annotation::Constructor { name, params, .. }) => {
                !context_shadowed && name == "TestContext" && params.is_empty()
            }
            _ => false,
        },
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
    package_id: &str,
    in_test_file: bool,
    context_shadowed: bool,
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
            if !generics.is_empty()
                || !params_supported(params, context_shadowed)
                || !is_supported_return(return_annotation)
            {
                sink.push(diagnostics::attribute::test_unsupported_signature(
                    name_span,
                ));
                return;
            }
            records.push(TestFunction::new(
                package_id,
                name,
                title,
                doc.clone(),
                *name_span,
            ));
        }
        Expression::Struct {
            attributes, fields, ..
        } => {
            flag_misplaced(attributes, sink);
            for field in fields {
                flag_misplaced(field.attributes(), sink);
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
