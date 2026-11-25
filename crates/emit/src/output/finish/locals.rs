use crate::lookup::state::scope::ScopeState;
use crate::lookup::state::scope_stack::ScopeStack;
use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::visit::{VisitorMut, visit_statements_mut};
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

pub(crate) fn identify_body_locals(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
    scope: &mut ScopeState,
) -> FxHashSet<LocalId> {
    struct Resolve<'a> {
        scope: &'a mut ScopeState,
        frames: ScopeStack<FxHashMap<String, LocalId>>,
        shadowing: FxHashSet<LocalId>,
    }

    impl VisitorMut for Resolve<'_> {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            let GoExpressionNode::Identifier(name) = node else {
                return;
            };
            if let Some(id) = name.id() {
                assert!(
                    self.frames
                        .iter()
                        .any(|frame| frame.values().any(|local| *local == id)),
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

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if name.spelling() == "_" {
                name.finish();
                return;
            }
            let current = self.frames.innermost().get(name.spelling()).copied();

            let id =
                current.unwrap_or_else(|| name.id().unwrap_or_else(|| self.scope.new_local_id()));
            name.identify(id);
            if self
                .frames
                .enclosing()
                .any(|frame| frame.get(name.spelling()).is_some_and(|outer| *outer != id))
            {
                self.shadowing.insert(id);
            }
            self.frames
                .innermost_mut()
                .insert(name.spelling().to_string(), id);
        }

        fn enter_scope(&mut self) {
            self.frames.push(FxHashMap::default());
        }

        fn exit_scope(&mut self) {
            self.frames.pop();
        }
    }

    let mut resolver = Resolve {
        scope,
        frames: ScopeStack::new(FxHashMap::default()),
        shadowing: FxHashSet::default(),
    };
    for name in parameters {
        let mut name = name.clone();
        resolver.local_binding(&mut name);
    }
    visit_statements_mut(statements, &mut resolver);
    resolver.shadowing
}
