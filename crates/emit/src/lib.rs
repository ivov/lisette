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
pub(crate) use types::prelude::PreludeType;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use names::go_name::go_test_function_name;
pub use output::OutputFile;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use abi::callable::{CallableReturnAbi, OptionReturnAbi};
use abi::catalog::GoAbiCatalog;
use abi::go_payload_layout;
use abi::layout::SlotOrigin;
use analyze::facts::is_nullable_option;
use diagnostics::LisetteDiagnostic;
use names::go_name::GeneratedPackage;
use names::packages::{PackageRequirements, PackageUse};
use plan::PackagePlan;
use plan::bodies::{LoopId, LoweredBlock, Statement, define};
use plan::go_expression::GoExpressionNode;
use plan::local::GoIdentifier;
use plan::values::GoExpression;
use state::adapter_registry::AdapterRegistry;
use state::file_namespace::FileNamespace;
use state::package_state::PackageState;
use state::scope::ScopeState;
use syntax::ast::{BindingId, Expression, Span};
use syntax::program;
use syntax::program::{Definition, DefinitionBody, EmitInput, File, interface_requirements};
use syntax::types::{Symbol, Type, peel_alias};
use types::go_type::GoType;

#[derive(Clone, Debug)]
pub struct EmitOptions {
    pub sourcemap: bool,
    pub emit_tests: bool,
}

/// A library root's Go package name, from its package path (`example.com/lib/v2` -> `lib`).
pub fn root_package_name(go_module: &str) -> String {
    go_name::sanitize_package_name(program::go_import_default_name(go_module)).into_owned()
}

#[derive(Default)]
pub(crate) struct GlobalEmitData {
    go_abi_catalog: GoAbiCatalog,
    exported_method_names: HashSet<String>,
    /// Method selectors whose interface bounds require a single generic result.
    tagged_method_names: HashSet<String>,
    /// Built once: a layout depends only on the definitions.
    enum_layouts: HashMap<Symbol, EnumLayout>,
}

impl GlobalEmitData {
    fn compute(definitions: &HashMap<Symbol, Definition>) -> Self {
        let mut globals = GlobalEmitData {
            go_abi_catalog: GoAbiCatalog::from_definitions(definitions),
            exported_method_names: HashSet::default(),
            tagged_method_names: HashSet::default(),
            enum_layouts: HashMap::default(),
        };

        for (key, definition) in definitions.iter() {
            globals.register_exported_methods(key, definition);
            globals.register_bound_returns(definition, definitions);
            globals.register_enum_layout(key, definition, definitions);
        }

        globals
    }

    fn register_enum_layout(
        &mut self,
        key: &Symbol,
        definition: &Definition,
        definitions: &HashMap<Symbol, Definition>,
    ) {
        let DefinitionBody::Enum {
            generics,
            variants,
            default_variant,
            ..
        } = &definition.body
        else {
            return;
        };
        if key.starts_with(go_name::PRELUDE_PREFIX) {
            return;
        }
        let layout = EnumLayout::new(key, generics, variants, *default_variant, |id| {
            definitions.get(id)
        });
        self.enum_layouts.insert(key.clone(), layout);
    }

    fn register_bound_returns(
        &mut self,
        definition: &Definition,
        definitions: &HashMap<Symbol, Definition>,
    ) {
        let function_bounds = definition
            .ty
            .as_function_type()
            .into_iter()
            .flat_map(|function| &function.bounds)
            .map(|bound| &bound.ty);
        let type_bounds = definition
            .body
            .generics()
            .into_iter()
            .flatten()
            .flat_map(|generic| generic.resolved_bounds().into_iter().flatten());
        let method_bounds = definition
            .methods()
            .into_iter()
            .flat_map(|methods| methods.values())
            .filter_map(|method| method.ty.as_function_type())
            .flat_map(|function| &function.bounds)
            .map(|bound| &bound.ty);
        for bound in function_bounds.chain(type_bounds).chain(method_bounds) {
            for requirement in interface_requirements(bound, |id| definitions.get(id)) {
                if go_name::is_go_import(&requirement.declaring_interface) {
                    continue;
                }
                let Some(return_ty) = requirement.method.ty.unwrap_forall().get_function_ret()
                else {
                    continue;
                };
                if matches!(
                    peel_alias(return_ty, |id| definitions.get(id)),
                    Type::Parameter(_)
                ) {
                    // A generic result must keep one value for every instantiation.
                    // Implementations must expose that same ABI to satisfy a Go bound.
                    self.tagged_method_names
                        .insert(go_name::snake_to_camel(&requirement.name));
                }
            }
        }
    }

