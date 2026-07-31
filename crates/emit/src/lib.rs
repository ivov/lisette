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
pub(crate) use context::lowering::{LineIndex, ReturnContext};
pub(crate) use definitions::enum_layout::EnumLayout;
pub(crate) use names::go_name;
pub(crate) use names::go_name::escape_reserved;
pub(crate) use output::OutputCollector;
pub(crate) use render::Renderer;
pub(crate) use state::bindings::Bindings;
pub(crate) use types::prelude::PreludeType;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use names::go_name::go_test_function_name;
pub use output::OutputFile;
pub use output::imports;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;

use abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use abi::catalog::GoAbiCatalog;
use analyze::facts::{EmitFactsConfig, is_nullable_option};
use diagnostics::LisetteDiagnostic;
use names::go_name::GeneratedPackage;
use names::packages::{PackageRequirements, PackageUse};
use plan::ModulePlan;
use plan::bodies::{LoopId, LoweredBlock, LoweredStatement};
use state::adapter_registry::AdapterRegistry;
use state::bindings::BindingSnapshot;
use state::file_namespace::FileNamespace;
use state::module_state::{FunctionEmissionState, ModuleState};
use state::scope::ScopeState;
use syntax::ast::Span;
use syntax::program::{
    Definition, DefinitionBody, EmitInput, EqualityIndex, File, MutationInfo, TestIndex, UnusedInfo,
};
use syntax::types::{Symbol, Type};
use types::go_type::GoType;

#[derive(Clone, Debug)]
pub struct EmitOptions {
    pub sourcemap: bool,
    pub emit_tests: bool,
}

/// A library root's Go package name, from its module path (`example.com/lib/v2` -> `lib`).
pub fn root_package_name(go_module: &str) -> String {
    go_name::sanitize_package_name(syntax::program::go_import_default_name(go_module)).into_owned()
}

#[derive(Default)]
pub(crate) struct GlobalEmitData {
    go_abi_catalog: GoAbiCatalog,
    exported_method_names: HashSet<String>,
}

impl GlobalEmitData {
    fn compute(definitions: &HashMap<Symbol, Definition>) -> Self {
        let mut globals = GlobalEmitData {
            go_abi_catalog: GoAbiCatalog::from_definitions(definitions),
            exported_method_names: HashSet::default(),
        };

        for (key, definition) in definitions.iter() {
            let is_go = go_name::is_go_import(key);
            globals.register_exported_methods(key, definition, is_go);
        }

        globals
    }

    fn register_exported_methods(&mut self, key: &Symbol, definition: &Definition, is_go: bool) {
        match &definition.body {
            DefinitionBody::Interface {
                definition: iface, ..
            } if definition.visibility.is_public() => {
                for method_name in iface.methods.keys() {
                    self.exported_method_names.insert(method_name.to_string());
                }
            }
            DefinitionBody::Value { .. }
                if definition.visibility.is_public()
                    && !is_go
                    && !key.starts_with(go_name::PRELUDE_PREFIX)
                    && key.chars().filter(|c| *c == '.').count() >= 2 =>
            {
                let method_name = go_name::unqualified_name(key);
                self.exported_method_names.insert(method_name.to_string());
            }
            _ => {}
        }

        if definition.visibility.is_public() && definition.is_display() {
            self.exported_method_names.insert("to_string".to_string());
        }
    }
}

pub(crate) fn classify_go_return_type(
    definitions: &HashMap<Symbol, Definition>,
    return_ty: &Type,
    go_hints: &[String],
) -> Option<CallableReturnAbi> {
    let payload = || {
        if return_ty
            .ok_type()
            .tuple_arity()
            .is_some_and(|arity| arity >= 2)
        {
            PayloadLayout::Flattened
        } else {
            PayloadLayout::Packed
        }
    };
    if return_ty.is_partial() {
        return Some(CallableReturnAbi::Partial { payload: payload() });
    }
    if return_ty.is_result() {
        return Some(if return_ty.ok_type().is_unit() {
            CallableReturnAbi::BareError
        } else {
            CallableReturnAbi::Result { payload: payload() }
        });
    }
    if return_ty.is_option() {
        if let Some(value) = sentinel_hint(go_hints) {
            return Some(CallableReturnAbi::Option(OptionReturnAbi::Sentinel(value)));
        }
        if !is_nullable_option(definitions, return_ty) {
            return Some(CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload: payload(),
            }));
        }
        if go_hints.iter().any(|s| s == "comma_ok") {
            return Some(CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                payload: payload(),
            }));
        }
        return Some(CallableReturnAbi::Option(OptionReturnAbi::Nullable));
    }
    if let Some(arity) = return_ty.tuple_arity()
        && arity >= 2
    {
        return Some(CallableReturnAbi::Tuple { arity });
    }
    None
}

