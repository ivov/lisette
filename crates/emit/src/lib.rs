pub(crate) mod bindings;
pub(crate) mod calls;
mod collectors;
mod constraint_collector;
pub(crate) mod control_flow;
pub(crate) mod definitions;
pub(crate) mod expressions;
mod facts;
pub mod imports;
mod inline_uses;
mod module_state;
pub(crate) mod names;
mod output;
pub(crate) mod patterns;
mod placement;
pub(crate) mod queries;
mod requirements;
mod scope;
pub(crate) mod statements;
pub(crate) mod types;
mod utils;

pub(crate) use bindings::Bindings;
pub(crate) use calls::go_interop::GoCallStrategy;
pub(crate) use definitions::enum_layout::EnumLayout;
pub(crate) use facts::EmitFacts;
pub(crate) use names::go_name;
pub(crate) use names::go_name::escape_reserved;
pub(crate) use output::OutputCollector;
pub(crate) use requirements::EmitEffects;
pub(crate) use types::emitter::{LineIndex, LoopContext, ReturnContext};
pub(crate) use types::prelude::PreludeType;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use output::OutputFile;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use ecow::EcoString;
use imports::ImportBuilder;
use syntax::ast::Span;
use syntax::program::{
    Definition, DefinitionBody, EmitInput, File, ModuleId, MutationInfo, UnusedInfo,
};
use syntax::types::{Symbol, Type};

#[derive(Clone, Debug, Default)]
pub struct EmitOptions {
    pub debug: bool,
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
                && let Type::Function { return_type, .. } = match definition.ty() {
                    Type::Forall { body, .. } => body.as_ref(),
                    other => other,
                }
                && let Some(strategy) =
                    classify_go_return_type(definitions, return_type, definition.go_hints())
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

/// Make-function name registry entries for a user-declared enum, paralleling
/// [`PreludeType::make_function_entries`].
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
        if !facts::is_nullable_option(definitions, return_ty) {
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

pub struct Emitter<'a> {
    pub(crate) facts: EmitFacts<'a>,
    pub(crate) module: module_state::ModuleState,
    pub(crate) function_state: module_state::FunctionEmissionState,
    pub(crate) scope: scope::ScopeState,

    synthesized_adapter_types: HashMap<(EcoString, EcoString), String>,
    pending_adapter_types: Vec<String>,

    // Per-file accumulated state (reset between files)
    pub(crate) requirements: requirements::EmitRequirements,

    /// Fallback for deep callers that cannot reach a `&ReturnContext` threaded
    /// from a tail boundary. Set only at function/lambda/try/recover scope
    /// entry via `with_scope_return_context_fallback`.
    scope_return_context_fallback: ReturnContext,
}

impl<'a> Emitter<'a> {
    pub(crate) fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        match self.classify_direct_emission(&return_ty) {
            Some(shape) => ReturnContext::Lowered { return_ty, shape },
            None => ReturnContext::Tagged(return_ty),
        }
    }

    pub(crate) fn scope_return_context_fallback(&self) -> &ReturnContext {
        &self.scope_return_context_fallback
    }

    pub(crate) fn with_scope_return_context_fallback<F, R>(&mut self, ctx: ReturnContext, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = std::mem::replace(&mut self.scope_return_context_fallback, ctx);
        let result = f(self);
        self.scope_return_context_fallback = saved;
        result
    }
}