    fn register_exported_methods(&mut self, key: &Symbol, definition: &Definition) {
        let is_user_definition =
            !go_name::is_go_import(key) && !key.starts_with(go_name::PRELUDE_PREFIX);
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
                    && is_user_definition
                    && key.split('.').nth(2).is_some() =>
            {
                let method_name = go_name::unqualified_name(key);
                self.exported_method_names.insert(method_name.to_string());
            }
            _ => {}
        }

        if is_user_definition && let Some(methods) = definition.methods() {
            for method in methods
                .values()
                .filter(|method| method.visibility.is_public())
            {
                self.exported_method_names
                    .insert(method.source_name.to_string());
            }
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
    let payload = || go_payload_layout(return_ty);
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

pub struct Planner<'a> {
    facts: EmitFacts<'a>,
    package: PackageState,
    scope: ScopeState,
    adapter_registry: AdapterRegistry,
    namespace: FileNamespace,
}

impl Planner<'_> {
    fn require_fmt(&mut self) {
        self.require_generated_package(GeneratedPackage::Fmt);
    }

    fn require_errors(&mut self) {
        self.require_generated_package(GeneratedPackage::Errors);
    }

    fn require_json(&mut self) {
        self.require_generated_package(GeneratedPackage::Json);
    }

    fn require_cmp(&mut self) {
        self.require_generated_package(GeneratedPackage::Cmp);
    }

    fn require_testkit(&mut self) {
        self.require_generated_package(GeneratedPackage::TestKit);
    }

    fn require_testing(&mut self) {
        self.require_generated_package(GeneratedPackage::Testing);
    }

    fn require_generated_package(&mut self, package: GeneratedPackage) {
        self.namespace.require(PackageUse::generated(package));
    }

    fn use_go_type(&mut self, ty: &Type) -> String {
        self.use_rendered_go_type(self.go_type(ty))
    }

    fn use_rendered_go_type(&mut self, go_type: GoType) -> String {
        self.namespace.absorb(go_type.requirements());
        go_type.code
    }

    fn require_packages(&mut self, requirements: &PackageRequirements) {
        self.namespace.absorb(requirements);
    }

    fn collect_imports(&mut self, statements: &[Statement]) {
        let namespace = &mut self.namespace;
        for statement in statements {
            statement
                .kind
                .visit_expressions(&mut |node| require_qualified(namespace, node));
        }
    }

    fn render_expression(&mut self, expression: &GoExpression) -> String {
        let namespace = &mut self.namespace;
        expression
            .node()
            .visit(&mut |node| require_qualified(namespace, node));
        expression.rendered()
    }
}

fn require_qualified(namespace: &mut FileNamespace, node: &GoExpressionNode) {
    if let GoExpressionNode::Qualified { package, .. } = node {
        namespace.require(package.clone());
    }
}