fn sentinel_hint(hints: &[String]) -> Option<i64> {
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
    pub equality_index: &'a EqualityIndex,
    pub test_index: &'a TestIndex,
    pub go_package_names: &'a HashMap<String, String>,
    pub go_module_ids: &'a HashSet<String>,
}

pub struct Planner<'a> {
    facts: EmitFacts<'a>,
    module: ModuleState,
    function_state: FunctionEmissionState,
    scope: ScopeState,
    adapter_registry: AdapterRegistry,
    namespace: RefCell<Option<FileNamespace>>,
}

impl Planner<'_> {
    fn file_namespace(&self) -> Ref<'_, FileNamespace> {
        Ref::map(self.namespace.borrow(), |namespace| {
            namespace
                .as_ref()
                .expect("package resolution requires an active file namespace")
        })
    }

    fn file_namespace_mut(&self) -> RefMut<'_, FileNamespace> {
        RefMut::map(self.namespace.borrow_mut(), |namespace| {
            namespace
                .as_mut()
                .expect("package resolution requires an active file namespace")
        })
    }

    fn require_stdlib(&self) {
        self.require_generated_package(GeneratedPackage::Prelude);
    }

    fn require_fmt(&self) {
        self.require_generated_package(GeneratedPackage::Fmt);
    }

    fn require_errors(&self) {
        self.require_generated_package(GeneratedPackage::Errors);
    }

    fn require_slices(&self) {
        self.require_generated_package(GeneratedPackage::Slices);
    }

    fn require_strings(&self) {
        self.require_generated_package(GeneratedPackage::Strings);
    }

    fn require_maps(&self) {
        self.require_generated_package(GeneratedPackage::Maps);
    }

    fn require_json(&self) {
        self.require_generated_package(GeneratedPackage::Json);
    }

    fn require_cmp(&self) {
        self.require_generated_package(GeneratedPackage::Cmp);
    }

    fn require_testkit(&self) {
        self.require_generated_package(GeneratedPackage::TestKit);
    }

    fn require_testing(&self) {
        self.require_generated_package(GeneratedPackage::Testing);
    }

    fn require_generated_package(&self, package: GeneratedPackage) {
        self.file_namespace_mut()
            .require(PackageUse::generated(package));
    }

    fn note_go_type(&self, go_type: &GoType) {
        self.file_namespace_mut().absorb(go_type.requirements());
    }

    fn require_packages(&self, requirements: &PackageRequirements) {
        self.file_namespace_mut().absorb(requirements);
    }
}

impl<'a> Planner<'a> {
    fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        match self.classify_direct_emission(&return_ty) {
            Some(shape) => ReturnContext::Lowered { return_ty, shape },
            None => ReturnContext::Tagged(return_ty),
        }
    }

    /// Append this file's newly-synthesized adapter declarations to `source`.
    fn drain_file_emission_into(&mut self, source: &mut OutputCollector) {
        for adapter_declaration in self.adapter_registry.drain_declarations() {
            source.collect_with_blank(adapter_declaration);
        }
    }
}

impl<'a> Planner<'a> {
    pub fn emit(
        analysis: &'a EmitInput,
        go_module: &str,
        entry_package_name: &'a str,
        options: EmitOptions,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
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
        };

        let mut files_by_module: HashMap<&str, Vec<&File>> = HashMap::default();
        for file in analysis.files.values().filter(|file| !file.is_d_lis()) {
            if !analysis.cached_modules.contains(&file.module_id) {
                files_by_module
                    .entry(file.module_id.as_str())
                    .or_default()
                    .push(file);
            }
        }
        for files in files_by_module.values_mut() {
            files.sort_unstable_by_key(|file| file.id);
        }
        let mut work: Vec<_> = files_by_module.into_iter().collect();
        work.sort_unstable_by_key(|(module_id, _)| *module_id);

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |(module_id, files): &(&str, Vec<&File>)| {
            emit_module(
                analysis,
                go_module,
                entry_package_name,
                &shared,
                module_id,
                files,
            )
        };

