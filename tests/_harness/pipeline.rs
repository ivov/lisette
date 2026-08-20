use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::LisetteDiagnostic;
use semantics::checker::TaskState;
use semantics::store::Store;
use std::mem;
use stdlib::{Target, get_go_stdlib_typedef};
use syntax::types;
use syntax::{
    ast::{Expression, FunctionBody},
    lex::Lexer,
    parse::Parser,
    program::{Definition, EqualityIndex, File, MutationInfo, UnusedInfo},
    types::Symbol,
};

use super::new_test_store;

use super::TEST_PACKAGE_ID;
use super::wrap::{TEST_WRAPPER_NAME, wrap};

pub(super) const TEST_FILE_ID: u32 = 0;

pub(super) struct InferredTestFile {
    pub(super) store: Store,
    pub(super) checker: TaskState,
}

/// Runs a synthetic single-file package through the production registration and inference path.
pub(super) fn infer_test_file(
    source: &str,
    ast: Vec<Expression>,
    extra_go_typedefs: &[(String, String)],
) -> InferredTestFile {
    let mut store = new_test_store();
    store.add_package(TEST_PACKAGE_ID);

    let mut checker = TaskState::for_package(TEST_PACKAGE_ID);
    let locator = deps::TypedefLocator::default();

    for (name, typedef) in extra_go_typedefs {
        checker.parse_and_register_go_package(&mut store, name, typedef, None, &locator);
    }

    for item in &ast {
        if let Expression::PackageImport { name, .. } = item
            && let Some(go_pkg) = name.strip_prefix("go:")
            && let Some(typedef) = get_go_stdlib_typedef(go_pkg, Target::host())
        {
            checker.parse_and_register_go_package(&mut store, name, typedef, None, &locator);
        }
    }

    store.store_file(File {
        id: TEST_FILE_ID,
        package_id: TEST_PACKAGE_ID.to_string(),
        parse_status: syntax::FileParseStatus::Clean,
        name: "test.lis".to_string(),
        display_path: "test.lis".to_string(),
        source_path: None,
        source: source.to_string(),
        items: ast,
        file_comment: None,
    });

    let package = checker.register_package(&mut store, TEST_PACKAGE_ID);
    checker.finalize_registration(&mut store);
    checker.infer_package(&mut store, package);
    checker.check_post_inference_bounds(&store);

    InferredTestFile { store, checker }
}

pub struct TestPipeline {
    source: String,
    raw_source: String,
    wrapped: bool,
    e2e_suite_mode: bool,
    extra_go_typedefs: Vec<(String, String)>,
}

impl TestPipeline {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            raw_source: source.to_string(),
            wrapped: false,
            e2e_suite_mode: false,
            extra_go_typedefs: Vec::new(),
        }
    }

    pub fn wrapped(mut self) -> Self {
        self.wrapped = true;
        self.source = wrap(&self.raw_source);
        self
    }

    /// Keep the `__test__` wrapper in the typed AST so it is emitted as a callable Go fn.
    #[allow(dead_code)]
    pub fn e2e_suite_mode(mut self) -> Self {
        self.e2e_suite_mode = true;
        self
    }

    pub fn with_go_typedef(mut self, package_name: &str, typedef_source: &str) -> Self {
        self.extra_go_typedefs
            .push((package_name.to_string(), typedef_source.to_string()));
        self
    }

    pub fn compile(self) -> CompiledTest {
        let lex_result = Lexer::new(&self.source, TEST_FILE_ID).lex();
        if lex_result.failed() {
            panic!("Lexing failed in test: {:?}", lex_result.errors);
        }

        let parse_result = Parser::new(lex_result.tokens, &self.source).parse();
        if parse_result.has_errors() {
            panic!("Parsing failed in test: {:?}", parse_result.errors);
        }

        CompiledTest {
            ast: parse_result.ast,
            source: self.source,
            wrapped: self.wrapped,
            e2e_suite_mode: self.e2e_suite_mode,
            extra_go_typedefs: self.extra_go_typedefs,
        }
    }
}

pub struct CompiledTest {
    ast: Vec<Expression>,
    source: String,
    wrapped: bool,
    e2e_suite_mode: bool,
    extra_go_typedefs: Vec<(String, String)>,
}

