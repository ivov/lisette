use crate::plan::declarations::{DeclarationKind, GoDeclaration};
use crate::types::go_type::GoType;
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
pub(crate) use names::go_name::escape_local;
pub(crate) use output::OutputCollector;
pub(crate) use render::Renderer;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use names::go_name::go_test_function_name;
pub use output::OutputFile;

use std::path::PathBuf;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use abi::catalog::GoAbiCatalog;
use abi::go_payload_layout;
use context::lowering::LoopContext;
use diagnostics::LisetteDiagnostic;
use names::go_name::GeneratedPackage;
use names::packages::{PackageRequirements, PackageUse};
use names::source_name::SourceName;
use plan::PackagePlan;
use plan::bodies::{LoopId, LoweredBlock, LoweredStatement, Statement, define};
use plan::go_expression::{GoExpressionNode, Position, TreeVisitor};
use plan::local::GoIdentifier;
use plan::values::GoExpression;
use state::adapter_registry::AdapterRegistry;
use state::file_namespace::FileNamespace;
use state::package_state::PackageState;
use state::scope::ScopeState;
use syntax::ast::{BindingId, Expression, Span};
use syntax::program;
use syntax::program::SlotOrigin;
use syntax::program::{Definition, DefinitionBody, EmitInput, File, GoMethodPolicy};
use syntax::types::{Symbol, Type};
use types::go_type::is_prelude_type;

#[derive(Clone, Debug)]
pub struct EmitOptions {
    pub sourcemap: bool,
    /// Base for relative paths in `//line` directives.
    pub sourcemap_base: Option<PathBuf>,
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
        let policy = GoMethodPolicy::compute(definitions.iter(), |id| definitions.get(id));
        let mut globals = GlobalEmitData {
            go_abi_catalog: GoAbiCatalog::from_definitions(definitions),
            exported_method_names: policy.exported_method_names.into_iter().collect(),
            tagged_method_names: policy.tagged_method_names.into_iter().collect(),
            enum_layouts: HashMap::default(),
        };

        for (key, definition) in definitions.iter() {
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
}

pub(crate) fn classify_go_return_type(
    definitions: &HashMap<Symbol, Definition>,
    return_ty: &Type,
    go_hints: &[program::GoHint],
) -> Option<LoweredReturnAbi> {
    use syntax::program::{GoReturnContract as Contract, go_return_contract};
    let payload = || go_payload_layout(return_ty);
    Some(
        match go_return_contract(return_ty, go_hints, |id| definitions.get(id)) {
            Contract::Value => return None,
            Contract::Error => LoweredReturnAbi::BareError,
            Contract::Result => LoweredReturnAbi::Result { payload: payload() },
            Contract::Partial => LoweredReturnAbi::Partial { payload: payload() },
            Contract::CommaOk => {
                LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { payload: payload() })
            }
            Contract::Nullable => LoweredReturnAbi::Option(OptionReturnAbi::Nullable),
            Contract::Sentinel(value) => LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value)),
            Contract::Tuple(arity) => LoweredReturnAbi::Tuple { arity },
        },
    )
}

pub struct Planner<'a> {
    facts: EmitFacts<'a>,
    package: &'a mut PackageState,
    scope: ScopeState,
    adapter_registry: &'a mut AdapterRegistry,
    namespace: &'a mut FileNamespace,
}

pub(crate) struct PlanningSnapshot {
    package: PackageState,
    scope: ScopeState,
    adapter_registry: AdapterRegistry,
    namespace: FileNamespace,
}

