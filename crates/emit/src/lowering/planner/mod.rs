mod collectors;
mod collisions;
mod generics;
pub(in crate::lowering) mod package;
mod resolution;

use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::adapter_registry::AdapterRegistry;
use crate::lookup::state::conversion_record::ConversionRecord;
use crate::lookup::state::enclosing::ReturnContext;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lookup::state::scope::ScopeState;
use crate::target::ir::bodies::{LoopTransfer, LoweredBlock, LoweredStatement, Statement, define};
use crate::target::ir::declarations::{DeclarationKind, GoDeclaration};
use crate::target::ir::go_expression::{GoExpressionNode, TreeVisitor};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::names::packages::{PackageRequirements, PackageUse};
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use syntax::ast::{BindingId, Expression, Span};
use syntax::types::Type;

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
        result: Option<GoExpression>,
        result_ty: Option<Type>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.scope.push_loop(result, result_ty);
        let result = f(self);
        self.scope.pop_loop();
        result
    }

    pub(in crate::lowering) fn current_loop_transfer(&self) -> LoopTransfer {
        self.scope
            .current_loop_id()
            .map_or(LoopTransfer::Unlabeled, LoopTransfer::Source)
    }

    pub(in crate::lowering) fn shadows_declaration(&self, go_name: &str) -> bool {
        self.scope.is_go_name_declared(go_name) || self.names.is_package_block_name(go_name)
    }

    pub(in crate::lowering) fn claim_declared_binding(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        preferred: &SourceName,
    ) -> GoIdentifier {
        let go_name = self.claim_declared_go_name(ids, preferred);
        self.scope.bind_new_local(lisette_name, ids, go_name)
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
                            .go_name_bound_by_other_lisette_name(&go_name, lisette_name)
                }
                OuterBindings::StayVisible => self.scope.has_binding_for_go_name(&go_name),
            };

        let go_name = if hides {
            self.fresh_local(source)
        } else {
            go_name
        };
        self.scope.declare_go_name(&go_name);
        self.scope.bind_new_local(lisette_name, ids, go_name)
    }

    pub(in crate::lowering) fn claim_declared_go_name(
        &mut self,
        ids: &[BindingId],
        preferred: &SourceName,
    ) -> GoIdentifier {
        let go_name = self.scope.new_local_identifier(preferred);
        let go_name = if self.shadows_declaration(&go_name)
            || self.scope.go_name_bound_outside_ids(&go_name, ids)
        {
            let fresh = self
                .scope
                .fresh_binding_go_name(preferred.base(), self.names);
            self.scope.new_local_identifier(fresh)
        } else {
            go_name
        };

        self.scope.declare_go_name(&go_name);
        go_name
    }

    pub(in crate::lowering) fn stable_source<'h>(
        &mut self,
        statements: &mut Vec<Statement>,
        hint: impl Into<NameHint<'h>>,
        value: GoExpression,
    ) -> GoExpression {
        if value.as_identifier().is_some() {
            return value;
        }
        GoExpression::identifier(self.hoist_temp(statements, hint, value))
    }

    pub(in crate::lowering) fn hoist_temp<'h>(
        &mut self,
        setup: &mut Vec<Statement>,
        hint: impl Into<NameHint<'h>>,
        value: GoExpression,
    ) -> GoIdentifier {
        let local = self.fresh_declared(hint);
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

    pub(in crate::lowering) fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scope.enter_block();
        let result = f(self);
        self.scope.exit_block();
        result
    }

    pub(in crate::lowering) fn with_rejection_span<R>(
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

    pub(in crate::lowering) fn fresh_declared<'h>(
        &mut self,
        hint: impl Into<NameHint<'h>>,
    ) -> GoIdentifier {
        let name = self.fresh_local(hint);
        self.scope.declare_go_name(&name);
        name
    }

    pub(in crate::lowering) fn fresh_local<'h>(
        &mut self,
        hint: impl Into<NameHint<'h>>,
    ) -> GoIdentifier {
        let spelling = hint.into().spelling();
        self.scope.fresh_go_name(Some(spelling), self.names)
    }
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum NameHint<'h> {
    Static(&'static str),
    Source(&'h SourceName),
    Like(&'h GoIdentifier),
}

impl<'h> NameHint<'h> {
    fn spelling(self) -> &'h str {
        match self {
            Self::Static(hint) => hint,
            Self::Source(name) => name.base(),
            Self::Like(identifier) => identifier.spelling(),
        }
    }
}

impl From<&'static str> for NameHint<'_> {
    fn from(hint: &'static str) -> Self {
        Self::Static(hint)
    }
}

impl<'h> From<&'h SourceName> for NameHint<'h> {
    fn from(name: &'h SourceName) -> Self {
        Self::Source(name)
    }
}

impl<'h> From<&'h GoIdentifier> for NameHint<'h> {
    fn from(identifier: &'h GoIdentifier) -> Self {
        Self::Like(identifier)
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
