mod ast_lints;
mod fact_lints;
mod ref_lints;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::facts::Facts;
use crate::store::Store;
use diagnostics::DiagnosticSink;
use diagnostics::LisetteDiagnostic;
use syntax::ast::Expression;
use syntax::program::File;
use syntax::program::Module;
use syntax::program::UnusedInfo;

use ast_lints::AstLintGroup;
use fact_lints::FactLintGroup;
use ref_lints::RefLintGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lint {
    UnusedVariable,
    UnusedParameter,
    UnusedMut,
    UnusedImport,
    UnusedType,
    UnusedFunction,
    UnusedConstant,
    UnusedStructField,
    UnusedEnumVariant,
    UnusedLiteral,
    UnusedResult,
    UnusedOption,
    UnusedValue,
    DeadCodeAfterReturn,
    DeadCodeAfterBreak,
    DeadCodeAfterContinue,
    DeadCodeAfterDivergingIf,
    DeadCodeAfterDivergingMatch,
    DeadCodeAfterInfiniteLoop,
    DeadCodeAfterDivergingCall,
    DoubleBoolNegation,
    DoubleIntNegation,
    SelfComparison,
    SelfAssignment,
    MatchLiteralCollection,
    EmptyMatchArm,
    InternalTypeLeak,
    UnnecessaryReference,
    UnusedTypeParameter,
    RestOnlySlicePattern,
    NonPascalCaseType,
    NonPascalCaseTypeParameter,
    NonPascalCaseEnumVariant,
    NonSnakeCaseFunction,
    NonSnakeCaseVariable,
    NonSnakeCaseParameter,
    NonSnakeCaseStructField,
    NonScreamingSnakeCaseConstant,
    RedundantIfLet,
    RedundantLetElse,
    SingleArmMatch,
    RedundantIfLetElse,
    UnreachableIfLetElse,
    TryBlockNoSuccessPath,
    ExcessParensOnCondition,
}

#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    disabled: HashSet<Lint>,
}

impl LintConfig {
    pub fn is_enabled(&self, lint: Lint) -> bool {
        !self.disabled.contains(&lint)
    }
}

pub trait LintRule {
    fn check(&self, ctx: &LintContext) -> Vec<LisetteDiagnostic>;
}

pub struct LintContext<'a> {
    pub ast: &'a [Expression],
    pub facts: &'a Facts,
    pub module: Option<&'a Module>,
    pub config: &'a LintConfig,
    pub is_d_lis: bool,
    /// Files for this module (used by ref_lints for cross-file analysis)
    pub files: &'a HashMap<u32, File>,
}

fn all_lint_rules() -> Vec<Box<dyn LintRule>> {
    vec![
        Box::new(FactLintGroup),
        Box::new(AstLintGroup),
        Box::new(RefLintGroup),
    ]
}

pub fn lint_all_modules(store: &Store, facts: &Facts, sink: &DiagnosticSink) -> UnusedInfo {
    let config = LintConfig::default();
    let mut unused = UnusedInfo::default();

    // Fact lints run once globally (facts are shared across all modules)
    {
        let empty_ast = [];
        let empty_files = HashMap::default();
        let ctx = LintContext {
            ast: &empty_ast,
            facts,
            module: None,
            config: &config,
            is_d_lis: false,
            files: &empty_files,
        };
        let mut diagnostics = FactLintGroup.check(&ctx);
        diagnostics.sort_by_key(|d| d.primary_offset());
        for diagnostic in diagnostics {
            sink.push(diagnostic);
        }
    }

    for module in store.modules.values() {
        if module.is_internal() {
            continue;
        }
        lint_module(module, facts, &config, sink, &mut unused);
    }

    for b in facts.bindings.values() {
        if !b.used {
            unused.mark_binding_unused(b.span);
        }
    }

    unused
}

fn lint_module(
    module: &Module,
    facts: &Facts,
    config: &LintConfig,
    sink: &DiagnosticSink,
    unused: &mut UnusedInfo,
) {
    // Module-level lints (reference graph analysis)
    let ref_result = ref_lints::run_ref_lints(module, &module.files, config, facts);
    if !ref_result.unused_import_aliases.is_empty() {
        unused.imports_by_module.insert(
            module.id.clone().into(),
            ref_result
                .unused_import_aliases
                .into_iter()
                .map(|s| s.into())
                .collect(),
        );
    }
    for span in ref_result.unused_definition_spans {
        unused.mark_definition_unused(span);
    }
    for diagnostic in ref_result.diagnostics {
        sink.push(diagnostic);
    }

    // File-level lints
    for file in module.files.values() {
        let ctx = LintContext {
            ast: &file.items,
            facts,
            module: Some(module),
            config,
            is_d_lis: file.is_d_lis(),
            files: &module.files,
        };

        let mut diagnostics = AstLintGroup.check(&ctx);
        diagnostics.sort_by_key(|d| d.primary_offset());
        for diagnostic in diagnostics {
            sink.push(diagnostic);
        }
    }
}

pub fn lint_file(ctx: &LintContext, sink: &DiagnosticSink) {
    let mut diagnostics: Vec<_> = all_lint_rules()
        .iter()
        .flat_map(|lint| lint.check(ctx))
        .collect();

    diagnostics.sort_by_key(|d| d.primary_offset());

    for diagnostic in diagnostics {
        sink.push(diagnostic);
    }
}
