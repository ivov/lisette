use crate::target::names::go_name::escape_local;
use std::ptr;

use super::local_names::LocalNames;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::lookup::facts::reusable_bindings::ReusableBindings;
use crate::lookup::state::bindings::{BindingValue, ComponentBinding, InlineExpr, TupleBinding};
use crate::lookup::state::enclosing::LoopContext;
use crate::lookup::state::enclosing::ReturnContext;
use crate::target::ir::bodies::LoopId;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use syntax::EcoString;
use syntax::ast::{BindingId, Expression, IdentifierResolution};
use syntax::constant::Constant;
use syntax::types::Type;

#[derive(Clone)]
pub(crate) struct ScopeState {
    pub(super) names: LocalNames,
    next_local_id: u32,
    next_loop_id: u32,
    frames: Vec<ScopeFrame>,
    type_params: Vec<TypeParam>,
    reusable_bindings: ReusableBindings,
    local_constants: FxHashMap<BindingId, Constant>,
}

#[derive(Clone)]
struct Binding {
    name: String,
    ids: Vec<BindingId>,
    value: BindingValue,
}

#[derive(Clone)]
struct ScopeFrame {
    bindings: Vec<Binding>,
    declarations: DeclarationScope,
    established: Vec<GoExpression>,
}

#[derive(Clone)]
enum DeclarationScope {
    Transparent,
    Block(Declarations),
    Function(FunctionScope),
}

#[derive(Clone)]
struct FunctionScope {
    declarations: Declarations,
    return_ctx: ReturnContext,
    test_handle: Option<GoIdentifier>,
    loops: Vec<LoopContext>,
    assign_targets: Vec<GoIdentifier>,
}

