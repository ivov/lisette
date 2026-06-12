mod abi;
mod analyze;
pub(crate) mod calls;
pub(crate) mod context;
pub(crate) mod control_flow;
pub(crate) mod definitions;
pub(crate) mod expressions;
pub(crate) mod names;
mod output;
pub(crate) mod patterns;
mod plan;
mod render;
mod state;
pub(crate) mod statements;
pub(crate) mod types;
mod utils;

pub(crate) use analyze::facts::EmitFacts;
pub(crate) use calls::go_interop::GoCallStrategy;
pub(crate) use context::lowering::{LineIndex, LoopContext, ReturnContext};
pub(crate) use definitions::enum_layout::EnumLayout;
pub(crate) use names::go_name;
pub(crate) use names::go_name::escape_reserved;
pub(crate) use output::OutputCollector;
pub(crate) use render::Renderer;
pub(crate) use state::bindings::Bindings;
pub(crate) use state::effects::EmitEffects;
pub(crate) use types::prelude::PreludeType;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use output::OutputFile;
pub use output::imports;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

use analyze::facts::{EmitFactsConfig, is_nullable_option};
use names::constraints::GenericConstraintTable;
use output::imports::ImportBuilder;
use plan::ModulePlan;
use plan::bodies::{LoweredBlock, LoweredStatement};
use state::adapter_registry::AdapterRegistry;
use state::module_state::{FunctionEmissionState, ModuleState};
use state::scope::ScopeState;
use syntax::ast::Span;
use syntax::program::{
    Definition, DefinitionBody, EmitInput, File, ModuleId, MutationInfo, UnusedInfo,
};
use syntax::types::{Symbol, Type};

#[derive(Clone, Debug, Default)]
pub struct EmitOptions {
    pub sourcemap: bool,
}

#[derive(Default)]
pub(crate) struct GlobalEmitData {
    pub(crate) go_call_strategies: HashMap<String, GoCallStrategy>,
    pub(crate) exported_method_names: HashSet<String>,
    pub(crate) make_function_names: HashMap<String, String>,
}

impl GlobalEmitData {
    fn compute(definitions: &HashMap<Symbol, Definition>) -> Self {
        let mut globals = GlobalEmitData::default();

        for prelude_type in PreludeType::enum_types() {
            for (constructor, make_fn) in prelude_type.make_function_entries() {
                globals.make_function_names.insert(constructor, make_fn);
            }
        }

        for (key, definition) in definitions.iter() {
            let is_go = go_name::is_go_import(key);

            if is_go
                && let Type::Function(f) = match definition.ty() {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                }
                && let Some(strategy) =
                    classify_go_return_type(definitions, &f.return_type, definition.go_hints())
            {
                globals.go_call_strategies.insert(key.to_string(), strategy);
            }

            match &definition.body {
                DefinitionBody::Interface {
                    definition: iface, ..
                } if definition.visibility.is_public() => {
                    for method_name in iface.methods.keys() {
                        globals
                            .exported_method_names
                            .insert(method_name.to_string());
                    }
                }
                DefinitionBody::Value { .. }
                    if definition.visibility.is_public()
                        && !is_go
                        && !key.starts_with(go_name::PRELUDE_PREFIX)
                        && key.chars().filter(|c| *c == '.').count() >= 2 =>
                {
                    let method_name = go_name::unqualified_name(key);
                    globals
                        .exported_method_names
                        .insert(method_name.to_string());
                }
                _ => {}
            }

            if definition.visibility.is_public() && definition.is_display() {
                globals
                    .exported_method_names
                    .insert("to_string".to_string());
            }

            if let Definition {
                name: Some(name),
                body: DefinitionBody::Enum { variants, .. },
                ..
            } = definition
                && PreludeType::from_name(name).is_none()
            {
                for (constructor, make_fn) in user_enum_make_function_entries(name, variants) {
                    globals.make_function_names.insert(constructor, make_fn);
                }
            }
        }

        globals
    }
}

/// Make-function name registry entries for a user-declared enum.
pub(crate) fn user_enum_make_function_entries<'a>(
    name: &'a str,
    variants: &'a [syntax::ast::EnumVariant],
) -> impl Iterator<Item = (String, String)> + 'a {
    let go_type_name = go_name::escape_keyword(name).into_owned();
    variants.iter().map(move |variant| {
        let constructor = format!("{}.{}", name, variant.name);
        let make_fn = format!("Make{}{}", go_type_name, variant.name);
        (constructor, make_fn)
    })
}

