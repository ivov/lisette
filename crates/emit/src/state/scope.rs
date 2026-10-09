mod names;

use names::LocalNames;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::mem;

use crate::ReturnContext;
use crate::context::lowering::LoopContext;
use crate::plan::bodies::LoopId;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::values::GoExpression;
use crate::state::bindings::{BindingValue, ComponentBinding, InlineExpr, TupleBinding};
use syntax::EcoString;
use syntax::ast::{BindingId, IdentifierResolution};
use syntax::types::Type;

pub(crate) struct ScopeState {
    names: LocalNames,
    next_local_id: u32,
    next_loop_id: u32,
    frames: Vec<ScopeFrame>,
    loop_stack: Vec<LoopContext>,
    assign_targets: HashSet<String>,
    /// Type parameters in scope with their bounds, receiver first.
    type_params: Vec<(EcoString, Vec<Type>)>,
}

struct ScopeFrame {
    bindings: HashMap<String, usize>,
    binding_ids: HashMap<BindingId, usize>,
    binding_values: Vec<BindingValue>,
    declarations: DeclarationScope,
    /// Conditions the branch being lowered has already tested in this scope.
    established: Vec<GoExpression>,
}

enum DeclarationScope {
    /// A semantic binding scope that emits no Go braces. Declarations belong
    /// to the nearest enclosing Go scope.
    Transparent,
    Block(Declarations),
    /// A Go function body: hides outer declarations and lowers `return` against `return_ctx`.
    Function {
        declarations: Declarations,
        return_ctx: ReturnContext,
        test_handle: Option<GoIdentifier>,
    },
}

type Declarations = HashMap<String, DeclarationKind>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    Local,
    TypeParameter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PairStatusKind {
    Error,
    Ok,
}

impl ScopeState {
    pub(crate) fn new() -> Self {
        Self {
            names: LocalNames::default(),
            next_local_id: 0,
            next_loop_id: 0,
            frames: vec![ScopeFrame {
                bindings: HashMap::default(),
                binding_ids: HashMap::default(),
                binding_values: Vec::new(),
                declarations: DeclarationScope::Block(HashMap::default()),
                established: Vec::new(),
            }],
            loop_stack: Vec::new(),
            assign_targets: HashSet::default(),
            type_params: Vec::new(),
        }
    }

    pub(crate) fn begin_declaration(&mut self) -> ScopeState {
        mem::replace(self, Self::new())
    }

    pub(crate) fn end_declaration(&mut self, outer: ScopeState) {
        *self = outer;
    }

    pub(crate) fn set_type_params(&mut self, type_params: Vec<(EcoString, Vec<Type>)>) {
        self.type_params = type_params;
    }

    pub(crate) fn type_params(&self) -> &[(EcoString, Vec<Type>)] {
        &self.type_params
    }

    pub(crate) fn declare_type_param(&mut self, go_name: &str) {
        self.current_declarations_mut()
            .insert(go_name.to_string(), DeclarationKind::TypeParameter);
    }

    /// Bind `lisette_name` to `go_name`, read by identifiers resolved to any of `ids`.
    pub(crate) fn bind_source(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        go_name: impl Into<String>,
    ) -> GoIdentifier {
        let go_name = crate::escape_reserved(&go_name.into()).into_owned();
        let id = self
            .generated_local_id(&go_name)
            .unwrap_or_else(|| self.new_local_id());
        let identifier = GoIdentifier::local(go_name, id);
        self.set_binding(
            lisette_name.into(),
            ids,
            BindingValue::GoName(identifier.clone()),
        );
        identifier
    }

    pub(crate) fn new_local_id(&mut self) -> LocalId {
        let id = LocalId(self.next_local_id);
        self.next_local_id += 1;
        id
    }

    pub(crate) fn set_component_binding(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        mut components: ComponentBinding,
    ) {
        self.identify_binding_name(&mut components.value);
        self.identify_binding_name(&mut components.status);
        self.set_binding(
            lisette_name.into(),
            ids,
            BindingValue::Components(components),
        );
    }

    pub(crate) fn set_tuple_binding(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        mut tuple: TupleBinding,
    ) {
        for name in &mut tuple.names {
            self.identify_binding_name(name);
        }
        self.set_binding(
            lisette_name.into(),
            ids,
            BindingValue::TupleComponents(tuple),
        );
    }