impl<'a> Emitter<'a> {
    pub fn emit(analysis: &'a EmitInput, go_module: &str, options: EmitOptions) -> Vec<OutputFile> {
        let line_indexes: Arc<HashMap<u32, LineIndex>> = Arc::new(if options.debug {
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

        let globals = Arc::new(GlobalEmitData::compute(&analysis.definitions));

        let mut work: Vec<(&ModuleId, &syntax::program::ModuleInfo)> = analysis
            .modules
            .iter()
            .filter(|(id, _)| !analysis.cached_modules.contains(*id))
            .collect();
        work.sort_unstable_by(|a, b| a.0.cmp(b.0));

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |&(module_id, module_info): &(&ModuleId, &syntax::program::ModuleInfo)| {
            emit_module(
                analysis,
                go_module,
                &options,
                &line_indexes,
                &globals,
                module_id,
                module_info,
            )
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
        let (debug, line_indexes) = match source {
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
        let facts = EmitFacts::new(facts::EmitFactsConfig {
            definitions: config.definitions,
            unused: config.unused,
            mutations: config.mutations,
            ufcs_methods: config.ufcs_methods,
            go_package_names: config.go_package_names,
            go_module_ids: config.go_module_ids,
            entry_module: config.module_id.to_string(),
            go_module: config.go_module.to_string(),
            options: EmitOptions { debug },
            line_indexes,
            globals,
            current_module: config.module_id.to_string(),
        });
        Self::new(facts)
    }

    fn new(facts: EmitFacts<'a>) -> Self {
        Self {
            facts,
            module: module_state::ModuleState::default(),
            function_state: module_state::FunctionEmissionState::default(),
            scope: scope::ScopeState::new(),
            synthesized_adapter_types: HashMap::default(),
            pending_adapter_types: Vec::new(),
            requirements: requirements::EmitRequirements::new(),
            scope_return_context_fallback: ReturnContext::None,
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
        utils::write_line!(output, "{} := {}", tmp, value);
        tmp
    }

    pub(crate) fn capture_emission<F, R>(&mut self, output: &mut String, f: F) -> (String, R)
    where
        F: FnOnce(&mut Self, &mut String) -> R,
    {
        let before = output.len();
        let value = f(self, output);
        let captured = output[before..].to_string();
        output.truncate(before);
        (captured, value)
    }

    pub(crate) fn capture_scoped<F>(&mut self, output: &mut String, f: F) -> Option<String>
    where
        F: FnOnce(&mut Self, &mut String),
    {
        self.enter_scope();
        let (captured, ()) = self.capture_emission(output, |this, buf| f(this, buf));
        self.exit_scope();
        (!captured.is_empty()).then_some(captured)
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
        if !self.facts.debug_enabled() || span.is_dummy() {
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
        self.facts.set_current_module(module_id);
        self.collect_module_aliases(files);
        self.collect_local_exported_method_names(files);
        self.collect_generic_constraints(files);
        self.collect_enum_layouts();
        self.collect_escape_remap(files);
        let mut make_functions_by_file = self.collect_local_make_function_code();

        let mut output_files = Vec::new();

        let package_name = if self.facts.is_entry_module(module_id) {
            "main".to_string()
        } else {
            let raw = module_id.rsplit('/').next().unwrap_or(module_id);
            go_name::sanitize_package_name(raw).into_owned()
        };

        for file in files {
            let mut source = OutputCollector::new();

            if let Some(functions) = make_functions_by_file.remove(&file.id) {
                for function in functions {
                    source.collect_with_blank(function);
                }
            }

            self.pending_adapter_types.clear();

            for expression in &file.items {
                self.scope.reset_for_top_level();
                let code = self.emit_top_item(expression);
                if !code.is_empty() {
                    source.collect_with_blank(code);
                }
            }

            for adapter_decl in std::mem::take(&mut self.pending_adapter_types) {
                source.collect_with_blank(adapter_decl);
            }

            let mut import_builder = ImportBuilder::new(
                self.facts.go_module(),
                self.facts.unused_imports_for_current_module(),
                self.facts.go_package_names(),
            );
            import_builder.collect_from_file(file);

            self.requirements.drain_into(&mut import_builder);

            import_builder.filter_unused_imports();

            let rendered_source = source.render();

            let (imports, diagnostics) = import_builder.build();
            output_files.push(OutputFile {
                name: file.go_filename(),
                imports,
                source: rendered_source,
                package_name: package_name.clone(),
                diagnostics,
            });
        }

        output_files
    }
}

fn emit_module<'a>(
    analysis: &'a EmitInput,
    go_module: &str,
    options: &EmitOptions,
    line_indexes: &Arc<HashMap<u32, LineIndex>>,
    globals: &Arc<GlobalEmitData>,
    module_id: &str,
    module_info: &syntax::program::ModuleInfo,
) -> Vec<OutputFile> {
    let facts = EmitFacts::new(facts::EmitFactsConfig {
        definitions: &analysis.definitions,
        unused: &analysis.unused,
        mutations: &analysis.mutations,
        ufcs_methods: &analysis.ufcs_methods,
        go_package_names: &analysis.go_package_names,
        go_module_ids: &analysis.go_module_ids,
        entry_module: analysis.entry_module_id.to_string(),
        go_module: go_module.to_string(),
        options: options.clone(),
        line_indexes: line_indexes.clone(),
        globals: globals.clone(),
        current_module: module_id.to_string(),
    });
    let mut emitter: Emitter<'a> = Emitter::new(facts);

    let files: Vec<_> = module_info
        .file_ids
        .iter()
        .filter_map(|fid| analysis.files.get(fid))
        .collect();

    let mut module_output = emitter.emit_files(&files, module_id);

    if module_id != analysis.entry_module_id.as_str() {
        for file in &mut module_output {
            file.name = format!("{}/{}", module_info.path, file.name);
        }
    }

    module_output
}
