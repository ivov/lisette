use diagnostics::{LisetteDiagnostic, LocalSink};
use semantics::{checker::TaskState, module_graph::build_module_graph, store::Store};
use stdlib::get_go_stdlib_typedef;
use syntax::{ast::Expression, types::Type};

use super::init_prelude;

use super::builders::*;
use super::filesystem::MockFileSystem;
use super::pipeline::TestPipeline;
use super::register_test_builtins;

pub fn infer(raw_source: &str) -> InferResult {
    let result = TestPipeline::new(raw_source)
        .wrapped()
        .compile()
        .run_inference();

    InferResult {
        ast: result.ast,
        errors: result.errors,
    }
}

pub fn infer_module(module_name: &str, fs: MockFileSystem) -> InferResult {
    let available_folders = fs.get_folders();

    let mut store = Store::new();

    store.module_ids.extend(available_folders);

    let sink = LocalSink::new();

    let locator = deps::TypedefLocator::default();
    let mut graph_result =
        build_module_graph(&mut store, Some(&fs), module_name, &sink, false, &locator);

    if sink.has_errors() {
        return InferResult {
            ast: vec![],
            errors: sink.take(),
        };
    }

    init_prelude(&mut store);

    let ast = {
        let mut checker = TaskState::with_fresh_allocator(&sink);
        checker
            .ufcs_methods
            .extend(semantics::prelude::compute_prelude_ufcs(&store));
        register_test_builtins(&mut store, &mut checker);
        checker.put_prelude_in_scope(&store);

        let order = std::mem::take(&mut graph_result.order);
        for module_id in order {
            if let Some(go_pkg) = module_id.strip_prefix("go:") {
                if let Some(typedef) = get_go_stdlib_typedef(go_pkg) {
                    checker.parse_and_register_go_module(&mut store, &module_id, typedef, &locator);
                }
                continue;
            }

            if store.is_visited(&module_id) {
                continue;
            }

            let files = graph_result.files.remove(&module_id).unwrap_or_default();

            let prev_module_id = checker.cursor.module_id.clone();
            checker.cursor.module_id = module_id.to_string();

            store.store_module(&module_id, files);
            checker.register_module(&mut store, &module_id);
            checker.infer_module(&mut store, &module_id);

            checker.cursor.module_id = prev_module_id;
        }

        for (module_id, typed_file) in std::mem::take(&mut checker.typed_files) {
            store.store_file(&module_id, typed_file);
        }

        let module = store.get_module(module_name).unwrap();
        let ast: Vec<_> = module
            .files
            .values()
            .flat_map(|f| f.items.clone())
            .collect();

        if !checker.failed() {
            let analysis = semantics::context::AnalysisContext::new(&store, &checker.ufcs_methods);
            let mut unused = syntax::program::UnusedInfo::default();
            semantics::validators::run(
                &analysis,
                &mut checker.facts,
                checker.sink,
                &mut unused,
                false,
            );
        }

        ast
    };

    InferResult {
        ast,
        errors: sink.take(),
    }
}

pub struct InferResult {
    pub ast: Vec<Expression>,
    pub errors: Vec<LisetteDiagnostic>,
}

impl InferResult {
    pub fn assert_type(self, expected: Type) -> Self {
        ensure_no_errors(&self.errors);

        let actual = self
            .get_expression_type_at(0)
            .unwrap_or_else(|| panic!("No expression found at index 0"));

        if !types_equal(&actual, &expected) {
            panic!(
                "Type mismatch at expression 0\nExpected: {}\nActual:   {}",
                expected.stringify(),
                actual.stringify()
            );
        }

        self
    }

    pub fn assert_last_type(self, expected: Type) -> Self {
        ensure_no_errors(&self.errors);

        let last_index = self.ast.len().saturating_sub(1);
        let actual = self
            .get_expression_type_at(last_index)
            .unwrap_or_else(|| panic!("No expression found at index {}", last_index));

        if !types_equal(&actual, &expected) {
            panic!(
                "Type mismatch at expression {}\nExpected: {}\nActual: {}",
                last_index,
                expected.stringify(),
                actual.stringify()
            );
        }

        self
    }

    pub fn assert_type_int(self) -> Self {
        self.assert_type(int_type())
    }