        let module_outputs: Vec<Result<Vec<OutputFile>, Vec<LisetteDiagnostic>>> =
            if work.len() < PARALLEL_THRESHOLD {
                work.iter().map(emit_one).collect()
            } else {
                use rayon::prelude::*;
                work.par_iter().map(emit_one).collect()
            };

        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        for module_output in module_outputs {
            match module_output {
                Ok(mut files) => output.append(&mut files),
                Err(mut errors) => diagnostics.append(&mut errors),
            }
        }

        if diagnostics.is_empty() {
            output.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(output)
        } else {
            diagnostics.sort_by(LisetteDiagnostic::sort_key);
            Err(diagnostics)
        }
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
            equality_index: config.equality_index,
            test_index: config.test_index,
            go_package_names: config.go_package_names,
            go_module_ids: config.go_module_ids,
            entry_module: config.module_id.to_string(),
            entry_package_name: "main",
            go_module: config.go_module.to_string(),
            options: EmitOptions {
                sourcemap,
                emit_tests: false,
            },
            line_indexes,
            globals,
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
            namespace: RefCell::new(None),
        }
    }

    fn with_loop<R>(&mut self, result_var: impl Into<String>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.push_loop(result_var.into());
        let result = f(self);
        self.scope.pop_loop();
        result
    }

    fn current_loop_result_var(&self) -> Option<&str> {
        self.scope.current_loop_result_var()
    }

    fn current_loop_id(&self) -> Option<LoopId> {
        self.scope.current_loop_id()
    }

    /// Push the enclosing function/lambda/try/recover return context. This
    /// scope stack is the single source of truth for return-context lowering;
    /// all readers consult it via [`Planner::return_ctx`].
    fn with_return_context<R>(&mut self, ctx: ReturnContext, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.push_return_ctx(ctx);
        let result = f(self);
        self.scope.pop_return_ctx();
        result
    }

    fn with_test_handle<R>(&mut self, name: Option<String>, f: impl FnOnce(&mut Self) -> R) -> R {
        if let Some(name) = name {
            self.scope.push_test_handle(name);
            let result = f(self);
            self.scope.pop_test_handle();
            result
        } else {
            f(self)
        }
    }

    fn current_test_handle(&self) -> Option<String> {
        self.scope.current_test_handle().map(str::to_string)
    }

    /// The enclosing function/lambda/try/recover return context, maintained on
    /// the scope stack and shared cheaply via `Rc`. Defaults to
    /// `ReturnContext::None` outside any function body (e.g. module-level
    /// collection). This is the single source of truth for return-context
    /// lowering.
    fn return_ctx(&self) -> ReturnContext {
        self.scope
            .current_return_ctx()
            .unwrap_or(ReturnContext::None)
    }

    /// `true` if this is a new declaration in the current block (use `:=`),
    /// `false` if the name is already declared (use `=`).
    fn try_declare(&mut self, go_name: &str) -> bool {
        self.scope.try_declare_go_name(go_name)
    }

    fn is_declared(&self, go_name: &str) -> bool {
        self.scope.is_go_name_declared(go_name)
    }

    /// Unconditionally marks `go_name` as declared in the current block.
    fn declare(&mut self, go_name: &str) {
        self.scope.declare_go_name(go_name);
    }

    /// Allocate a fresh Go temp, register it as declared, and emit
    /// `tmp := value` into `output`.
    fn hoist_tmp_value(&mut self, output: &mut String, hint: &str, value: &str) -> String {
        let tmp = self.fresh_var(Some(hint));
        self.declare(&tmp);
        write_line!(output, "{} := {}", tmp, value);
        tmp
    }

    /// Structured counterpart of `hoist_tmp_value`: push a `TempBind` leaf.
    fn hoist_tmp_value_statement(
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
    /// when its IR emits no output.
    fn capture_scoped_block<F>(&mut self, f: F) -> Option<LoweredBlock>
    where
        F: FnOnce(&mut Self) -> LoweredBlock,
    {
        let block = self.with_scope(f);
        (!block.renders_empty()).then_some(block)
    }

    fn enter_scope(&mut self) {
        self.scope.enter_block();
    }

    fn exit_scope(&mut self) {
        self.scope.exit_block();
    }

    fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.enter_scope();
        let result = f(self);
        self.exit_scope();
        result
    }

    fn with_binding_frame<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.push_binding_frame();
        let result = f(self);
        self.scope.pop_binding_frame();
        result
    }

    fn with_assign_target<R>(&mut self, target: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        let newly_active = self.scope.activate_assign_target(target);
        let result = f(self);
        if newly_active {
            self.scope.deactivate_assign_target(target);
        }
        result
    }

    fn with_binding_snapshot<R>(
        &mut self,
        snapshot: BindingSnapshot,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let current = self.scope.replace_binding_snapshot(snapshot);
        let result = f(self);
        let _ = self.scope.replace_binding_snapshot(current);
        result
    }

    fn capture_go_uses<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> (R, HashSet<String>) {
        self.scope.enter_use_region();
        let result = f(self);
        let uses = self.scope.exit_use_region();
        (result, uses)
    }

    fn fresh_var(&mut self, hint: Option<&str>) -> String {
        self.scope.fresh_go_name(hint)
    }

    fn with_function_body_context<R>(
        &mut self,
        return_ctx: ReturnContext,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.push_const_frame();
        self.scope.push_return_ctx(return_ctx);
        let result = f(self);
        self.scope.pop_return_ctx();
        self.scope.pop_const_frame();
        result
    }

    fn record_go_const(&mut self, go_identifier: String) {
        self.scope.record_go_const_binding(go_identifier);
    }

    fn is_go_const_binding(&self, go_identifier: &str) -> bool {
        self.scope.is_go_const_binding(go_identifier)
    }

    fn maybe_line_directive(&self, span: &Span) -> String {
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

    pub fn emit_files(
        &mut self,
        files: &[&File],
        module_id: &str,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let plan = self.build_module_plan(files, module_id);
        self.render_module_plan(files, plan)
    }

    fn render_module_plan(
        &mut self,
        files: &[&File],
        plan: ModulePlan,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let ModulePlan {
            package_name,
            files: file_plans,
            mut collision_diagnostics,
        } = plan;
        let mut output_files = Vec::new();
        let mut all_diagnostics = Vec::new();

        for (i, (file, file_plan)) in files.iter().zip(file_plans).enumerate() {
            *self.namespace.borrow_mut() = Some(file_plan.namespace);
            let mut source = OutputCollector::new();

            for function in &file_plan.make_functions {
                source.collect_with_blank(
                    self.create_make_function_code(&function.enum_id, &function.variant_name),
                );
            }

            for expression in &file.items {
                self.scope.reset_for_top_level();
                let code = self.emit_top_item(expression);
                if !code.is_empty() {
                    source.collect_with_blank(code);
                }
            }

            self.drain_file_emission_into(&mut source);
            let rendered_source = source.render();

            let namespace = self
                .namespace
                .borrow_mut()
                .take()
                .expect("file namespace was installed before rendering");
            let (imports, mut diagnostics) =
                namespace.finish(self.facts.go_package_names(), self.facts.go_module_ids());
            if i == 0 {
                diagnostics.append(&mut collision_diagnostics);
            }
            all_diagnostics.append(&mut diagnostics);
            output_files.push(OutputFile {
                name: file.go_filename(),
                imports,
                source: rendered_source,
                package_name: package_name.clone(),
                file_comment: file.file_comment.clone(),
            });
        }

        if all_diagnostics.is_empty() {
            Ok(output_files)
        } else {
            all_diagnostics.sort_by(LisetteDiagnostic::sort_key);
            Err(all_diagnostics)
        }
    }
}