impl Planner<'_> {
    pub(crate) fn planning_snapshot(&self) -> PlanningSnapshot {
        let Self {
            facts: _,
            package,
            scope,
            adapter_registry,
            namespace,
        } = self;
        PlanningSnapshot {
            package: (**package).clone(),
            scope: scope.clone(),
            adapter_registry: (**adapter_registry).clone(),
            namespace: (**namespace).clone(),
        }
    }

    pub(crate) fn restore_planning(&mut self, saved: PlanningSnapshot) {
        let PlanningSnapshot {
            package,
            scope,
            adapter_registry,
            namespace,
        } = saved;
        *self.package = package;
        self.scope = scope;
        *self.adapter_registry = adapter_registry;
        *self.namespace = namespace;
    }

    fn require_testkit(&mut self) {
        self.require_generated_package(GeneratedPackage::TestKit);
    }

    fn require_generated_package(&mut self, package: GeneratedPackage) {
        self.namespace.require(PackageUse::generated(package));
    }

    fn require_packages(&mut self, requirements: &PackageRequirements) {
        self.namespace.absorb(requirements);
    }

    pub(crate) fn collect_declaration_imports(&mut self, declaration: &GoDeclaration) {
        let mut types: Vec<&GoType> = Vec::new();
        let generics = match &declaration.kind {
            DeclarationKind::Type {
                parameters,
                underlying,
                ..
            }
            | DeclarationKind::Alias {
                parameters,
                underlying,
                ..
            } => {
                types.push(underlying);
                parameters.as_slice()
            }
            DeclarationKind::Struct {
                parameters, fields, ..
            } => {
                types.extend(fields.iter().map(|field| &field.ty));
                parameters.as_slice()
            }
            DeclarationKind::Interface {
                parameters,
                parents,
                methods,
                ..
            } => {
                types.extend(parents);
                for method in methods {
                    types.extend(&method.parameters);
                    types.push(&method.result);
                }
                parameters.as_slice()
            }
            DeclarationKind::Constants(constants) => {
                for constant in constants {
                    types.extend(constant.ty.as_ref());
                    if let Some(value) = &constant.value {
                        value.node().walk(&mut ImportCollector(self.namespace));
                    }
                }
                &[]
            }
            DeclarationKind::Binding(binding) => {
                self.collect_imports(&[binding.clone().into()]);
                &[]
            }
            DeclarationKind::Function { signature, body } => {
                types.extend(
                    signature
                        .receiver
                        .iter()
                        .map(|parameter| &parameter.go_type),
                );
                types.extend(
                    signature
                        .parameters
                        .iter()
                        .map(|parameter| &parameter.go_type),
                );
                types.push(&signature.result);
                self.collect_imports(&body.statements);
                signature.generics.as_slice()
            }
        };
        types.extend(generics.iter().flat_map(|parameter| &parameter.bounds));
        for ty in types {
            self.require_packages(ty.requirements());
        }
    }

    fn collect_imports(&mut self, statements: &[Statement]) {
        let mut collector = ImportCollector(self.namespace);
        for statement in statements {
            statement.kind.walk(&mut collector);
        }
    }
}

struct ImportCollector<'a>(&'a mut FileNamespace);

impl TreeVisitor for ImportCollector<'_> {
    fn expression(&mut self, node: &GoExpressionNode) {
        if let GoExpressionNode::Qualified { package, .. } = node {
            self.0.require(package.clone());
        }
        node.own_types(&mut |go_type| self.0.absorb(go_type.requirements()));
    }

    fn statement(&mut self, statement: &LoweredStatement) {
        statement.own_types(&mut |go_type| self.0.absorb(go_type.requirements()));
    }
}