pub(crate) fn classify_go_return_type(
    definitions: &HashMap<Symbol, Definition>,
    return_ty: &Type,
    go_hints: &[String],
) -> Option<GoCallStrategy> {
    if return_ty.is_partial() {
        return Some(GoCallStrategy::Partial);
    }
    if return_ty.is_result() {
        return Some(GoCallStrategy::Result);
    }
    if return_ty.is_option() {
        if let Some(value) = sentinel_hint(go_hints) {
            return Some(GoCallStrategy::Sentinel { value });
        }
        if !is_nullable_option(definitions, return_ty) {
            return Some(GoCallStrategy::CommaOk);
        }
        if go_hints.iter().any(|s| s == "comma_ok") {
            return Some(GoCallStrategy::CommaOk);
        }
        return Some(GoCallStrategy::NullableReturn);
    }
    if let Some(arity) = return_ty.tuple_arity()
        && arity >= 2
    {
        return Some(GoCallStrategy::Tuple { arity });
    }
    None
}

pub(crate) fn sentinel_hint(hints: &[String]) -> Option<i64> {
    hints
        .iter()
        .any(|h| h == "sentinel_minus_one")
        .then_some(-1)
}

pub struct TestEmitConfig<'a> {
    pub definitions: &'a HashMap<Symbol, Definition>,
    pub module_id: &'a str,
    pub go_module: &'a str,
    pub unused: &'a UnusedInfo,
    pub mutations: &'a MutationInfo,
    pub ufcs_methods: &'a HashSet<(String, String)>,
    pub go_package_names: &'a HashMap<String, String>,
    pub go_module_ids: &'a HashSet<String>,
}

pub struct Planner<'a> {
    pub(crate) facts: EmitFacts<'a>,
    pub(crate) module: ModuleState,
    pub(crate) function_state: FunctionEmissionState,
    pub(crate) scope: ScopeState,
    pub(crate) adapter_registry: AdapterRegistry,
}

impl<'a> Planner<'a> {
    pub(crate) fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        match self.classify_direct_emission(&return_ty) {
            Some(shape) => ReturnContext::Lowered { return_ty, shape },
            None => ReturnContext::Tagged(return_ty),
        }
    }

    /// Append this file's newly-synthesized adapter declarations to `source`.
    pub(crate) fn drain_file_emission_into(&mut self, source: &mut OutputCollector) {
        for adapter_declaration in self.adapter_registry.flush_new_declarations() {
            source.collect_with_blank(adapter_declaration);
        }
    }
}

impl<'a> Planner<'a> {
    pub fn emit(analysis: &'a EmitInput, go_module: &str, options: EmitOptions) -> Vec<OutputFile> {
        let line_indexes: Arc<HashMap<u32, LineIndex>> = Arc::new(if options.sourcemap {
            analysis
                .files
                .iter()
                .map(|(file_id, file)| {
                    (
                        *file_id,
                        LineIndex::from_source(file.display_path.clone(), &file.source),
                    )
                })
                .collect()
        } else {
            HashMap::default()
        });

        let shared = SharedEmitContext {
            options,
            line_indexes,
            globals: Arc::new(GlobalEmitData::compute(&analysis.definitions)),
            generic_base: Arc::new(OnceLock::new()),
        };

        let mut work: Vec<(&ModuleId, &syntax::program::ModuleInfo)> = analysis
            .modules
            .iter()
            .filter(|(id, _)| !analysis.cached_modules.contains(*id))
            .collect();
        work.sort_unstable_by(|a, b| a.0.cmp(b.0));

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |&(module_id, module_info): &(&ModuleId, &syntax::program::ModuleInfo)| {
            emit_module(analysis, go_module, &shared, module_id, module_info)
        };

        let mut output: Vec<OutputFile> = if work.len() < PARALLEL_THRESHOLD {
            work.iter().flat_map(emit_one).collect()
        } else {
            use rayon::prelude::*;
            work.par_iter().flat_map_iter(emit_one).collect()
        };

        output.sort_by(|a, b| a.name.cmp(&b.name));
        output
    }

    pub fn new_for_tests(config: &TestEmitConfig<'a>, source: Option<&str>) -> Self {
        let (sourcemap, line_indexes) = match source {
            Some(src) => (
                true,
                Arc::new(HashMap::from_iter([(
                    0u32,
                    LineIndex::from_source("src/test.lis".to_string(), src),
                )])),
            ),
            None => (false, Arc::new(HashMap::default())),
        };
        let globals = Arc::new(GlobalEmitData::compute(config.definitions));
        let facts = EmitFacts::new(EmitFactsConfig {
            definitions: config.definitions,
            unused: config.unused,
            mutations: config.mutations,
            ufcs_methods: config.ufcs_methods,
            go_package_names: config.go_package_names,
            go_module_ids: config.go_module_ids,
            entry_module: config.module_id.to_string(),
            go_module: config.go_module.to_string(),
            options: EmitOptions { sourcemap },
            line_indexes,
            globals,
            generic_base: Arc::new(OnceLock::new()),
            current_module: config.module_id.to_string(),
        });
        Self::new(facts)
    }