    pub fn assert_type_bool(self) -> Self {
        self.assert_type(bool_type())
    }

    pub fn assert_type_string(self) -> Self {
        self.assert_type(string_type())
    }

    pub fn assert_type_unit(self) -> Self {
        self.assert_type(unit_type())
    }

    pub fn assert_type_float(self) -> Self {
        self.assert_type(float_type())
    }

    pub fn assert_type_char(self) -> Self {
        self.assert_type(rune_type())
    }

    pub fn assert_type_tuple(self, t1: Type, t2: Type) -> Self {
        self.assert_type(tuple_type(vec![t1, t2]))
    }

    pub fn assert_type_slice_of(self, element_type: Type) -> Self {
        self.assert_type(slice_type(element_type))
    }

    pub fn assert_type_empty_slice(self) -> Self {
        let actual = self
            .get_expression_type_at(0)
            .unwrap_or_else(|| panic!("No expression found at index 0"));

        if !is_slice_with_type_var(&actual) {
            panic!(
                "Expected Slice with type variable, got {}",
                actual.stringify()
            );
        }

        self
    }

    pub fn assert_type_slice_of_ints(self) -> Self {
        self.assert_type_slice_of(int_type())
    }

    pub fn assert_type_slice_of_strings(self) -> Self {
        self.assert_type_slice_of(string_type())
    }

    pub fn assert_type_slice_of_booleans(self) -> Self {
        self.assert_type_slice_of(bool_type())
    }

    pub fn assert_function_type(self, takes: Vec<Type>, returns: Type) -> Self {
        self.assert_type(fun_type(takes, returns))
    }

    pub fn assert_last_function_type(self, takes: Vec<Type>, returns: Type) -> Self {
        self.assert_last_type(fun_type(takes, returns))
    }

    pub fn assert_type_struct(self, name: &str) -> Self {
        self.assert_type(con_type(name, vec![]))
    }

    pub fn assert_type_struct_generic(self, name: &str, generics: Vec<Type>) -> Self {
        self.assert_type(con_type(name, generics))
    }

    pub fn assert_no_errors(self) -> Self {
        ensure_no_errors(&self.errors);
        self
    }

    pub fn assert_resolve_code(self, code: &str) -> Self {
        self.assert_code(&format!("resolve.{}", code))
    }

    pub fn assert_infer_code(self, code: &str) -> Self {
        self.assert_code(&format!("infer.{}", code))
    }

    fn assert_code(self, expected_code: &str) -> Self {
        if self.errors.is_empty() {
            panic!("Expected errors, but inference succeeded");
        }

        let has_code = self.errors.iter().any(|err| {
            err.code_str()
                .map(|code| code == expected_code)
                .unwrap_or(false)
        });

        if !has_code {
            let actual_codes: Vec<&str> = self
                .errors
                .iter()
                .filter_map(|err| err.code_str())
                .collect();
            panic!(
                "Expected error code '{}', but got codes: {:?}\nFull errors:\n{}",
                expected_code,
                actual_codes,
                format_errors(&self.errors)
            );
        }

        self
    }

    pub fn assert_type_mismatch(self) -> Self {
        self.assert_error_contains("type mismatch")
    }

    pub fn assert_circular_type(self) -> Self {
        self.assert_resolve_code("circular_type_alias")
    }

    pub fn assert_not_found(self) -> Self {
        self.assert_error_contains("not found")
    }

    pub fn assert_exhaustiveness_error(self) -> Self {
        self.assert_error_contains("not exhaustive")
    }

    pub fn assert_redundancy_error(self) -> Self {
        self.assert_error_contains("redundant")
    }

    fn assert_error_contains(self, needle: &str) -> Self {
        if self.errors.is_empty() {
            panic!("Expected errors, but inference succeeded");
        }

        let errors_str = format_errors(&self.errors);
        if !errors_str
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        {
            panic!(
                "Expected error to contain '{}', but got:\n{}",
                needle, errors_str
            );
        }

        self
    }

    fn get_expression_type_at(&self, index: usize) -> Option<Type> {
        self.ast
            .get(index)
            .map(|expression| expression.get_type().clone())
    }
}