impl<'a> Planner<'a> {
    fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        self.return_context_for_slot(return_ty, SlotOrigin::Lisette)
    }

    fn return_context_for_method(&self, return_ty: Type) -> ReturnContext {
        match self.method_return_abi(&return_ty) {
            CallableReturnAbi::Lowered(shape) => ReturnContext::Lowered { return_ty, shape },
            _ => self.return_context_for_type(return_ty),
        }
    }

    fn return_context_for_slot(&self, return_ty: Type, origin: SlotOrigin) -> ReturnContext {
        if types::go_type::returns_go_void(&return_ty) {
            return ReturnContext::Void(return_ty);
        }
        let peeled = self.facts.peel_alias(&return_ty);
        match self.classify_slot_emission(&peeled, origin) {
            Some(shape) => ReturnContext::Lowered { return_ty, shape },
            None => ReturnContext::Tagged(return_ty),
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
                .filter(|(_, file)| !file.is_d_lis())
                .map(|(file_id, file)| {
                    let path = match &options.sourcemap_base {
                        Some(base) => base.join(&file.display_path).display().to_string(),
                        None => file.display_path.clone(),
                    };
                    (*file_id, LineIndex::from_source(path, &file.source))
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

    fn new(
        facts: EmitFacts<'a>,
        package: &'a mut PackageState,
        adapter_registry: &'a mut AdapterRegistry,
        namespace: &'a mut FileNamespace,
    ) -> Self {
        Self {
            facts,
            package,
            scope: ScopeState::new(),
            adapter_registry,
            namespace,
        }
    }

    /// Lower a loop body whose `break value` writes `result`.
    fn with_loop<R>(
        &mut self,
        result: GoExpression,
        result_ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.push_loop(result, result_ty);
        let result = f(self);
        self.scope.pop_loop();
        result
    }

    fn current_loop(&self) -> Option<&LoopContext> {
        self.scope.current_loop()
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
        preferred: &SourceName,
    ) -> GoIdentifier {
        let go_name = self.claim_declared_go_name(ids, preferred);
        self.scope.bind_source(lisette_name, ids, go_name)
    }

    /// Bind `lisette_name` in the current Go block to `preferred`, or to a fresh name
    /// when `preferred` would hide a name the block still reads or writes.
    fn claim_block_binding(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: &SourceName,
        outer: OuterBindings,
    ) -> GoIdentifier {
        self.claim_block_binding_as(lisette_name, ids, preferred.into(), preferred, outer)
    }

    fn claim_block_binding_as(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: GoIdentifier,
        source: &SourceName,
        outer: OuterBindings,
    ) -> GoIdentifier {
        let go_name = self.scope.new_local_identifier(preferred);
        let hides = self.package.is_package_block_name(&go_name)
            || self.scope.current_block_declares(&go_name)
            || match outer {
                OuterBindings::MayShadow => {
                    self.scope.is_active_assign_target(&go_name)
                        || self
                            .scope
                            .binds_go_name_for_other_source_name(&go_name, lisette_name)
                }
                OuterBindings::StayVisible => self.scope.has_binding_for_go_name(&go_name),
            };
        let go_name = if hides {
            self.fresh_source(source)
        } else {
            go_name
        };
        self.declare(&go_name);
        self.scope.bind_source(lisette_name, ids, go_name)
    }

    /// Declare `preferred`, or a fresh name if it would shadow. Binds nothing.
    fn claim_declared_go_name(
        &mut self,
        ids: &[BindingId],
        preferred: &SourceName,
    ) -> GoIdentifier {
        let go_name = self.scope.new_local_identifier(preferred);
        let go_name = if self.shadows_declaration(&go_name)
            || self.scope.has_other_binding_for_go_name(&go_name, ids)
        {
            let fresh = self
                .scope
                .fresh_binding_go_name(preferred.base(), self.package);
            self.scope.new_local_identifier(fresh)
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
        hint: &'static str,
        value: GoExpression,
    ) -> GoExpression {
        if value.as_identifier().is_some() {
            return value;
        }
        GoExpression::identifier(self.hoist_tmp_value_statement(statements, hint, value))
    }

    /// Allocate a fresh Go temp, register it as declared, and push `tmp := value`.
    fn hoist_tmp_value_statement(
        &mut self,
        setup: &mut Vec<Statement>,
        hint: &'static str,
        value: GoExpression,
    ) -> GoIdentifier {
        let tmp = self.fresh_declared(hint);
        setup.push(define(tmp.clone(), value));
        tmp
    }

    fn hoist_source_value(
        &mut self,
        setup: &mut Vec<Statement>,
        name: &SourceName,
        value: GoExpression,
    ) -> GoIdentifier {
        let local = self.fresh_declared_source(name);
        setup.push(define(local.clone(), value));
        local
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

    fn focused<R>(&mut self, span: Span, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.namespace.rejections().enter(span);
        let result = f(self);
        self.namespace.rejections().leave(previous);
        result
    }

    fn with_declaration_scope<R>(&mut self, f: impl FnOnce(&mut Planner<'_>) -> R) -> R {
        let mut declaration = Planner::new(
            self.facts.clone(),
            self.package,
            self.adapter_registry,
            self.namespace,
        );
        f(&mut declaration)
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
        let name = target
            .as_identifier()
            .filter(|name| name.spelling() != "_")
            .cloned();
        let Some(name) = name else {
            return f(self);
        };
        self.scope.push_assign_target(name);
        let result = f(self);
        self.scope.pop_assign_target();
        result
    }

    /// Allocate and register a local whose declaration belongs to the current block.
    fn fresh_declared(&mut self, hint: &'static str) -> GoIdentifier {
        let name = self.fresh_local(Some(hint));
        self.declare(&name);
        name
    }

    fn fresh_declared_source(&mut self, name: &SourceName) -> GoIdentifier {
        let local = self.fresh_source(name);
        self.declare(&local);
        local
    }

    fn fresh_local(&mut self, hint: Option<&'static str>) -> GoIdentifier {
        self.scope.fresh_go_name(hint, self.package)
    }

    fn fresh_source(&mut self, name: &SourceName) -> GoIdentifier {
        self.scope.fresh_go_name(Some(name.base()), self.package)
    }

    fn fresh_like(&mut self, name: &GoIdentifier) -> GoIdentifier {
        self.scope
            .fresh_go_name(Some(name.spelling()), self.package)
    }

    fn source_position(&self, span: &Span) -> Option<Position> {
        self.position_at(span, span.byte_offset)
    }

    fn end_position(&self, span: &Span) -> Option<Position> {
        self.position_at(span, span.byte_offset + span.byte_length)
    }

    fn position_at(&self, span: &Span, byte_offset: u32) -> Option<Position> {
        if span.is_dummy() {
            return None;
        }
        let source = self.facts.line_index(span.file_id)?;
        Some(Position {
            path: source.path.clone(),
            line: source.line_for_offset(byte_offset),
            column: source.col_for_offset(byte_offset),
        })
    }

    fn emit_files_with_facts(
        facts: EmitFacts<'a>,
        files: &[&File],
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let mut package = PackageState::default();
        let mut adapters = AdapterRegistry::default();
        let plan = Planner::new(
            facts.clone(),
            &mut package,
            &mut adapters,
            &mut FileNamespace::default(),
        )
        .build_package_plan(files);
        let PackagePlan {
            package_name,
            collision_diagnostics,
            files,
        } = plan;
        let mut output_files = Vec::new();
        let mut all_diagnostics = collision_diagnostics;

        for plan in files {
            let file = plan.file;
            let mut namespace = FileNamespace::new(plan.imports);
            let source = Planner::new(facts.clone(), &mut package, &mut adapters, &mut namespace)
                .render_file_source(file);
            let (imports, source, mut diagnostics) = namespace.finish(source);
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
            Err(all_diagnostics)
        }
    }

    fn render_file_source(&mut self, file: &File) -> OutputCollector {
        let mut declarations = Vec::new();
        for expression in &file.items {
            let Expression::Enum { name, variants, .. } = expression else {
                continue;
            };
            if is_prelude_type(name) {
                continue;
            }
            let enum_id = self.facts.qualified_current(name);
            for variant in variants {
                let mut declaration = self.create_make_function_code(&enum_id, &variant.name);
                declaration.position = self.source_position(&variant.name_span);
                declarations.push(declaration);
            }
        }
        for expression in &file.items {
            declarations.extend(self.with_declaration_scope(|this| this.emit_top_item(expression)));
        }
        declarations.extend(self.namespace.take_adapters());
        for declaration in &declarations {
            self.collect_declaration_imports(declaration);
        }
        let mut source = OutputCollector::new();
        for declaration in &declarations {
            source.collect_with_blank(Renderer.render_declaration(declaration));
        }
        source
    }
}

/// Whether a new Go block binding may hide outer bindings of the same spelling.
#[derive(Clone, Copy)]
pub(crate) enum OuterBindings {
    MayShadow,
    /// The block still reads them, as a pattern subject does.
    StayVisible,
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

/// A pipeline step's call span starts at the pipeline's first value, not at the step.
pub(crate) fn written_span(expression: &Expression) -> Span {
    let span = expression.get_span();
    if let Expression::Call { expression, .. } = expression {
        let callee = expression.get_span();
        let end = span.byte_offset + span.byte_length;
        if !callee.is_dummy()
            && callee.file_id == span.file_id
            && callee.byte_offset > span.byte_offset
            && callee.byte_offset < end
        {
            return Span::new(span.file_id, callee.byte_offset, end - callee.byte_offset);
        }
    }
    span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_typed_if_operand_has_no_result_slot() {
        use crate::context::expression::ExpressionContext;
        use syntax::ast::{ExpressionId, Literal};
        use syntax::types::SimpleKind;

        let input = EmitInput::default();
        let shared = SharedEmitContext {
            go_module: "example.com/test",
            entry_package_name: "main",
            emit_tests: false,
            line_indexes: None,
            globals: GlobalEmitData::default(),
        };
        let facts = EmitFacts::new(&input, &shared, "test".into());
        let mut package = PackageState::default();
        let mut adapters = AdapterRegistry::default();
        let mut namespace = FileNamespace::default();
        let mut planner = Planner::new(facts, &mut package, &mut adapters, &mut namespace);
        let span = Span::dummy();
        let diverging = Expression::Loop {
            id: ExpressionId::parsed(0, 11),
            body: Box::new(Expression::Block {
                id: ExpressionId::parsed(0, 10),
                items: vec![],
                ty: Type::unit(),
                span,
            }),
            ty: Type::Never,
            span,
        };
        let expression = Expression::If {
            id: ExpressionId::parsed(0, 9),
            condition: Box::new(Expression::Literal {
                id: ExpressionId::parsed(0, 8),
                literal: Literal::Boolean(true),
                ty: Type::Simple(SimpleKind::Bool),
                span,
            }),
            consequence: Box::new(diverging.clone()),
            alternative: Some(Box::new(diverging)),
            ty: Type::Never,
            span,
        };
        let plan = planner.plan_operand(&expression, ExpressionContext::value());
        assert!(
            plan.expression().is_empty(),
            "a diverging operand has no result"
        );
        let (statements, _) = plan.into_parts();
        assert!(LoweredBlock { statements }.ends_with_diverge());
    }

    #[test]
    fn nested_declarations_own_locals_and_share_file_requirements() {
        let input = EmitInput::default();
        let shared = SharedEmitContext {
            go_module: "example.com/test",
            entry_package_name: "main",
            emit_tests: false,
            line_indexes: None,
            globals: GlobalEmitData::default(),
        };
        let facts = EmitFacts::new(&input, &shared, "test".into());
        let mut package = PackageState::default();
        let mut adapters = AdapterRegistry::default();
        let mut namespace = FileNamespace::default();
        let mut planner = Planner::new(facts, &mut package, &mut adapters, &mut namespace);
        planner.scope.bind_source("outer", &[], "outer");
        let outer = planner.fresh_local(Some("value"));
        planner.with_declaration_scope(|declaration| {
            assert!(declaration.scope.bound_go_identifier("outer").is_none());
            assert_eq!(declaration.fresh_local(Some("value")).spelling(), "value");
            declaration
                .namespace
                .require(PackageUse::generated(GeneratedPackage::Fmt));
            declaration.with_declaration_scope(|nested| {
                assert_eq!(nested.fresh_local(Some("value")).spelling(), "value");
                nested
                    .namespace
                    .require(PackageUse::generated(GeneratedPackage::Errors));
            });
            assert_eq!(declaration.fresh_local(Some("value")).spelling(), "value2");
        });
        assert_eq!(outer.spelling(), "value");
        assert_eq!(planner.fresh_local(Some("value")).spelling(), "value2");
        assert!(planner.scope.bound_go_identifier("outer").is_some());
        let (imports, _, diagnostics) = namespace.finish(OutputCollector::new());
        assert!(diagnostics.is_empty());
        assert_eq!(
            imports
                .iter()
                .map(|import| import.path.as_str())
                .collect::<Vec<_>>(),
            ["errors", "fmt"]
        );
    }
    #[test]
    fn abandoned_planning_restores_names_imports_and_adapter_declarations() {
        use crate::plan::declarations::{DeclarationKind, GoDeclaration};
        use crate::types::go_type::GoType;
        let input = EmitInput::default();
        let shared = SharedEmitContext {
            go_module: "example.com/test",
            entry_package_name: "main",
            emit_tests: false,
            line_indexes: None,
            globals: GlobalEmitData::default(),
        };
        let facts = EmitFacts::new(&input, &shared, "test".into());
        let mut package = PackageState::default();
        let mut adapters = AdapterRegistry::default();
        let mut namespace = FileNamespace::default();
        let mut planner = Planner::new(facts, &mut package, &mut adapters, &mut namespace);
        let checkpoint = planner.planning_snapshot();
        planner.fresh_local(Some("temporary"));
        planner
            .namespace
            .require(PackageUse::generated(GeneratedPackage::Errors));
        let key = (Type::int(), Type::bool());
        planner
            .adapter_registry
            .register(Some(key.clone()), |_| "abandoned".to_string());
        planner
            .namespace
            .collect_adapters(vec![GoDeclaration::new(DeclarationKind::Type {
                name: "abandoned".to_string(),
                parameters: vec![],
                underlying: GoType::new("int"),
            })]);
        planner.restore_planning(checkpoint);
        assert_eq!(
            planner.fresh_local(Some("temporary")).spelling(),
            "temporary"
        );
        assert!(planner.adapter_registry.lookup(&key).is_none());
        assert!(planner.namespace.take_adapters().is_empty());
        let (imports, source, diagnostics) = namespace.finish(OutputCollector::new());
        assert!(imports.is_empty());
        assert!(source.is_empty());
        assert!(diagnostics.is_empty());
    }
}