impl<'a> Planner<'a> {
    fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        self.return_context_for_slot(return_ty, SlotOrigin::Lisette)
    }

    fn return_context_for_slot(&self, return_ty: Type, origin: SlotOrigin) -> ReturnContext {
        let peeled = self.facts.peel_alias(&return_ty);
        match self.classify_slot_emission(&peeled, origin) {
            Some(shape) => ReturnContext::Lowered {
                return_ty: peeled,
                shape,
            },
            // A non-container keeps its declared name, so a Go-named
            // function type still identifies its slot.
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
        let line_indexes = options.sourcemap.then(|| {
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
        });

        let shared = SharedEmitContext {
            go_module,
            entry_package_name,
            emit_tests: options.emit_tests,
            line_indexes,
            globals: GlobalEmitData::compute(&analysis.definitions),
        };

        let mut files_by_package: HashMap<&str, Vec<&File>> = HashMap::default();
        for file in analysis.files.values().filter(|file| !file.is_d_lis()) {
            if !analysis.cached_packages.contains(&file.package_id) {
                files_by_package
                    .entry(file.package_id.as_str())
                    .or_default()
                    .push(file);
            }
        }
        for files in files_by_package.values_mut() {
            files.sort_unstable_by_key(|file| file.id);
        }
        let mut work: Vec<_> = files_by_package.into_iter().collect();
        work.sort_unstable_by_key(|(package_id, _)| *package_id);

        const PARALLEL_THRESHOLD: usize = 4;

        let emit_one = |(package_id, files): &(&str, Vec<&File>)| {
            emit_package(analysis, &shared, package_id, files)
        };

        let package_outputs: Vec<Result<Vec<OutputFile>, Vec<LisetteDiagnostic>>> =
            if work.len() < PARALLEL_THRESHOLD {
                work.iter().map(emit_one).collect()
            } else {
                use rayon::prelude::*;
                work.par_iter().map(emit_one).collect()
            };

        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        for package_output in package_outputs {
            match package_output {
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

    fn new(facts: EmitFacts<'a>) -> Self {
        Self {
            facts,
            package: PackageState::default(),
            scope: ScopeState::new(),
            adapter_registry: AdapterRegistry::default(),
            namespace: FileNamespace::default(),
        }
    }

    /// Lower a loop body whose `break value` writes `result`.
    fn with_loop<R>(&mut self, result: GoExpression, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.push_loop(result.clone());
        let result = self.with_assign_target(&result, f);
        self.scope.pop_loop();
        result
    }

    fn current_loop_result(&self) -> Option<&GoExpression> {
        self.scope.current_loop_result()
    }

    fn current_loop_id(&self) -> Option<LoopId> {
        self.scope.current_loop_id()
    }

    fn return_ctx(&self) -> ReturnContext {
        self.scope.current_return_ctx()
    }

    /// `true` if this is a new declaration in the current block (use `:=`),
    /// `false` if the name is already declared (use `=`).
    fn try_declare(&mut self, go_name: &str) -> bool {
        self.scope.try_declare_go_name(go_name)
    }

    fn is_declared(&self, go_name: &str) -> bool {
        self.scope.is_go_name_declared(go_name)
    }

    fn shadows_declaration(&self, go_name: &str) -> bool {
        self.is_declared(go_name) || self.package.is_package_block_name(go_name)
    }

    /// Whether an enclosing branch already tested this exact condition.
    fn is_condition_established(&self, condition: &GoExpression) -> bool {
        self.scope.is_condition_established(condition)
    }

    /// Unconditionally marks `go_name` as declared in the current block.
    fn declare(&mut self, go_name: &str) {
        self.scope.declare_go_name(go_name);
    }

    fn claim_declared_binding(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: impl Into<String>,
    ) -> GoIdentifier {
        let go_name = self.claim_declared_go_name(lisette_name, preferred);
        self.scope.bind_source(lisette_name, ids, go_name)
    }

    /// Declare `preferred` for `lisette_name`, or a fresh name if it would shadow. Binds nothing.
    fn claim_declared_go_name(
        &mut self,
        lisette_name: &str,
        preferred: impl Into<String>,
    ) -> String {
        let go_name = escape_reserved(&preferred.into()).into_owned();
        let go_name = if self.shadows_declaration(&go_name)
            || self
                .scope
                .has_other_binding_for_go_name(&go_name, lisette_name)
        {
            let fresh = self
                .scope
                .fresh_binding_go_name(lisette_name, &self.package);
            escape_reserved(&fresh).into_owned()
        } else {
            go_name
        };
        self.declare(&go_name);
        go_name
    }

    /// Bind `value` to a name that can be read more than once.
    fn stable_source(
        &mut self,
        statements: &mut Vec<Statement>,
        hint: &str,
        value: GoExpression,
    ) -> GoExpression {
        if value.as_identifier().is_some() {
            return value;
        }
        GoExpression::name(self.hoist_tmp_value_statement(statements, hint, value))
    }

    /// Allocate a fresh Go temp, register it as declared, and push `tmp := value`.
    fn hoist_tmp_value_statement(
        &mut self,
        setup: &mut Vec<Statement>,
        hint: &str,
        value: GoExpression,
    ) -> String {
        let tmp = self.fresh_var(Some(hint));
        self.declare(&tmp);
        setup.push(define(tmp.clone(), value));
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

    fn with_declaration_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.scope.begin_declaration();
        let result = f(self);
        self.scope.end_declaration(outer);
        result
    }

    fn with_binding_frame<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.push_binding_frame();
        let result = f(self);
        self.scope.pop_binding_frame();
        result
    }

    fn with_isolated_function<R>(
        &mut self,
        return_ctx: ReturnContext,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.enter_isolated_function(return_ctx);
        let result = f(self);
        self.scope.exit_isolated_function();
        result
    }

    fn with_assign_target<R>(
        &mut self,
        target: &GoExpression,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let newly_active = self.scope.activate_assign_target(target);
        let result = f(self);
        if newly_active {
            self.scope.deactivate_assign_target(target);
        }
        result
    }

    fn fresh_var(&mut self, hint: Option<&str>) -> String {
        self.scope.fresh_go_name(hint, &self.package)
    }

    fn maybe_line_directive(&self, span: &Span) -> Option<String> {
        if span.is_dummy() {
            return None;
        }
        let source = self.facts.line_index(span.file_id)?;
        let line = source.line_for_offset(span.byte_offset);
        let col = source.col_for_offset(span.byte_offset);
        Some(format!("//line {}:{}:{}\n", source.path, line, col))
    }

    fn emit_files_with_facts(
        facts: EmitFacts<'a>,
        files: &[&File],
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        Self::new(facts).emit_files(files)
    }

    fn emit_files(mut self, files: &[&File]) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let plan = self.build_package_plan(files);
        self.render_package_plan(files, plan)
    }

    fn render_package_plan(
        mut self,
        files: &[&File],
        plan: PackagePlan,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let PackagePlan {
            package_name,
            collision_diagnostics,
            imports: import_plans,
        } = plan;
        let mut output_files = Vec::new();
        let mut all_diagnostics = collision_diagnostics;

        for (file, imports) in files.iter().zip(import_plans) {
            self.namespace = FileNamespace::new(imports);
            let source = self.render_file_source(file);
            let (imports, mut diagnostics) = self.namespace.finish();
            all_diagnostics.append(&mut diagnostics);
            output_files.push(OutputFile {
                name: file.go_filename(),
                imports,
                source,
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

    fn render_file_source(&mut self, file: &File) -> String {
        let mut source = OutputCollector::new();

        for expression in &file.items {
            let Expression::Enum { name, variants, .. } = expression else {
                continue;
            };
            if PreludeType::from_name(name).is_some() {
                continue;
            }
            let enum_id = self.facts.qualified_current(name);
            for variant in variants {
                source.collect_with_blank(self.create_make_function_code(&enum_id, &variant.name));
            }
        }

        for expression in &file.items {
            let code = self.with_declaration_scope(|this| this.emit_top_item(expression));
            if !code.is_empty() {
                source.collect_with_blank(code);
            }
        }

        self.drain_file_emission_into(&mut source);
        source.render()
    }
}

/// Emit state built once in [`Planner::emit`] and shared by every package worker.
pub(crate) struct SharedEmitContext<'a> {
    pub(crate) go_module: &'a str,
    pub(crate) entry_package_name: &'a str,
    pub(crate) emit_tests: bool,
    pub(crate) line_indexes: Option<HashMap<u32, LineIndex>>,
    pub(crate) globals: GlobalEmitData,
}

fn emit_package<'a>(
    analysis: &'a EmitInput,
    shared: &'a SharedEmitContext<'a>,
    package_id: &str,
    files: &[&'a File],
) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
    let facts = EmitFacts::new(analysis, shared, package_id.to_string().into());
    Planner::emit_files_with_facts(facts, files).map(|mut package_output| {
        if package_id != analysis.entry_package_id.as_str() {
            for file in &mut package_output {
                file.name = format!("{}/{}", package_id, file.name);
            }
        }
        package_output
    })
}