impl FunctionScope {
    fn new(declarations: Declarations, return_ctx: ReturnContext) -> Self {
        Self {
            declarations,
            return_ctx,
            test_handle: None,
            loops: Vec::new(),
            assign_targets: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Declarations {
    names: FxHashSet<String>,
    spent_errors: Vec<(GoIdentifier, Type)>,
}

impl Declarations {
    fn contains(&self, go_name: &str) -> bool {
        self.names.contains(go_name)
    }

    fn insert(&mut self, go_name: &str) -> bool {
        self.spent_errors
            .retain(|(name, _)| name.spelling() != go_name);
        self.names.insert(go_name.to_string())
    }

    fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Clone)]
pub(crate) struct TypeParam {
    pub(crate) name: EcoString,
    pub(crate) go_name: String,
    pub(crate) bounds: Vec<Type>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairStatusKind {
    Error,
    Ok,
}

impl ScopeFrame {
    fn new(declarations: DeclarationScope) -> Self {
        Self {
            bindings: Vec::new(),
            declarations,
            established: Vec::new(),
        }
    }
}

impl ScopeState {
    pub(crate) fn new() -> Self {
        Self {
            names: LocalNames::default(),
            next_local_id: 0,
            next_loop_id: 0,
            frames: vec![ScopeFrame::new(DeclarationScope::Function(
                FunctionScope::new(Declarations::default(), ReturnContext::None),
            ))],
            type_params: Vec::new(),
            reusable_bindings: ReusableBindings::default(),
            local_constants: FxHashMap::default(),
        }
    }

    pub(crate) fn set_type_params(&mut self, type_params: Vec<TypeParam>) {
        self.type_params = type_params;
    }

    pub(crate) fn type_params(&self) -> &[TypeParam] {
        &self.type_params
    }

    pub(crate) fn declares_type_param(&self, go_name: &str) -> bool {
        self.type_params
            .iter()
            .any(|param| param.go_name == go_name)
    }

    pub(crate) fn bind_new_local(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        go_name: impl Into<GoIdentifier>,
    ) -> GoIdentifier {
        self.bind_with_new_identifier(
            lisette_name.into(),
            ids,
            go_name.into(),
            BindingValue::GoName,
        )
    }

    pub(crate) fn record_local_constant(&mut self, binding: BindingId, constant: Constant) {
        self.local_constants.insert(binding, constant);
    }

    pub(crate) fn local_constant(&self, binding: BindingId) -> Option<Constant> {
        self.local_constants.get(&binding).cloned()
    }

    pub(crate) fn bind_new_const(
        &mut self,
        lisette_name: &str,
        ids: &[BindingId],
        go_name: GoIdentifier,
    ) -> GoIdentifier {
        self.bind_with_new_identifier(
            lisette_name.to_string(),
            ids,
            go_name,
            BindingValue::GoConst,
        )
    }

    fn bind_with_new_identifier(
        &mut self,
        lisette_name: String,
        ids: &[BindingId],
        go_name: GoIdentifier,
        value: fn(GoIdentifier) -> BindingValue,
    ) -> GoIdentifier {
        let identifier = self.new_local_identifier(go_name);
        self.push_binding(lisette_name, ids, value(identifier.clone()));
        identifier
    }

    pub(crate) fn new_local_identifier(
        &mut self,
        go_name: impl Into<GoIdentifier>,
    ) -> GoIdentifier {
        let mut name = go_name.into();
        *name.spelling_mut() = escape_local(name.spelling()).into_owned();
        if name.id().is_none() {
            name.identify(self.new_local_id());
        }
        name
    }

    pub(crate) fn bind_existing_identifier(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        identifier: GoIdentifier,
    ) {
        self.push_binding(lisette_name.into(), ids, BindingValue::GoName(identifier));
    }

    pub(crate) fn new_local_id(&mut self) -> LocalId {
        let id = LocalId(self.next_local_id);
        self.next_local_id += 1;
        id
    }

    pub(crate) fn bind_components_filling_ids(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        mut components: ComponentBinding,
    ) {
        self.identify_binding_name(&mut components.value);
        self.identify_binding_name(&mut components.status);
        self.push_binding(
            lisette_name.into(),
            ids,
            BindingValue::Components(components),
        );
    }

    pub(crate) fn bind_tuple_filling_ids(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        mut tuple: TupleBinding,
    ) {
        for name in &mut tuple.names {
            self.identify_binding_name(name);
        }
        self.push_binding(
            lisette_name.into(),
            ids,
            BindingValue::TupleComponents(tuple),
        );
    }

    fn identify_binding_name(&mut self, name: &mut GoIdentifier) {
        if name.spelling() == "_" || name.id().is_some() {
            return;
        }
        let id = self.new_local_id();
        name.identify(id);
    }

    pub(crate) fn bind_inline_expr(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        expr: InlineExpr,
    ) {
        self.push_binding(lisette_name.into(), ids, BindingValue::InlineExpr(expr));
    }

    pub(crate) fn resolve_identifier_binding(&self, lisette_name: &str) -> Option<&BindingValue> {
        self.bindings()
            .find(|binding| binding.name == lisette_name)
            .map(|binding| &binding.value)
    }

    fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.frames
            .iter()
            .rev()
            .flat_map(|frame| frame.bindings.iter().rev())
    }

    fn visible_bindings(&self) -> impl Iterator<Item = &Binding> {
        let mut seen = FxHashSet::default();
        self.bindings()
            .filter(move |binding| seen.insert(binding.name.as_str()))
    }

    pub(crate) fn bound_go_identifier(&self, lisette_name: &str) -> Option<&GoIdentifier> {
        match self.resolve_identifier_binding(lisette_name) {
            Some(BindingValue::GoName(name) | BindingValue::GoConst(name)) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn set_reusable_bindings(&mut self, bindings: ReusableBindings) {
        self.reusable_bindings = bindings;
    }

    pub(crate) fn reusable_binding(&self, id: BindingId) -> Option<GoIdentifier> {
        let hidden = self.reusable_bindings.hidden_by(id)?;
        let Some(BindingValue::GoName(name)) = self.resolve_binding_id(hidden) else {
            return None;
        };
        (self.is_go_name_declared(name) && !self.is_active_assign_target(name))
            .then(|| name.clone())
    }

    pub(crate) fn resolve_binding_id(&self, id: BindingId) -> Option<&BindingValue> {
        self.bindings()
            .find(|binding| binding.ids.contains(&id))
            .map(|binding| &binding.value)
    }

    fn id_resolves_to(&self, id: BindingId, binding: &Binding) -> bool {
        self.resolve_binding_id(id)
            .is_some_and(|value| ptr::eq(value, &binding.value))
    }

    fn is_reachable(&self, binding: &Binding) -> bool {
        self.resolve_identifier_binding(&binding.name)
            .is_some_and(|value| ptr::eq(value, &binding.value))
            || binding
                .ids
                .iter()
                .any(|id| self.id_resolves_to(*id, binding))
    }

    pub(crate) fn source_binding_for_local(&self, local: &GoIdentifier) -> Option<BindingId> {
        let local_id = local.id()?;
        self.bindings().find_map(|binding| {
            if !matches!(&binding.value, BindingValue::GoName(name) if name.id() == Some(local_id))
            {
                return None;
            }
            binding
                .ids
                .iter()
                .copied()
                .filter(|id| self.id_resolves_to(*id, binding))
                .max()
        })
    }

    pub(crate) fn resolve_identifier_with_resolution(
        &self,
        value: &str,
        resolution: &IdentifierResolution,
    ) -> Option<&BindingValue> {
        match resolution {
            IdentifierResolution::Binding(id) => self.resolve_binding_id(*id),
            IdentifierResolution::Definition { .. } | IdentifierResolution::Unresolved => {
                self.resolve_identifier_binding(value)
            }
        }
    }

    pub(crate) fn lacks_binding_for_local(&self, local: &GoIdentifier) -> bool {
        let Some(id) = local.id() else {
            return true;
        };
        self.active_assign_targets()
            .all(|target| target.id() != Some(id))
            && self
                .bindings()
                .all(|binding| !binding.value.mentions_local(local) || !self.is_reachable(binding))
    }

    pub(crate) fn has_binding_for_go_name(&self, go_name: &str) -> bool {
        self.is_active_assign_target(go_name)
            || self
                .visible_bindings()
                .any(|binding| binding.value.mentions(go_name))
    }

    pub(crate) fn expression_reads_go_name(&self, expression: &Expression, go_name: &str) -> bool {
        match expression {
            Expression::Identifier {
                value, resolution, ..
            } => self
                .resolve_identifier_with_resolution(value, resolution)
                .is_some_and(|binding| binding.mentions(go_name)),
            _ => expression
                .children()
                .into_iter()
                .any(|child| self.expression_reads_go_name(child, go_name)),
        }
    }

    pub(crate) fn go_name_bound_by_other_lisette_name(
        &self,
        go_name: &str,
        lisette_name: &str,
    ) -> bool {
        self.visible_bindings()
            .any(|binding| binding.name != lisette_name && binding.value.mentions(go_name))
    }

    pub(crate) fn go_name_bound_outside_ids(&self, go_name: &str, ids: &[BindingId]) -> bool {
        self.visible_bindings().any(|binding| {
            binding.value.mentions(go_name)
                && !ids.iter().any(|id| self.id_resolves_to(*id, binding))
        })
    }

    pub(crate) fn push_binding_frame(&mut self) {
        self.push_frame(DeclarationScope::Transparent);
    }

    pub(crate) fn pop_binding_frame(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Transparent
            ),
            "a binding frame must be pushed before it is popped"
        );
        self.pop_frame();
    }

    pub(crate) fn declare_go_name(&mut self, go_name: &str) {
        self.current_declarations_mut().insert(go_name);
    }

    pub(crate) fn declare_spent_error(&mut self, name: &GoIdentifier, error_ty: &Type) {
        let declarations = self.current_declarations_mut();
        declarations.insert(name);
        declarations
            .spent_errors
            .push((name.clone(), error_ty.clone()));
    }

    pub(crate) fn spent_error(&self, error_ty: &Type) -> Option<GoIdentifier> {
        self.current_declarations()
            .spent_errors
            .iter()
            .find(|(_, ty)| ty == error_ty)
            .map(|(name, _)| name.clone())
    }

    pub(crate) fn try_declare_go_name(&mut self, go_name: &str) -> bool {
        !self.declares_type_param(go_name) && self.current_declarations_mut().insert(go_name)
    }

    pub(crate) fn current_block_declares(&self, go_name: &str) -> bool {
        self.declares_type_param(go_name) || self.current_declarations().contains(go_name)
    }

    pub(crate) fn is_go_name_declared(&self, go_name: &str) -> bool {
        if self.declares_type_param(go_name) {
            return true;
        }
        for frame in self.frames.iter().rev() {
            match &frame.declarations {
                DeclarationScope::Transparent => {}
                DeclarationScope::Block(names) if names.contains(go_name) => return true,
                DeclarationScope::Block(_) => {}
                DeclarationScope::Function(function) => {
                    return function.declarations.contains(go_name);
                }
            }
        }
        false
    }

    pub(crate) fn current_block_declared_nonempty(&self) -> bool {
        !self.current_declarations().is_empty()
    }

    pub(crate) fn enter_block(&mut self) {
        self.push_frame(DeclarationScope::Block(Declarations::default()));
    }

    pub(crate) fn exit_block(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Block(_)
            ),
            "a block must be entered before it is exited"
        );
        self.pop_frame();
    }