fn format_errors(errors: &[LisetteDiagnostic]) -> String {
    errors
        .iter()
        .map(|e| format!("{:?}", e))
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn ensure_no_errors(errors: &[LisetteDiagnostic]) {
    if !errors.is_empty() {
        panic!("Expected no errors, but got:\n{}", format_errors(errors));
    }
}

fn is_slice_with_type_var(ty: &Type) -> bool {
    match ty {
        Type::Nominal { id, params, .. } => {
            id.rsplit('.').next().unwrap_or("") == "Slice"
                && params.len() == 1
                && matches!(params[0], Type::Var { .. })
        }
        Type::Compound {
            kind: syntax::types::CompoundKind::Slice,
            args,
        } => args.len() == 1 && matches!(args[0], Type::Var { .. }),
        _ => false,
    }
}

fn types_equal(t1: &Type, t2: &Type) -> bool {
    if let (Some(n1), Some(n2)) = (t1.get_name(), t2.get_name())
        && n1 == n2
    {
        let args1 = t1.get_type_params().unwrap_or(&[]);
        let args2 = t2.get_type_params().unwrap_or(&[]);
        if args1.len() == args2.len()
            && args1
                .iter()
                .zip(args2.iter())
                .all(|(a1, a2)| types_equal(a1, a2))
        {
            return true;
        }
    }

    match (t1, t2) {
        (Type::Compound { kind, args }, Type::Nominal { id, params, .. })
        | (Type::Nominal { id, params, .. }, Type::Compound { kind, args }) => {
            let leaf = id.rsplit('.').next().unwrap_or("");
            if kind.leaf_name() == leaf && args.len() == params.len() {
                return args
                    .iter()
                    .zip(params.iter())
                    .all(|(x, y)| types_equal(x, y));
            }
        }
        (Type::Simple(kind), Type::Nominal { id, params, .. })
        | (Type::Nominal { id, params, .. }, Type::Simple(kind)) => {
            let leaf = id.rsplit('.').next().unwrap_or("");
            if kind.leaf_name() == leaf && params.is_empty() {
                return true;
            }
        }
        _ => {}
    }

    if let Type::Nominal {
        underlying_ty: Some(u),
        ..
    } = t1
        && types_equal(u, t2)
    {
        return true;
    }
    if let Type::Nominal {
        underlying_ty: Some(u),
        ..
    } = t2
        && types_equal(t1, u)
    {
        return true;
    }

    match (t1, t2) {
        (Type::Var { .. }, Type::Var { .. }) => true,

        (
            Type::Nominal {
                id: id1,
                params: args1,
                underlying_ty: u1,
            },
            Type::Nominal {
                id: id2,
                params: args2,
                ..
            },
        ) => {
            let name1 = id1.rsplit('.').next().unwrap_or("");
            let name2 = id2.rsplit('.').next().unwrap_or("");
            if name1 == name2
                && args1.len() == args2.len()
                && args1
                    .iter()
                    .zip(args2.iter())
                    .all(|(a1, a2)| types_equal(a1, a2))
            {
                return true;
            }
            if let Some(u) = u1
                && types_equal(u, t2)
            {
                return true;
            }
            false
        }

        (
            Type::Function {
                params: args1,
                return_type: ret1,
                ..
            },
            Type::Function {
                params: args2,
                return_type: ret2,
                ..
            },
        ) => {
            args1.len() == args2.len()
                && args1
                    .iter()
                    .zip(args2.iter())
                    .all(|(a1, a2)| types_equal(a1, a2))
                && types_equal(ret1, ret2)
        }

        (Type::Tuple(elems1), Type::Tuple(elems2)) => {
            elems1.len() == elems2.len()
                && elems1
                    .iter()
                    .zip(elems2.iter())
                    .all(|(e1, e2)| types_equal(e1, e2))
        }

        (Type::Simple(k1), Type::Simple(k2)) => k1 == k2,

        (Type::Compound { kind: k1, args: a1 }, Type::Compound { kind: k2, args: a2 }) => {
            k1 == k2
                && a1.len() == a2.len()
                && a1.iter().zip(a2.iter()).all(|(x, y)| types_equal(x, y))
        }

        _ => false,
    }
}
