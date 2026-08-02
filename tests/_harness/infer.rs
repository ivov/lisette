use diagnostics::{LisetteDiagnostic, LocalSink};
use rustc_hash::FxHashMap as HashMap;
use semantics::loader::Loader;
use semantics::{
    checker::TaskState,
    module_graph::Roots,
    module_graph::{ModuleGraphOptions, build_module_graph},
};
use stdlib::{Target, get_go_stdlib_typedef};
use syntax::{
    ast::Expression,
    program::Definition,
    types::{Symbol, Type},
};

use super::new_test_store;

use super::builders::*;
use super::filesystem::MockFileSystem;
use super::pipeline::TestPipeline;
pub fn checker_errors(raw_source: &str, typedefs: &[(&str, &str)]) -> Vec<LisetteDiagnostic> {
    let mut pipeline = TestPipeline::new(raw_source);
    for (name, source) in typedefs {
        pipeline = pipeline.with_go_typedef(name, source);
    }
    pipeline.compile().run_inference().errors
}

pub fn infer(raw_source: &str) -> InferResult {
    let result = TestPipeline::new(raw_source)
        .wrapped()
        .compile()
        .run_inference();

    InferResult {
        ast: result.ast,
        errors: result.errors,
        definitions: result.definitions,
    }
}

pub fn infer_with_go_typedefs(raw_source: &str, typedefs: &[(&str, &str)]) -> InferResult {
    let mut pipeline = TestPipeline::new(raw_source).wrapped();
    for (name, source) in typedefs {
        pipeline = pipeline.with_go_typedef(name, source);
    }
    let result = pipeline.compile().run_inference();

    InferResult {
        ast: result.ast,
        errors: result.errors,
        definitions: result.definitions,
    }
}

pub fn infer_module(module_name: &str, fs: MockFileSystem) -> InferResult {
    let mut store = new_test_store();

    let sink = LocalSink::new();

    let locator = deps::TypedefLocator::default();
    let discovered = fs.discover_modules();
    let additional = discovered.test_roots().cloned().collect();
    let roots = Roots {
        primary: vec![module_name.to_string()],
        additional,
    };
    let mut graph_result = build_module_graph(
        &mut store,
        roots,
        ModuleGraphOptions {
            loader: Some(&fs),
            sink: &sink,
            scope: &semantics::AnalysisScope::Project(std::path::PathBuf::new()),
            locator: &locator,
            include_tests: true,
            project_kind: semantics::ProjectKind::Binary,
        },
    );

    let mut parsed: HashMap<String, Vec<syntax::program::File>> = graph_result
        .files
        .drain()
        .map(|(module_id, files)| {
            let files = files
                .into_iter()
                .map(|file| {
                    let (file, errors) = file.parse();
                    sink.extend_parse_errors(errors);
                    file
                })
                .collect();
            (module_id, files)
        })
        .collect();

    if sink.has_errors() {
        return InferResult {
            ast: vec![],
            errors: sink.into_diagnostics(),
            definitions: HashMap::default(),
        };
    }

    let ast = {
        let mut checker = TaskState::with_fresh_allocator();
        checker.put_prelude_in_scope(&store);

        let order = std::mem::take(&mut graph_result.order);
        let mut to_infer: Vec<String> = Vec::new();
        for module_id in order {
            if let Some(go_pkg) = module_id.strip_prefix("go:") {
                if let Some(typedef) = get_go_stdlib_typedef(go_pkg, Target::host()) {
                    checker.parse_and_register_go_module(
                        &mut store, &module_id, typedef, None, &locator,
                    );
                }
                continue;
            }

            let files = parsed.remove(&module_id).unwrap_or_default();

            store.store_module(&module_id, files);
            checker.register_module(&mut store, &module_id);

            to_infer.push(module_id);
        }

        checker.finalize_equality(&mut store);
        checker.check_pending_generic_bounds(&store);
        checker.finalize_tests(&mut store);

        for module_id in &to_infer {
            checker.infer_module(&mut store, module_id);
        }

        checker.check_post_inference_bounds(&store);

        let module = store.get_module(module_name).unwrap();
        let ast: Vec<_> = module
            .source_files()
            .flat_map(|f| f.items.clone())
            .collect();

        if !checker.failed() {
            let mut unused = syntax::program::UnusedInfo::default();
            passes::run(
                &store,
                &mut checker.facts,
                &checker.sink,
                &mut unused,
                false,
            );
        }

        sink.extend(checker.sink.into_diagnostics());
        ast
    };

    let definitions = store
        .modules
        .values()
        .flat_map(|module| module.definitions.iter())
        .map(|(name, definition)| (name.clone(), definition.clone()))
        .collect();

    InferResult {
        ast,
        errors: sink.into_diagnostics(),
        definitions,
    }
}