impl CompiledTest {
    pub fn run_inference(self) -> InferenceResult {
        let Self {
            ast,
            source,
            wrapped,
            e2e_suite_mode,
            extra_go_typedefs,
        } = self;
        let InferredTestFile { mut store, checker } =
            infer_test_file(&source, ast, &extra_go_typedefs);

        let (
            typed_ast,
            errors,
            definitions,
            unused,
            mutations,
            equality_index,
            go_package_names,
            go_package_ids,
        ) = {
            let mut typed_ast = store
                .get_file(TEST_FILE_ID)
                .expect("inferred test file must remain in the store")
                .items
                .clone();
            if !checker.failed() {
                passes::run(
                    &store,
                    &checker.facts,
                    &checker.sink,
                    passes::LintMode::Skip,
                    passes::UnusedItemReporting::Report,
                );
            }

            if wrapped && !e2e_suite_mode {
                let has_hoisted = typed_ast.len() > 1
                    && typed_ast.iter().any(|expr| {
                        matches!(expr, Expression::Function { name, .. } if name == TEST_WRAPPER_NAME)
                    });

                if has_hoisted {
                    typed_ast = typed_ast
                        .into_iter()
                        .filter_map(|expr| {
                            if let Expression::Function { ref name, .. } = expr
                                && name == TEST_WRAPPER_NAME
                            {
                                return unwrap_test_wrapper(expr);
                            }
                            None
                        })
                        .collect();
                } else {
                    typed_ast = typed_ast
                        .into_iter()
                        .filter_map(unwrap_test_wrapper)
                        .collect();
                }
            }

            let definitions: HashMap<Symbol, Definition> = store
                .packages
                .values()
                .flat_map(|m| m.definitions.iter())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let mut unused = UnusedInfo::default();
            let mut mutations = MutationInfo::default();
            for (&binding_id, b) in checker.facts.bindings.iter() {
                if !b.used {
                    unused.mark_binding_unused(b.span);
                }
                if let Some(mutation) = b.mutation {
                    mutations.record(binding_id, mutation);
                }
            }

            let equality_index = mem::take(&mut store.equality_index);
            let go_package_names = store.go_package_names.clone();
            let go_package_ids: HashSet<String> = store
                .packages
                .keys()
                .filter(|id| id.starts_with(types::GO_IMPORT_PREFIX))
                .cloned()
                .collect();

            let errors = checker.sink.into_diagnostics();

            (
                typed_ast,
                errors,
                definitions,
                unused,
                mutations,
                equality_index,
                go_package_names,
                go_package_ids,
            )
        };

        InferenceResult {
            ast: typed_ast,
            errors,
            definitions,
            package_id: TEST_PACKAGE_ID.to_string(),
            unused,
            mutations,
            equality_index,
            go_package_names,
            go_package_ids,
        }
    }
}

pub struct InferenceResult {
    pub ast: Vec<Expression>,
    pub errors: Vec<LisetteDiagnostic>,
    pub definitions: HashMap<Symbol, Definition>,
    pub package_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub equality_index: EqualityIndex,
    pub go_package_names: HashMap<String, String>,
    pub go_package_ids: HashSet<String>,
}

fn unwrap_test_wrapper(expression: Expression) -> Option<Expression> {
    let Expression::Function { name, .. } = &expression else {
        return Some(expression);
    };

    if name != TEST_WRAPPER_NAME {
        return Some(expression);
    }

    let Expression::Function { body, .. } = expression else {
        unreachable!()
    };

    let FunctionBody::Definition(body) = body else {
        panic!("Expected definition for {TEST_WRAPPER_NAME} wrapper function");
    };
    let Expression::Block { items, .. } = *body else {
        panic!(
            "Expected Block as body of {} wrapper function",
            TEST_WRAPPER_NAME
        );
    };

    items.into_iter().next_back()
}

#[cfg(test)]
mod tests {
    use super::TestPipeline;

    #[test]
    fn production_file_checks_reject_definitions_that_shadow_imports() {
        let result = TestPipeline::new(
            r#"
import "go:fmt"

fn fmt() {}
"#,
        )
        .compile()
        .run_inference();

        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code_str() == Some("resolve.name_shadows_import")),
            "expected name_shadows_import, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn production_file_checks_reject_reference_aliasing_between_siblings() {
        let result = TestPipeline::new(
            r#"
fn bump(value: mut Ref<int>) -> int {
  value.* = value.* + 1
  value.*
}

fn main() {
  let mut value = 1
  let pair = (bump(&value), value)
  let _ = pair
}
"#,
        )
        .compile()
        .run_inference();

        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code_str() == Some("infer.reference_aliases_sibling")),
            "expected reference_aliases_sibling, got: {:?}",
            result.errors
        );
    }
}