    fn identify_binding_name(&mut self, name: &mut GoIdentifier) {
        if name.spelling() == "_" {
            return;
        }
        let id = self
            .generated_local_id(name.spelling())
            .unwrap_or_else(|| self.new_local_id());
        name.identify(id);
    }

    pub(crate) fn bind_inline_expr(
        &mut self,
        lisette_name: impl Into<String>,
        ids: &[BindingId],
        expr: InlineExpr,
    ) {
        self.set_binding(lisette_name.into(), ids, BindingValue::InlineExpr(expr));
    }

    pub(crate) fn mark_go_const(&mut self, lisette_name: &str) {
        let Some(BindingValue::GoName(name)) =
            self.resolve_identifier_binding(lisette_name).cloned()
        else {
            return;
        };
        if let Some(slot) = self.current_frame().bindings.get(lisette_name).copied() {
            self.current_frame_mut().binding_values[slot] = BindingValue::GoConst(name);
        } else {
            self.set_binding(lisette_name.to_string(), &[], BindingValue::GoConst(name));
        }
    }

    pub(crate) fn resolve_identifier_binding(&self, lisette_name: &str) -> Option<&BindingValue> {
        self.frames.iter().rev().find_map(|frame| {
            frame
                .bindings
                .get(lisette_name)
                .and_then(|slot| frame.binding_values.get(*slot))
        })
    }

    pub(crate) fn bound_go_identifier(&self, lisette_name: &str) -> Option<&GoIdentifier> {
        match self.resolve_identifier_binding(lisette_name) {
            Some(BindingValue::GoName(name) | BindingValue::GoConst(name)) => Some(name),
            _ => None,
        }
    }

    pub(crate) fn resolve_binding_id(&self, id: BindingId) -> Option<&BindingValue> {
        self.frames.iter().rev().find_map(|frame| {
            frame
                .binding_ids
                .get(&id)
                .and_then(|slot| frame.binding_values.get(*slot))
        })
    }

    /// `None` when the innermost local with this name is generated.
    pub(crate) fn source_binding_for_go_name(&self, go_name: &str) -> Option<BindingId> {
        for frame in self.frames.iter().rev() {
            for (id, slot) in &frame.binding_ids {
                if let Some(BindingValue::GoName(name)) = frame.binding_values.get(*slot)
                    && name.spelling() == go_name
                {
                    return Some(*id);
                }
            }
            if frame
                .binding_values
                .iter()
                .any(|value| value.local_named(go_name).is_some())
            {
                return None;
            }
        }
        None
    }

    /// The slot an identifier reads: source bindings by ID, others by spelling.
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

    pub(crate) fn resolve_binding_go_name(&self, lisette_name: &str) -> Option<&str> {
        self.resolve_identifier_binding(lisette_name)
            .and_then(BindingValue::as_go_name)
    }

    pub(crate) fn identifier_for_go_name(&self, go_name: String) -> GoIdentifier {
        if let Some(id) = self.generated_local_id(&go_name) {
            return GoIdentifier::local(go_name, id);
        }
        for frame in self.frames.iter().rev() {
            let mut found: Option<GoIdentifier> = None;
            for value in frame
                .bindings
                .values()
                .filter_map(|slot| frame.binding_values.get(*slot))
            {
                let Some(name) = value.local_named(&go_name) else {
                    continue;
                };
                if found
                    .as_ref()
                    .is_some_and(|previous| !previous.refers_to_same(name))
                {
                    return GoIdentifier::name(go_name);
                }
                found = Some(name.clone());
            }
            if let Some(name) = found {
                return name;
            }
        }
        GoIdentifier::name(go_name)
    }

    pub(crate) fn has_binding_for_go_name(&self, go_name: &str) -> bool {
        self.frames.iter().any(|frame| {
            frame.bindings.iter().any(|(source_name, slot)| {
                let value = &frame.binding_values[*slot];
                value.mentions(go_name)
                    && self
                        .resolve_identifier_binding(source_name)
                        .is_some_and(|visible| visible.mentions(go_name))
            })
        })
    }

