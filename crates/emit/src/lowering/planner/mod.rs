mod collectors;
mod collisions;
mod generics;
mod package;
mod resolution;

use crate::OutputFile;
use crate::lookup::facts::context::LineIndex;
use crate::lookup::facts::context::{GlobalEmitData, SharedEmitContext};
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::adapter_registry::AdapterRegistry;
use crate::lookup::state::conversion_record::ConversionRecord;
use crate::lookup::state::enclosing::LoopContext;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lookup::state::scope::ScopeState;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::return_abi;
use crate::lowering::abi::slot_coercion::check_applied_conversions;
use crate::output::render::Renderer;
use crate::output::render::output::OutputCollector;
use crate::target::ir::bodies::{
    LoopId, LoopTransfer, LoweredBlock, LoweredStatement, Statement, define,
};
use crate::target::ir::declarations::{DeclarationKind, GoDeclaration};
use crate::target::ir::go_expression::{GoExpressionNode, TreeVisitor};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::packages::{PackageRequirements, PackageUse};
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use crate::target::types::go_type::is_prelude_type;
use crate::target::types::return_abi::CallableReturnAbi;
use diagnostics::LisetteDiagnostic;
use package::{PackagePlan, build_package_plan};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::PathBuf;
use syntax::ast::{BindingId, Expression, Span};
use syntax::program::{EmitInput, File};
use syntax::types::Type;

#[derive(Clone, Debug)]
pub struct EmitOptions {
    pub sourcemap: bool,
    pub sourcemap_base: Option<PathBuf>,
    pub emit_tests: bool,
}

pub struct Planner<'a> {
    pub(in crate::lowering) facts: EmitFacts<'a>,
    pub(in crate::lowering) names: &'a PackageNames,
    pub(in crate::lowering) conversions: &'a mut ConversionRecord,
    pub(in crate::lowering) scope: ScopeState,
    pub(in crate::lowering) adapter_registry: &'a mut AdapterRegistry,
    pub(in crate::lowering) namespace: &'a mut FileNamespace,
}

pub(in crate::lowering) struct PlanningSnapshot {
    pub(in crate::lowering) conversions: ConversionRecord,
    pub(in crate::lowering) scope: ScopeState,
    pub(in crate::lowering) adapter_registry: AdapterRegistry,
    pub(in crate::lowering) namespace: FileNamespace,
}

impl<'a> Planner<'a> {
    pub(in crate::lowering) fn planning_snapshot(&self) -> PlanningSnapshot {
        let Self {
            facts: _,
            names: _,
            conversions,
            scope,
            adapter_registry,
            namespace,
        } = self;
        PlanningSnapshot {
            conversions: (**conversions).clone(),
            scope: scope.clone(),
            adapter_registry: (**adapter_registry).clone(),
            namespace: (**namespace).clone(),
        }
    }

    pub(in crate::lowering) fn restore_planning(&mut self, saved: PlanningSnapshot) {
        let PlanningSnapshot {
            conversions,
            scope,
            adapter_registry,
            namespace,
        } = saved;
        *self.conversions = conversions;
        self.scope = scope;
        *self.adapter_registry = adapter_registry;
        *self.namespace = namespace;
    }

    pub(in crate::lowering) fn require_testkit(&mut self) {
        self.require_generated_package(GeneratedPackage::TestKit);
    }

    pub(in crate::lowering) fn require_generated_package(&mut self, package: GeneratedPackage) {
        self.namespace.require(PackageUse::generated(package));
    }

    pub(in crate::lowering) fn require_packages(&mut self, requirements: &PackageRequirements) {
        self.namespace.absorb(requirements);
    }

    pub(in crate::lowering) fn collect_declaration_imports(&mut self, declaration: &GoDeclaration) {
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

    pub(in crate::lowering) fn collect_imports(&mut self, statements: &[Statement]) {
        let mut collector = ImportCollector(self.namespace);
        for statement in statements {
            statement.kind.walk(&mut collector);
        }
    }

    pub(in crate::lowering) fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        let abi = return_abi(&self.facts, &return_ty, ReturnSlot::Function);
        Self::return_context_from_abi(return_ty, abi)
    }

    pub(in crate::lowering) fn return_context_for_method(&self, return_ty: Type) -> ReturnContext {
        let abi = return_abi(&self.facts, &return_ty, ReturnSlot::Method);
        Self::return_context_from_abi(return_ty, abi)
    }