pub struct InferResult {
    pub ast: Vec<Expression>,
    pub errors: Vec<LisetteDiagnostic>,
    definitions: HashMap<Symbol, Definition>,
}

impl InferResult {
    pub fn assert_type(self, expected: Type) -> Self {
        ensure_no_errors(&self.errors);

        let actual = self
            .get_expression_type_at(0)
            .unwrap_or_else(|| panic!("No expression found at index 0"));

        if !types_equal(&actual, &expected, &self.definitions) {
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

        if !types_equal(&actual, &expected, &self.definitions) {
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

    pub fn assert_resolve_code_once(self, code: &str) -> Self {
        self.assert_code_count(&format!("resolve.{}", code), 1)
    }

    pub fn assert_infer_code_once(self, code: &str) -> Self {
        self.assert_code_count(&format!("infer.{}", code), 1)
    }

    pub fn assert_infer_code_count(self, code: &str, count: usize) -> Self {
        self.assert_code_count(&format!("infer.{}", code), count)
    }

    fn assert_code_count(self, expected_code: &str, expected: usize) -> Self {
        let count = self
            .errors
            .iter()
            .filter(|err| err.code_str() == Some(expected_code))
            .count();
        if count != expected {
            let actual_codes: Vec<&str> = self
                .errors
                .iter()
                .filter_map(|err| err.code_str())
                .collect();
            panic!(
                "Expected {} occurrence(s) of '{}', found {}. Codes: {:?}",
                expected, expected_code, count, actual_codes
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

    pub fn assert_error_contains(self, needle: &str) -> Self {
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

/// A consistent one-to-one renaming between the type variables of the two
/// types being compared, so `fn(a) -> a` and `fn(a) -> b` are not conflated.
#[derive(Default)]
struct VarBijection {
    forward: std::collections::HashMap<u32, u32>,
    backward: std::collections::HashMap<u32, u32>,
}

impl VarBijection {
    fn unify(&mut self, a: u32, b: u32) -> bool {
        *self.forward.entry(a).or_insert(b) == b && *self.backward.entry(b).or_insert(a) == a
    }
}

fn types_equal(t1: &Type, t2: &Type, definitions: &HashMap<Symbol, Definition>) -> bool {
    types_equal_with(t1, t2, definitions, &mut VarBijection::default())
}

fn types_equal_with(
    t1: &Type,
    t2: &Type,
    definitions: &HashMap<Symbol, Definition>,
    vars: &mut VarBijection,
) -> bool {
    let resolved1 = if matches!(t1, Type::Nominal { .. }) {
        syntax::types::peel_alias(t1, |id| definitions.get(id))
    } else {
        t1.clone()
    };
    let resolved2 = if matches!(t2, Type::Nominal { .. }) {
        syntax::types::peel_alias(t2, |id| definitions.get(id))
    } else {
        t2.clone()
    };
    let (t1, t2) = (&resolved1, &resolved2);

    if let (Some(n1), Some(n2)) = (t1.get_name(), t2.get_name())
        && n1 == n2
    {
        let args1 = t1.get_type_params().unwrap_or(&[]);
        let args2 = t2.get_type_params().unwrap_or(&[]);
        if args1.len() == args2.len()
            && args1
                .iter()
                .zip(args2.iter())
                .all(|(a1, a2)| types_equal_with(a1, a2, definitions, vars))
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
                    .all(|(x, y)| types_equal_with(x, y, definitions, vars));
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

    match (t1, t2) {
        (Type::Var { id: a, .. }, Type::Var { id: b, .. }) => vars.unify(a.index(), b.index()),

        (
            Type::Nominal {
                id: id1,
                params: args1,
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
                    .all(|(a1, a2)| types_equal_with(a1, a2, definitions, vars))
            {
                return true;
            }
            false
        }

        (Type::Function(f1), Type::Function(f2)) => {
            f1.params.len() == f2.params.len()
                && f1.params.iter().zip(f2.params.iter()).all(|(a1, a2)| {
                    a1.mutable == a2.mutable && types_equal_with(&a1.ty, &a2.ty, definitions, vars)
                })
                && types_equal_with(&f1.return_type, &f2.return_type, definitions, vars)
        }

        (Type::Tuple(elems1), Type::Tuple(elems2)) => {
            elems1.len() == elems2.len()
                && elems1
                    .iter()
                    .zip(elems2.iter())
                    .all(|(e1, e2)| types_equal_with(e1, e2, definitions, vars))
        }

        (Type::Simple(k1), Type::Simple(k2)) => k1 == k2,

        (Type::Compound { kind: k1, args: a1 }, Type::Compound { kind: k2, args: a2 }) => {
            k1 == k2
                && a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| types_equal_with(x, y, definitions, vars))
        }

        _ => false,
    }
}