    fn new(facts: EmitFacts<'a>) -> Self {
        Self {
            facts,
            module: ModuleState::default(),
            function_state: FunctionEmissionState::default(),
            scope: ScopeState::new(),
            adapter_registry: AdapterRegistry::default(),
        }
    }

    pub(crate) fn push_loop(&mut self, result_var: impl Into<String>) {
        self.scope.push_loop(LoopContext {
            result_var: result_var.into(),
            label: None,
        });
    }

    pub(crate) fn pop_loop(&mut self) {
        self.scope.pop_loop();
    }

    pub(crate) fn current_loop_result_var(&self) -> Option<&str> {
        self.scope.current_loop_result_var()
    }

    pub(crate) fn current_loop_label(&self) -> Option<&str> {
        self.scope.current_loop_label()
    }

    /// Push the enclosing function/lambda/try/recover return context. This
    /// scope stack is the single source of truth for return-context lowering;
    /// all readers consult it via [`Planner::return_ctx`].
    pub(crate) fn push_return_ctx(&mut self, ctx: ReturnContext) {
        self.scope.push_return_ctx(Rc::new(ctx));
    }

    pub(crate) fn pop_return_ctx(&mut self) {
        self.scope.pop_return_ctx();
    }

    /// The enclosing function/lambda/try/recover return context, maintained on
    /// the scope stack and shared cheaply via `Rc`. Defaults to
    /// `ReturnContext::None` outside any function body (e.g. module-level
    /// collection). This is the single source of truth for return-context
    /// lowering.
    pub(crate) fn return_ctx(&self) -> Rc<ReturnContext> {
        self.scope
            .current_return_ctx()
            .unwrap_or_else(|| Rc::new(ReturnContext::None))
    }

    /// `true` if this is a new declaration in the current block (use `:=`),
    /// `false` if the name is already declared (use `=`).
    pub(crate) fn try_declare(&mut self, go_name: &str) -> bool {
        self.scope.try_declare_go_name(go_name)
    }

    pub(crate) fn is_declared(&self, go_name: &str) -> bool {
        self.scope.is_go_name_declared(go_name)
    }

    /// Unconditionally marks `go_name` as declared in the current block.
    pub(crate) fn declare(&mut self, go_name: &str) {
        self.scope.declare_go_name(go_name);
    }

    /// Allocate a fresh Go temp, register it as declared, and emit
    /// `tmp := value` into `output`.
    pub(crate) fn hoist_tmp_value(
        &mut self,
        output: &mut String,
        hint: &str,
        value: &str,
    ) -> String {
        let tmp = self.fresh_var(Some(hint));
        self.declare(&tmp);
        write_line!(output, "{} := {}", tmp, value);
        tmp
    }

    /// Structured counterpart of `hoist_tmp_value`: push a `TempBind` leaf.
    pub(crate) fn hoist_tmp_value_statement(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        hint: &str,
        value: &str,
    ) -> String {
        let tmp = self.fresh_var(Some(hint));
        self.declare(&tmp);
        setup.push(LoweredStatement::TempBind {
            name: tmp.clone(),
            value: value.to_string(),
        });
        tmp
    }

    /// Run `f` inside a fresh scope to build a `LoweredBlock`, returning `None`
    /// when it renders empty.
    pub(crate) fn capture_scoped_block<F>(&mut self, f: F) -> Option<LoweredBlock>
    where
        F: FnOnce(&mut Self) -> LoweredBlock,
    {
        self.enter_scope();
        let block = f(self);
        self.exit_scope();
        let mut buffer = String::new();
        Renderer.render_lowered_block(&mut buffer, &block);
        (!buffer.is_empty()).then_some(block)
    }

    pub(crate) fn enter_scope(&mut self) {
        self.scope.enter_block();
    }

    pub(crate) fn exit_scope(&mut self) {
        self.scope.exit_block();
    }

    pub(crate) fn fresh_var(&mut self, hint: Option<&str>) -> String {
        self.scope.fresh_go_name(hint)
    }

    pub(crate) fn set_current_loop_label_if_needed(&mut self, needs_label: bool) {
        if needs_label {
            let label = self.fresh_var(Some("loop"));
            self.scope.set_current_loop_label(label);
        }
    }

    pub(crate) fn push_const_frame(&mut self) {
        self.scope.push_const_frame();
    }

