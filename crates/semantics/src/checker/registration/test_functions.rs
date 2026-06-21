use diagnostics::LocalSink;
use syntax::ast::{Annotation, Attribute, AttributeArg, Binding, Expression};
use syntax::attributes::test_attribute;
use syntax::program::TestFunction;
use syntax::types::{Symbol, Type};

use super::TaskState;
use crate::store::Store;

pub(crate) fn test_context_type() -> Type {
    Type::Nominal {
        id: Symbol::from_parts(crate::prelude::TEST_PRELUDE_MODULE_ID, "TestContext"),
        params: vec![],
        underlying_ty: None,
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

impl TaskState<'_> {
    /// Collect and validate a module's `#[test]` functions into `facts`
    /// (merge-safe, since this runs during parallel registration).
    pub(super) fn register_module_tests(&mut self, store: &Store, module_id: &str) {
        let module = store.get_module(module_id).expect("module must exist");
        let context_shadowed = module_shadows_test_context(store, module_id);
        let mut records: Vec<TestFunction> = Vec::new();
        for file in module.files.values().chain(module.typedefs.values()) {
            let in_test_file = file.is_test();
            for item in &file.items {
                collect_test_candidates(
                    item,
                    module_id,
                    in_test_file,
                    context_shadowed,
                    &mut records,
                    self.sink,
                );
            }
        }
        self.facts.test_functions.extend(records);
    }

    pub(crate) fn collect_cached_module_tests(&mut self, store: &Store, module_id: &str) {
        let Some(module) = store.get_module(module_id) else {
            return;
        };
        let context_shadowed = module_shadows_test_context(store, module_id);
        let discard = LocalSink::new();
        let mut records: Vec<TestFunction> = Vec::new();
        for file in module.files.values() {
            if !file.is_test() {
                continue;
            }
            let parsed = syntax::build_ast(&file.source, file.id);
            for item in &parsed.ast {
                collect_test_candidates(
                    item,
                    module_id,
                    true,
                    context_shadowed,
                    &mut records,
                    &discard,
                );
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

fn module_shadows_test_context(store: &Store, module_id: &str) -> bool {
    let qualified = format!("{module_id}.TestContext");
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
    module_id: &str,
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
