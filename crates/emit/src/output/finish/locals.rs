use crate::lookup::state::scope::ScopeState;
use crate::lookup::state::scope_stack::ScopeStack;
use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

struct LocalResolver<'a> {
    scope: &'a mut ScopeState,
    frames: ScopeStack<FxHashMap<LocalId, String>>,
    shadowing: FxHashSet<LocalId>,
}

impl VisitorMut for LocalResolver<'_> {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        let GoExpressionNode::Identifier(name) = node else {
            return;
        };
        if let Some(id) = name.id() {
            assert!(
                self.frames.iter().any(|frame| frame.contains_key(&id)),
                "identified read of `{}` has no visible declaration",
                name.spelling()
            );
            return;
        }
        assert!(
            !name.is_pending(),
            "read has no producer-assigned identity: {}",
            name.spelling()
        );
    }

    fn local_binding(&mut self, name: &mut GoIdentifier, _site: BindingSite) {
        if name.spelling() == "_" {
            name.finish();
            return;
        }
        let id = name
            .id()
            .or_else(|| {
                self.frames
                    .innermost()
                    .iter()
                    .find_map(|(id, spelling)| (spelling == name.spelling()).then_some(*id))
            })
            .unwrap_or_else(|| self.scope.new_local_id());
        name.identify(id);
        if self.frames.enclosing().any(|frame| {
            frame
                .iter()
                .any(|(outer, spelling)| *outer != id && spelling == name.spelling())
        }) {
            self.shadowing.insert(id);
        }
        self.frames
            .innermost_mut()
            .insert(id, name.spelling().to_string());
    }

    fn enter_scope(&mut self) {
        self.frames.push(FxHashMap::default());
    }

    fn exit_scope(&mut self) {
        self.frames.pop();
    }
}

pub(crate) fn identify_body_locals(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
    scope: &mut ScopeState,
) -> FxHashSet<LocalId> {
    let mut resolver = LocalResolver {
        scope,
        frames: ScopeStack::new(FxHashMap::default()),
        shadowing: FxHashSet::default(),
    };
    for name in parameters {
        let mut name = name.clone();
        resolver.local_binding(&mut name, BindingSite::Parameter);
    }
    visit_statements_mut(statements, &mut resolver);
    resolver.shadowing
}