    pub(crate) fn pop_const_frame(&mut self) {
        self.scope.pop_const_frame();
    }

    pub(crate) fn record_go_const(&mut self, go_identifier: String) {
        self.scope.record_go_const_binding(go_identifier);
    }

    pub(crate) fn is_go_const_binding(&self, go_identifier: &str) -> bool {
        self.scope.is_go_const_binding(go_identifier)
    }

    pub(crate) fn module_alias_for_type(&self, ty: &Type) -> Option<String> {
        if let Type::Nominal { id, .. } = ty {
            let module = self.facts.module_for_qualified_name(id)?;
            self.module.module_alias(module).map(str::to_string)
        } else {
            None
        }
    }

    pub(crate) fn maybe_line_directive(&self, span: &Span) -> String {
        if !self.facts.sourcemap_enabled() || span.is_dummy() {
            return String::new();
        }

        let Some(source) = self.facts.line_index(span.file_id) else {
            return String::new();
        };

        let line = source.line_for_offset(span.byte_offset);
        let col = source.col_for_offset(span.byte_offset);

        format!("//line {}:{}:{}\n", source.path, line, col)
    }

    pub fn emit_files(&mut self, files: &[&File], module_id: &str) -> Vec<OutputFile> {
        let plan = self.build_module_plan(files, module_id);
        self.render_module_plan(files, &plan)
    }

    fn render_module_plan(&mut self, files: &[&File], plan: &ModulePlan) -> Vec<OutputFile> {
        let mut output_files = Vec::new();

        for (i, (file, file_plan)) in files.iter().zip(&plan.files).enumerate() {
            debug_assert_eq!(file.id, file_plan.file_id, "plan/file order mismatch");

            let mut source = OutputCollector::new();

            for function in &file_plan.make_functions {
                source.collect_with_blank(function.clone());
            }

            let mut fx = EmitEffects::default();
            for expression in &file.items {
                self.scope.reset_for_top_level();
                let code = self.emit_top_item(expression, &mut fx);
                if !code.is_empty() {
                    source.collect_with_blank(code);
                }
            }

            let mut import_builder =
                ImportBuilder::from_plan(&file_plan.imports, self.facts.go_package_names());

            self.drain_file_emission_into(&mut source);
            fx.drain_into(&mut import_builder);
            if i == 0 {
                plan.collection_effects.drain_into(&mut import_builder);
            }

            import_builder.filter_unused_imports();

            let rendered_source = source.render();

            let (imports, mut diagnostics) = import_builder.build();
            if i == 0 {
                diagnostics.extend(plan.collision_diagnostics.iter().cloned());
            }
            output_files.push(OutputFile {
                name: file_plan.output_name.clone(),
                imports,
                source: rendered_source,
                package_name: plan.package_name.clone(),
                diagnostics,
            });
        }

        output_files
    }
}

/// Emit state built once in [`Planner::emit`] and shared (by `&`) across every
/// module worker: options plus the three computed-once `Arc`s.
struct SharedEmitContext {
    options: EmitOptions,
    line_indexes: Arc<HashMap<u32, LineIndex>>,
    globals: Arc<GlobalEmitData>,
    generic_base: Arc<OnceLock<GenericConstraintTable>>,
}

fn emit_module<'a>(
    analysis: &'a EmitInput,
    go_module: &str,
    shared_emit_ctx: &SharedEmitContext,
    module_id: &str,
    module_info: &syntax::program::ModuleInfo,
) -> Vec<OutputFile> {
    let facts = EmitFacts::new(EmitFactsConfig {
        definitions: &analysis.definitions,
        unused: &analysis.unused,
        mutations: &analysis.mutations,
        ufcs_methods: &analysis.ufcs_methods,
        go_package_names: &analysis.go_package_names,
        go_module_ids: &analysis.go_module_ids,
        entry_module: analysis.entry_module_id.to_string(),
        go_module: go_module.to_string(),
        options: shared_emit_ctx.options.clone(),
        line_indexes: shared_emit_ctx.line_indexes.clone(),
        globals: shared_emit_ctx.globals.clone(),
        generic_base: shared_emit_ctx.generic_base.clone(),
        current_module: module_id.to_string(),
    });
    let mut planner: Planner<'a> = Planner::new(facts);

    let files: Vec<_> = module_info
        .file_ids
        .iter()
        .filter_map(|fid| analysis.files.get(fid))
        .collect();

    let mut module_output = planner.emit_files(&files, module_id);

    if module_id != analysis.entry_module_id.as_str() {
        for file in &mut module_output {
            file.name = format!("{}/{}", module_info.path, file.name);
        }
    }

    module_output
}
