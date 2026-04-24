use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use diagnostics::{LisetteDiagnostic, LocalSink};
use semantics::{checker::TaskState, store::Store};
use stdlib::get_go_stdlib_typedef;
use syntax::{
    ast::Expression,
    desugar,
    lex::Lexer,
    parse::Parser,
    program::{Definition, File, FileImport, MutationInfo, UnusedInfo, Visibility},
    types::Symbol,
};

use super::init_prelude;

use crate::_harness::register_test_builtins;

use super::TEST_MODULE_ID;
use super::wrap::{TEST_WRAPPER_NAME, wrap};

pub struct TestPipeline {
    source: String,
    raw_source: String,
    wrapped: bool,
    extra_go_typedefs: Vec<(String, String)>,
}

impl TestPipeline {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            raw_source: source.to_string(),
            wrapped: false,
            extra_go_typedefs: Vec::new(),
        }
    }

    pub fn wrapped(mut self) -> Self {
        self.wrapped = true;
        self.source = wrap(&self.raw_source);
        self
    }

    pub fn with_go_typedef(mut self, module_name: &str, typedef_source: &str) -> Self {
        self.extra_go_typedefs
            .push((module_name.to_string(), typedef_source.to_string()));
        self
    }

    pub fn compile(self) -> CompiledTest {
        let lex_result = Lexer::new(&self.source, 0).lex();
        if lex_result.failed() {
            panic!("Lexing failed in test: {:?}", lex_result.errors);
        }

        let parse_result = Parser::new(lex_result.tokens, &self.source).parse();
        if parse_result.failed() {
            panic!("Parsing failed in test: {:?}", parse_result.errors);
        }

        let desugar_result = desugar::desugar(parse_result.ast);
        if !desugar_result.errors.is_empty() {
            panic!("Desugaring failed in test: {:?}", desugar_result.errors);
        }

        CompiledTest {
            ast: desugar_result.ast,
            wrapped: self.wrapped,
            extra_go_typedefs: self.extra_go_typedefs,
        }
    }
}

pub struct CompiledTest {
    ast: Vec<Expression>,
    wrapped: bool,
    extra_go_typedefs: Vec<(String, String)>,
}

impl CompiledTest {
    pub fn run_inference(self) -> InferenceResult {
        let mut store = Store::new();
        store.add_module(TEST_MODULE_ID);

        let sink = LocalSink::new();

        init_prelude(&mut store);

        let (typed_ast, definitions, unused, mutations, ufcs_methods, go_package_names) = {
            let mut checker = TaskState::with_fresh_allocator(&sink);
            checker
                .ufcs_methods
                .extend(semantics::prelude::compute_prelude_ufcs(&store));
            checker.cursor.module_id = TEST_MODULE_ID.to_string();
            register_test_builtins(&mut store, &mut checker);
            checker.put_prelude_in_scope(&store);

            let locator = deps::TypedefLocator::default();

            for (name, typedef) in &self.extra_go_typedefs {
                checker.parse_and_register_go_module(&mut store, name, typedef, &locator);
            }

            let imports: Vec<FileImport> = self
                .ast
                .iter()
                .filter_map(|item| {
                    if let Expression::ModuleImport {
                        name,
                        name_span,
                        alias,
                        span,
                    } = item
                    {
                        if let Some(go_pkg) = name.strip_prefix("go:")
                            && let Some(typedef) = get_go_stdlib_typedef(go_pkg)
                        {
                            checker
                                .parse_and_register_go_module(&mut store, name, typedef, &locator);
                        }
                        Some(FileImport {
                            name: name.clone(),
                            name_span: *name_span,
                            alias: alias.clone(),
                            span: *span,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            checker.put_imported_modules_in_scope(&store, &imports);

            checker.register_types_and_values(&mut store, &self.ast, &Visibility::Local);
            checker.check_const_cycles(&store, &[self.ast.as_slice()]);

            // Store AST in module so compute_module_ufcs can scan impl blocks (condition 3)
            let test_file_id = store.new_file_id();
            store.store_file(
                TEST_MODULE_ID,
                File::new(
                    TEST_MODULE_ID,
                    "test.lis",
                    "",
                    self.ast.clone(),
                    test_file_id,
                ),
            );
            {
                let module = store
                    .get_module(TEST_MODULE_ID)
                    .expect("test module must exist");
                let ufcs_entries =
                    semantics::call_classification::compute_module_ufcs(module, TEST_MODULE_ID);
                checker.ufcs_methods.extend(ufcs_entries);
            }

            let mut typed_ast = vec![];

            for expression in self.ast {
                let type_var = checker.new_type_var();
                let typed_expression = checker.infer_expression(&mut store, expression, &type_var);
                typed_ast.push(typed_expression);

                if checker.failed() {
                    break;
                }
            }

            {
                let folder = semantics::checker::freeze::FreezeFolder::new(&checker.env);
                folder.freeze_facts(&mut checker.facts);
            }
            typed_ast =
                semantics::checker::freeze::FreezeFolder::new(&checker.env).freeze_items(typed_ast);

            if !checker.failed() {
                // Overwrite the stored file with the typed AST so validators::run
                // sees post-inference items when iterating store.modules.
                store.store_file(
                    TEST_MODULE_ID,
                    File::new(
                        TEST_MODULE_ID,
                        "test.lis",
                        "",
                        typed_ast.clone(),
                        test_file_id,
                    ),
                );
                let analysis =
                    semantics::context::AnalysisContext::new(&store, &checker.ufcs_methods);
                let mut harness_unused = UnusedInfo::default();
                semantics::validators::run(
                    &analysis,
                    &mut checker.facts,
                    checker.sink,
                    &mut harness_unused,
                    false,
                );
            }

            if self.wrapped {
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
                                return Some(unwrap_test_wrapper(expr));
                            }
                            None
                        })
                        .collect();
                } else {
                    typed_ast = typed_ast.into_iter().map(unwrap_test_wrapper).collect();
                }
            }

            let definitions: HashMap<Symbol, Definition> = store
                .modules
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
                if b.mutated {
                    mutations.mark_binding_mutated(binding_id);
                }
            }

            let ufcs_methods = std::mem::take(&mut checker.ufcs_methods);
            let go_package_names = store.go_package_names.clone();

            (
                typed_ast,
                definitions,
                unused,
                mutations,
                ufcs_methods,
                go_package_names,
            )
        };

        InferenceResult {
            ast: typed_ast,
            errors: sink.take(),
            definitions,
            module_id: TEST_MODULE_ID.to_string(),
            unused,
            mutations,
            ufcs_methods,
            go_package_names,
        }
    }
}

pub struct InferenceResult {
    pub ast: Vec<Expression>,
    pub errors: Vec<LisetteDiagnostic>,
    pub definitions: HashMap<Symbol, Definition>,
    pub module_id: String,
    pub unused: UnusedInfo,
    pub mutations: MutationInfo,
    pub ufcs_methods: HashSet<(String, String)>,
    pub go_package_names: HashMap<String, String>,
}

fn unwrap_test_wrapper(expression: Expression) -> Expression {
    let Expression::Function { name, .. } = &expression else {
        return expression;
    };

    if name != TEST_WRAPPER_NAME {
        return expression;
    }

    let Expression::Function { body, .. } = expression else {
        unreachable!()
    };

    let Expression::Block { items, .. } = *body else {
        panic!(
            "Expected Block as body of {} wrapper function",
            TEST_WRAPPER_NAME
        );
    };

    items
        .into_iter()
        .next_back()
        .expect("Expected at least one expression in wrapped test function body")
}