/// Emit state built once in [`Planner::emit`] and shared by every module worker.
struct SharedEmitContext {
    options: EmitOptions,
    line_indexes: Arc<HashMap<u32, LineIndex>>,
    globals: Arc<GlobalEmitData>,
}

fn emit_module<'a>(
    analysis: &'a EmitInput,
    go_module: &str,
    entry_package_name: &'a str,
    shared_emit_ctx: &SharedEmitContext,
    module_id: &str,
    files: &[&'a File],
) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
    let facts = EmitFacts::new(EmitFactsConfig {
        definitions: &analysis.definitions,
        unused: &analysis.unused,
        mutations: &analysis.mutations,
        equality_index: &analysis.equality_index,
        test_index: &analysis.test_index,
        go_package_names: &analysis.go_package_names,
        go_module_ids: &analysis.go_module_ids,
        entry_module: analysis.entry_module_id.to_string(),
        entry_package_name,
        go_module: go_module.to_string(),
        options: shared_emit_ctx.options.clone(),
        line_indexes: shared_emit_ctx.line_indexes.clone(),
        globals: shared_emit_ctx.globals.clone(),
        current_module: module_id.to_string(),
    });
    let mut planner: Planner<'a> = Planner::new(facts);

    planner
        .emit_files(files, module_id)
        .map(|mut module_output| {
            if module_id != analysis.entry_module_id.as_str() {
                for file in &mut module_output {
                    file.name = format!("{}/{}", module_id, file.name);
                }
            }
            module_output
        })
}