    pub(in crate::lowering) fn return_context_from_abi(
        return_ty: Type,
        abi: CallableReturnAbi,
    ) -> ReturnContext {
        match abi {
            CallableReturnAbi::Void => ReturnContext::Void(return_ty),
            CallableReturnAbi::Lowered(shape) => ReturnContext::Lowered { return_ty, shape },
            CallableReturnAbi::Value => ReturnContext::Tagged(return_ty),
        }
    }

    pub fn emit(
        analysis: &'a mut EmitInput,
        go_module: &str,
        entry_package_name: &'a str,
        options: EmitOptions,
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let cached_packages = &analysis.cached_packages;
        for file in analysis.files.values_mut() {
            if !is_emitted(file, cached_packages) {
                continue;
            }
            for item in &mut file.items {
                item.strip_wrappers();
            }
            debug_assert!(
                !file.items.iter().any(Expression::contains_wrapper),
                "internal error: a parenthesized expression, a single-expression block or a reference read once reached emit"
            );
        }
        let analysis: &'a EmitInput = analysis;

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
        for file in analysis.files.values() {
            if is_emitted(file, &analysis.cached_packages) {
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

    pub(in crate::lowering) fn new(
        facts: EmitFacts<'a>,
        names: &'a PackageNames,
        conversions: &'a mut ConversionRecord,
        adapter_registry: &'a mut AdapterRegistry,
        namespace: &'a mut FileNamespace,
    ) -> Self {
        Self {
            facts,
            names,
            conversions,
            scope: ScopeState::new(),
            adapter_registry,
            namespace,
        }
    }

    pub(in crate::lowering) fn with_loop<R>(
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

    pub(in crate::lowering) fn current_loop(&self) -> Option<&LoopContext> {
        self.scope.current_loop()
    }

    pub(in crate::lowering) fn current_loop_id(&self) -> Option<LoopId> {
        self.scope.current_loop_id()
    }

    pub(in crate::lowering) fn current_loop_transfer(&self) -> LoopTransfer {
        self.current_loop_id()
            .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source)
    }

    pub(in crate::lowering) fn return_ctx(&self) -> ReturnContext {
        self.scope.current_return_ctx()
    }

    pub(in crate::lowering) fn try_declare(&mut self, go_name: &str) -> bool {
        self.scope.try_declare_go_name(go_name)
    }

    pub(in crate::lowering) fn is_declared(&self, go_name: &str) -> bool {
        self.scope.is_go_name_declared(go_name)
    }

    pub(in crate::lowering) fn shadows_declaration(&self, go_name: &str) -> bool {
        self.is_declared(go_name) || self.names.is_package_block_name(go_name)
    }

    pub(in crate::lowering) fn is_condition_established(&self, condition: &GoExpression) -> bool {
        self.scope.is_condition_established(condition)
    }

    pub(in crate::lowering) fn declare(&mut self, go_name: &str) {
        self.scope.declare_go_name(go_name);
    }

    pub(in crate::lowering) fn claim_declared_binding(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: &SourceName,
    ) -> GoIdentifier {
        let go_name = self.claim_declared_go_name(ids, preferred);
        self.scope.bind_source(lisette_name, ids, go_name)
    }

    pub(in crate::lowering) fn claim_block_binding(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: &SourceName,
        outer: OuterBindings,
    ) -> GoIdentifier {
        self.claim_block_binding_as(lisette_name, ids, preferred.into(), preferred, outer)
    }

    pub(in crate::lowering) fn claim_block_binding_as(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: GoIdentifier,
        source: &SourceName,
        outer: OuterBindings,
    ) -> GoIdentifier {
        let go_name = self.scope.new_local_identifier(preferred);
        let hides = self.names.is_package_block_name(&go_name)
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

    pub(in crate::lowering) fn claim_declared_go_name(
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
                .fresh_binding_go_name(preferred.base(), self.names);
            self.scope.new_local_identifier(fresh)
        } else {
            go_name
        };
        self.declare(&go_name);
        go_name
    }

    pub(in crate::lowering) fn stable_source(
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

    pub(in crate::lowering) fn hoist_tmp_value_statement(
        &mut self,
        setup: &mut Vec<Statement>,
        hint: &'static str,
        value: GoExpression,
    ) -> GoIdentifier {
        let tmp = self.fresh_declared(hint);
        setup.push(define(tmp.clone(), value));
        tmp
    }

    pub(in crate::lowering) fn hoist_source_value(
        &mut self,
        setup: &mut Vec<Statement>,
        name: &SourceName,
        value: GoExpression,
    ) -> GoIdentifier {
        let local = self.fresh_declared_source(name);
        setup.push(define(local.clone(), value));
        local
    }

    pub(in crate::lowering) fn capture_scoped_block<F>(&mut self, f: F) -> Option<LoweredBlock>
    where
        F: FnOnce(&mut Self) -> LoweredBlock,
    {
        let block = self.with_scope(f);
        (!block.renders_empty()).then_some(block)
    }

    pub(in crate::lowering) fn enter_scope(&mut self) {
        self.scope.enter_block();
    }

    pub(in crate::lowering) fn exit_scope(&mut self) {
        self.scope.exit_block();
    }

    pub(in crate::lowering) fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.enter_scope();
        let result = f(self);
        self.exit_scope();
        result
    }

    pub(in crate::lowering) fn focused<R>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.namespace.rejections().enter(span);
        let result = f(self);
        self.namespace.rejections().leave(previous);
        result
    }

    pub(in crate::lowering) fn with_declaration_scope<R>(
        &mut self,
        f: impl FnOnce(&mut Planner<'_>) -> R,
    ) -> R {
        let mut declaration = Planner::new(
            self.facts.clone(),
            self.names,
            self.conversions,
            self.adapter_registry,
            self.namespace,
        );
        f(&mut declaration)
    }

    pub(in crate::lowering) fn with_binding_frame<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.push_binding_frame();
        let result = f(self);
        self.scope.pop_binding_frame();
        result
    }

    pub(in crate::lowering) fn with_isolated_function<R>(
        &mut self,
        return_ctx: ReturnContext,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.enter_isolated_function(return_ctx);
        let result = f(self);
        self.scope.exit_isolated_function();
        result
    }

    pub(in crate::lowering) fn with_assign_target<R>(
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

    pub(in crate::lowering) fn fresh_declared(&mut self, hint: &'static str) -> GoIdentifier {
        let name = self.fresh_local(Some(hint));
        self.declare(&name);
        name
    }

    pub(in crate::lowering) fn fresh_declared_source(&mut self, name: &SourceName) -> GoIdentifier {
        let local = self.fresh_source(name);
        self.declare(&local);
        local
    }

    pub(in crate::lowering) fn fresh_local(&mut self, hint: Option<&'static str>) -> GoIdentifier {
        self.scope.fresh_go_name(hint, self.names)
    }

    pub(in crate::lowering) fn fresh_source(&mut self, name: &SourceName) -> GoIdentifier {
        self.scope.fresh_go_name(Some(name.base()), self.names)
    }

    pub(in crate::lowering) fn fresh_like(&mut self, name: &GoIdentifier) -> GoIdentifier {
        self.scope.fresh_go_name(Some(name.spelling()), self.names)
    }

    pub(in crate::lowering) fn emit_files_with_facts(
        facts: EmitFacts<'a>,
        files: &[&File],
    ) -> Result<Vec<OutputFile>, Vec<LisetteDiagnostic>> {
        let PackagePlan {
            package_name,
            names,
            collision_diagnostics,
            files: file_plans,
        } = build_package_plan(&facts, files);
        let mut conversions = ConversionRecord::default();
        let mut adapters = AdapterRegistry::default();
        let mut output_files = Vec::new();
        let mut all_diagnostics = collision_diagnostics;

        for plan in file_plans {
            let file = plan.file;
            let mut namespace = FileNamespace::new(plan.imports);
            let source = Planner::new(
                facts.clone(),
                &names,
                &mut conversions,
                &mut adapters,
                &mut namespace,
            )
            .render_file_source(file);
            let source = source.render();
            let (imports, mut diagnostics) = namespace.finish();
            all_diagnostics.append(&mut diagnostics);
            output_files.push(OutputFile {
                name: file.go_filename(),
                imports,
                source,
                package_name: package_name.clone(),
                file_comment: file.file_comment.clone(),
            });
        }

        let (applied, dropped_code) = conversions.recorded();
        check_applied_conversions(&facts, files, applied, dropped_code);
        if all_diagnostics.is_empty() {
            Ok(output_files)
        } else {
            Err(all_diagnostics)
        }
    }

    pub(in crate::lowering) fn render_file_source(&mut self, file: &File) -> OutputCollector {
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
                declaration.position = self.facts.source_position(&variant.name_span);
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

#[derive(Clone, Copy)]
pub(in crate::lowering) enum OuterBindings {
    MayShadow,
    StayVisible,
}

pub(in crate::lowering) fn emit_package<'a>(
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

pub(in crate::lowering) fn written_span(expression: &Expression) -> Span {
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

fn is_emitted(file: &File, cached_packages: &HashSet<String>) -> bool {
    !file.is_d_lis() && !cached_packages.contains(&file.package_id)
}