    pub(crate) fn establish_condition(&mut self, condition: GoExpression) {
        self.frames
            .iter_mut()
            .rev()
            .find(|frame| !matches!(frame.declarations, DeclarationScope::Transparent))
            .expect("scope state always retains a declaration scope")
            .established
            .push(condition);
    }

    pub(crate) fn is_condition_established(&self, condition: &GoExpression) -> bool {
        for frame in self.frames.iter().rev() {
            if frame
                .established
                .iter()
                .any(|established| established.node() == condition.node())
            {
                return true;
            }
            if matches!(frame.declarations, DeclarationScope::Function(_)) {
                return false;
            }
        }
        false
    }

    pub(crate) fn enter_isolated_function(&mut self, return_ctx: ReturnContext) {
        self.push_frame(DeclarationScope::Function(FunctionScope::new(
            Declarations::default(),
            return_ctx,
        )));
    }

    pub(crate) fn exit_isolated_function(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Function(_)
            ),
            "an isolated function must be entered before it is exited"
        );
        self.pop_frame();
    }

    pub(crate) fn push_loop(&mut self, result: Option<GoExpression>, result_ty: Option<Type>) {
        let id = LoopId(self.next_loop_id);
        self.next_loop_id += 1;
        self.current_function_mut().loops.push(LoopContext {
            id,
            result,
            result_ty,
        });
    }

    pub(crate) fn pop_loop(&mut self) {
        self.current_function_mut()
            .loops
            .pop()
            .expect("a loop context must be pushed before it is popped");
    }

    pub(crate) fn set_test_handle(&mut self, handle: GoIdentifier) {
        self.current_function_mut().test_handle = Some(handle);
    }

    pub(crate) fn current_test_handle(&self) -> Option<&GoIdentifier> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Function(function) => function.test_handle.as_ref(),
                _ => None,
            })
    }

    pub(crate) fn current_return_ctx(&self) -> ReturnContext {
        self.current_function().return_ctx.clone()
    }

    pub(crate) fn current_loop(&self) -> Option<&LoopContext> {
        self.current_function().loops.last()
    }

    pub(crate) fn current_loop_id(&self) -> Option<LoopId> {
        self.current_function()
            .loops
            .last()
            .map(|context| context.id)
    }

    pub(crate) fn push_assign_target(&mut self, name: GoIdentifier) {
        self.current_function_mut().assign_targets.push(name);
    }

    pub(crate) fn pop_assign_target(&mut self) {
        self.current_function_mut()
            .assign_targets
            .pop()
            .expect("an assign target must be pushed before it is popped");
    }

    pub(crate) fn is_active_assign_target(&self, var: &str) -> bool {
        self.active_assign_targets()
            .any(|target| target.spelling() == var)
    }

    fn active_assign_targets(&self) -> impl Iterator<Item = &GoIdentifier> {
        let function = self.current_function();
        function.assign_targets.iter().chain(
            function
                .loops
                .iter()
                .filter_map(|context| context.result.as_ref()?.as_identifier())
                .filter(|name| name.spelling() != "_"),
        )
    }

    fn current_function(&self) -> &FunctionScope {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Function(function) => Some(function),
                _ => None,
            })
            .expect("scope state always retains a function scope")
    }

    fn current_function_mut(&mut self) -> &mut FunctionScope {
        self.frames
            .iter_mut()
            .rev()
            .find_map(|frame| match &mut frame.declarations {
                DeclarationScope::Function(function) => Some(function),
                _ => None,
            })
            .expect("scope state always retains a function scope")
    }

    fn push_frame(&mut self, declarations: DeclarationScope) {
        self.frames.push(ScopeFrame::new(declarations));
    }

    fn pop_frame(&mut self) {
        assert!(self.frames.len() > 1, "cannot pop a stack's base frame");
        self.frames.pop();
    }

    fn current_frame(&self) -> &ScopeFrame {
        self.frames
            .last()
            .expect("scope state always retains a frame")
    }

    fn current_frame_mut(&mut self) -> &mut ScopeFrame {
        self.frames
            .last_mut()
            .expect("scope state always retains a frame")
    }

    fn push_binding(&mut self, name: String, ids: &[BindingId], value: BindingValue) {
        self.current_frame_mut().bindings.push(Binding {
            name,
            ids: ids.to_vec(),
            value,
        });
    }

    fn current_declarations(&self) -> &Declarations {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(names)
                | DeclarationScope::Function(FunctionScope {
                    declarations: names,
                    ..
                }) => Some(names),
            })
            .expect("scope state always retains a declaration scope")
    }

    fn current_declarations_mut(&mut self) -> &mut Declarations {
        self.frames
            .iter_mut()
            .rev()
            .find_map(|frame| match &mut frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(names)
                | DeclarationScope::Function(FunctionScope {
                    declarations: names,
                    ..
                }) => Some(names),
            })
            .expect("scope state always retains a declaration scope")
    }
}