    pub(crate) fn has_other_binding_for_go_name(&self, go_name: &str, lisette_name: &str) -> bool {
        self.frames.iter().any(|frame| {
            frame.bindings.iter().any(|(source_name, slot)| {
                let value = &frame.binding_values[*slot];
                source_name != lisette_name
                    && value.mentions(go_name)
                    && self
                        .resolve_identifier_binding(source_name)
                        .is_some_and(|visible| visible.mentions(go_name))
            })
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
        self.current_declarations_mut()
            .insert(go_name.to_string(), DeclarationKind::Local);
    }

    pub(crate) fn try_declare_go_name(&mut self, go_name: &str) -> bool {
        let current = self.current_declarations_mut();
        if current.contains_key(go_name) {
            false
        } else {
            current.insert(go_name.to_string(), DeclarationKind::Local);
            true
        }
    }

    pub(crate) fn current_block_declares(&self, go_name: &str) -> bool {
        self.current_declarations().contains_key(go_name)
    }

    pub(crate) fn is_go_name_declared(&self, go_name: &str) -> bool {
        for frame in self.frames.iter().rev() {
            match &frame.declarations {
                DeclarationScope::Transparent => {}
                DeclarationScope::Block(names) if names.contains_key(go_name) => return true,
                DeclarationScope::Block(_) => {}
                DeclarationScope::Function { declarations, .. } => {
                    return declarations.contains_key(go_name);
                }
            }
        }
        false
    }

    pub(crate) fn current_block_declared_nonempty(&self) -> bool {
        !self.current_declarations().is_empty()
    }

    pub(crate) fn enter_block(&mut self) {
        self.push_frame(DeclarationScope::Block(HashMap::default()));
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

    /// Record that the block being lowered runs only when `condition` holds.
    pub(crate) fn establish_condition(&mut self, mut condition: GoExpression) {
        condition.identify_names(&|name| self.identifier_for_go_name(name.to_string()).id());
        self.frames
            .iter_mut()
            .rev()
            .find(|frame| !matches!(frame.declarations, DeclarationScope::Transparent))
            .expect("scope state always retains a declaration scope")
            .established
            .push(condition);
    }

    pub(crate) fn is_condition_established(&self, condition: &GoExpression) -> bool {
        let mut condition = condition.clone();
        condition.identify_names(&|name| self.identifier_for_go_name(name.to_string()).id());
        for frame in self.frames.iter().rev() {
            if frame
                .established
                .iter()
                .any(|established| established.node() == condition.node())
            {
                return true;
            }
            if matches!(frame.declarations, DeclarationScope::Function { .. }) {
                return false;
            }
        }
        false
    }

    pub(crate) fn enter_isolated_function(&mut self, return_ctx: ReturnContext) {
        self.push_frame(DeclarationScope::Function {
            declarations: self.visible_type_params(),
            return_ctx,
            test_handle: None,
        });
    }

    pub(crate) fn exit_isolated_function(&mut self) {
        assert!(
            matches!(
                self.current_frame().declarations,
                DeclarationScope::Function { .. }
            ),
            "an isolated function must be entered before it is exited"
        );
        self.pop_frame();
    }

    pub(crate) fn push_loop(&mut self, result: GoExpression) {
        let id = LoopId(self.next_loop_id);
        self.next_loop_id += 1;
        self.loop_stack.push(LoopContext { id, result });
    }

    pub(crate) fn pop_loop(&mut self) {
        self.loop_stack
            .pop()
            .expect("a loop context must be pushed before it is popped");
    }

    pub(crate) fn set_test_handle(&mut self, handle: GoIdentifier) {
        let slot = self
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| match &mut frame.declarations {
                DeclarationScope::Function { test_handle, .. } => Some(test_handle),
                _ => None,
            })
            .expect("a test handle is a parameter of an isolated function");
        *slot = Some(handle);
    }

    /// The test handle of the nearest enclosing function that has one.
    pub(crate) fn current_test_handle(&self) -> Option<&GoIdentifier> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Function { test_handle, .. } => test_handle.as_ref(),
                _ => None,
            })
    }

    pub(crate) fn current_return_ctx(&self) -> ReturnContext {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Function { return_ctx, .. } => Some(return_ctx.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(crate) fn current_loop_result(&self) -> Option<&GoExpression> {
        self.loop_stack.last().map(|context| &context.result)
    }

    pub(crate) fn current_loop_id(&self) -> Option<LoopId> {
        self.loop_stack.last().map(|context| context.id)
    }

    /// Mark `target` as written later in the region, returning whether it became active.
    pub(crate) fn activate_assign_target(&mut self, target: &GoExpression) -> bool {
        match target.node() {
            GoExpressionNode::Identifier(name) if name.spelling() != "_" => {
                self.assign_targets.insert(name.to_string())
            }
            _ => false,
        }
    }

    pub(crate) fn deactivate_assign_target(&mut self, target: &GoExpression) {
        if let GoExpressionNode::Identifier(name) = target.node() {
            self.assign_targets.remove(name.spelling());
        }
    }

    pub(crate) fn is_active_assign_target(&self, var: &str) -> bool {
        self.assign_targets.contains(var)
    }

    fn push_frame(&mut self, declarations: DeclarationScope) {
        self.frames.push(ScopeFrame {
            bindings: HashMap::default(),
            binding_ids: HashMap::default(),
            binding_values: Vec::new(),
            declarations,
            established: Vec::new(),
        });
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

    fn set_binding(&mut self, name: String, ids: &[BindingId], value: BindingValue) {
        let frame = self.current_frame_mut();
        let slot = frame.binding_values.len();
        frame.binding_values.push(value);
        frame.bindings.insert(name, slot);
        for id in ids {
            frame.binding_ids.insert(*id, slot);
        }
    }

    fn current_declarations(&self) -> &Declarations {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| match &frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(names)
                | DeclarationScope::Function {
                    declarations: names,
                    ..
                } => Some(names),
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
                | DeclarationScope::Function {
                    declarations: names,
                    ..
                } => Some(names),
            })
            .expect("scope state always retains a declaration scope")
    }

    fn visible_type_params(&self) -> Declarations {
        self.frames
            .iter()
            .filter_map(|frame| match &frame.declarations {
                DeclarationScope::Transparent => None,
                DeclarationScope::Block(declarations)
                | DeclarationScope::Function { declarations, .. } => Some(declarations),
            })
            .flat_map(|declarations| declarations.iter())
            .filter(|(_, kind)| **kind == DeclarationKind::TypeParameter)
            .map(|(name, kind)| (name.clone(), *kind))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::go_expression::BinaryOp;
    use crate::plan::values::{GoExpression, Stability};
    use syntax::types::SubstitutionMap;

    fn pair_first() -> GoExpression {
        GoExpression::selector(GoExpression::name("pair".to_string()), "F0".to_string())
    }

    #[test]
    fn exiting_block_restores_shadowed_binding() {
        let mut scope = ScopeState::new();
        scope.bind_source("value", &[], "outer");
        scope.enter_block();
        scope.bind_source("value", &[], "inner");

        scope.exit_block();

        assert_eq!(scope.resolve_binding_go_name("value"), Some("outer"));
    }

    #[test]
    fn binding_ids_keep_outer_and_inner_values_distinct() {
        let mut scope = ScopeState::new();
        let outer = BindingId::new(1);
        let inner = BindingId::new(2);
        scope.bind_source("value", &[outer], "outer");
        scope.enter_block();
        scope.bind_source("value", &[inner], "inner");

        assert_eq!(
            scope
                .resolve_binding_id(outer)
                .and_then(BindingValue::as_go_name),
            Some("outer")
        );
        assert_eq!(
            scope
                .resolve_binding_id(inner)
                .and_then(BindingValue::as_go_name),
            Some("inner")
        );
        scope.exit_block();
        assert!(scope.resolve_binding_id(inner).is_none());
        assert_eq!(
            scope
                .resolve_binding_id(outer)
                .and_then(BindingValue::as_go_name),
            Some("outer")
        );
    }

    #[test]
    fn source_rebind_moves_its_ids_and_generated_bind_does_not_take_them() {
        let mut scope = ScopeState::new();
        let id = BindingId::new(5);
        scope.bind_source("value", &[id], "value");
        scope.bind_source("value", &[id], "value_2");
        scope.bind_source("value", &[], "generated");

        assert_eq!(
            scope
                .resolve_binding_id(id)
                .and_then(BindingValue::as_go_name),
            Some("value_2")
        );
        assert_eq!(scope.resolve_binding_go_name("value"), Some("generated"));
    }

    #[test]
    fn binding_ids_keep_same_frame_rebindings_distinct() {
        let mut scope = ScopeState::new();
        let old_id = BindingId::new(3);
        let new_id = BindingId::new(4);
        scope.bind_source("value", &[old_id], "original");
        scope.bind_source("value", &[new_id], "replacement");

        assert_eq!(
            scope
                .resolve_binding_id(old_id)
                .and_then(BindingValue::as_go_name),
            Some("original")
        );
        scope.mark_go_const("value");
        assert!(
            !scope
                .resolve_binding_id(old_id)
                .is_some_and(BindingValue::is_go_const)
        );
        assert!(
            scope
                .resolve_binding_id(new_id)
                .is_some_and(BindingValue::is_go_const)
        );
    }

    #[test]
    fn source_identifier_never_uses_a_same_spelled_binding() {
        let mut scope = ScopeState::new();
        scope.bind_source("value", &[], "other");
        let missing = IdentifierResolution::Binding(BindingId::new(7));
        assert!(
            scope
                .resolve_identifier_with_resolution("value", &missing)
                .is_none()
        );
        // A local const binds no ID, so a definition reads it by spelling.
        let definition = IdentifierResolution::Definition {
            name: "package.value".into(),
            instantiation: SubstitutionMap::default(),
        };
        assert!(
            scope
                .resolve_identifier_with_resolution("value", &definition)
                .is_some()
        );
        assert!(
            scope
                .resolve_identifier_with_resolution("value", &IdentifierResolution::Unresolved)
                .is_some()
        );
    }

    #[test]
    fn go_name_reference_uses_the_visible_local_id() {
        let mut scope = ScopeState::new();
        scope.bind_source("outer", &[], "value");
        let outer = scope.identifier_for_go_name("value".to_string());
        scope.enter_block();
        scope.bind_source("inner", &[], "value");
        let inner = scope.identifier_for_go_name("value".to_string());
        assert_ne!(outer.id(), inner.id());
        scope.exit_block();
        assert_eq!(
            scope.identifier_for_go_name("value".to_string()).id(),
            outer.id()
        );
    }

    #[test]
    fn established_conditions_keep_the_binding_seen_when_recorded() {
        let mut scope = ScopeState::new();
        scope.bind_source("value", &[], "value");
        let condition = || {
            GoExpression::binary(
                GoExpression::name("value".to_string()),
                BinaryOp::Eq,
                GoExpression::literal("1".to_string()),
            )
        };
        scope.establish_condition(condition());
        assert!(scope.is_condition_established(&condition()));

        scope.enter_block();
        scope.bind_source("value", &[], "value");
        assert!(!scope.is_condition_established(&condition()));
        scope.exit_block();

        assert!(scope.is_condition_established(&condition()));
    }

    #[test]
    fn nested_bindings_restore_names_constants_and_inline_expressions() {
        let mut scope = ScopeState::new();
        scope.bind_source("value", &[], "outer");
        scope.mark_go_const("value");
        scope.push_binding_frame();
        scope.bind_inline_expr(
            "value",
            &[],
            InlineExpr::new(pair_first(), Stability::Fixed),
        );
        scope.enter_block();
        scope.bind_source("value", &[], "inner");
        scope.bind_source("value", &[], "rebound");
        scope.exit_block();
        assert!(matches!(
            scope.resolve_identifier_binding("value"),
            Some(BindingValue::InlineExpr(expr)) if expr.expression().rendered() == "pair.F0"
        ));
        scope.pop_binding_frame();
        assert!(matches!(
            scope.resolve_identifier_binding("value"),
            Some(BindingValue::GoConst(name)) if name == "outer"
        ));
    }

    #[test]
    fn binding_frames_restore_all_bindings_but_keep_go_declarations() {
        let mut scope = ScopeState::new();
        scope.bind_source("value", &[], "outer");
        scope.push_binding_frame();
        scope.bind_source("value", &[], "inner");
        scope.bind_source("new", &[], "local");
        scope.declare_go_name("local");
        scope.pop_binding_frame();

        assert_eq!(scope.resolve_binding_go_name("value"), Some("outer"));
        assert!(scope.resolve_identifier_binding("new").is_none());
        assert!(!scope.has_binding_for_go_name("inner"));
        assert!(scope.is_go_name_declared("local"));
    }

    #[test]
    fn bound_go_names_follow_visible_bindings_including_aliases() {
        let mut scope = ScopeState::new();
        scope.bind_source("first", &[], "shared");
        scope.bind_source("second", &[], "shared");
        scope.enter_block();
        scope.bind_source("first", &[], "inner");
        assert!(scope.has_binding_for_go_name("shared"));
        scope.bind_inline_expr(
            "second",
            &[],
            InlineExpr::new(pair_first(), Stability::Fixed),
        );
        assert!(!scope.has_binding_for_go_name("shared"));
        scope.exit_block();
        assert!(scope.has_binding_for_go_name("shared"));
        assert!(!scope.has_binding_for_go_name("inner"));
    }
}
