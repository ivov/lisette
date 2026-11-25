use rustc_hash::FxHashMap;

use crate::target::ir::bodies::Statement;
use crate::target::ir::go_expression::{GoExpressionNode, verbatim_identifiers};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};

#[derive(Default)]
pub(crate) struct NameUses {
    reads: LocalCounts,
    opaque_reads: FxHashMap<String, usize>,
    assigned: LocalCounts,
    writes: LocalCounts,
    bindings: LocalCounts,
}

#[derive(Default)]
struct LocalCounts {
    by_id: FxHashMap<LocalId, usize>,
    by_spelling: FxHashMap<String, usize>,
}

impl LocalCounts {
    fn add(&mut self, name: &GoIdentifier) {
        *self
            .by_spelling
            .entry(name.spelling().to_string())
            .or_default() += 1;
        if let Some(id) = name.id() {
            *self.by_id.entry(id).or_default() += 1;
        }
    }

    fn get(&self, name: &GoIdentifier) -> usize {
        match name.id() {
            Some(id) => self.by_id.get(&id).copied().unwrap_or_default(),
            None => self
                .by_spelling
                .get(name.spelling())
                .copied()
                .unwrap_or_default(),
        }
    }
}

impl NameUses {
    pub(crate) fn of(statements: &mut Vec<Statement>) -> Self {
        let mut uses = Self::default();
        visit_statements_mut(statements, &mut uses);
        uses
    }

    pub(super) fn reads(&self, name: &GoIdentifier) -> usize {
        self.reads.get(name)
            + self
                .opaque_reads
                .get(name.spelling())
                .copied()
                .unwrap_or_default()
    }

    pub(crate) fn references(&self, name: &GoIdentifier) -> usize {
        self.reads(name)
    }

    pub(crate) fn bindings(&self, name: &GoIdentifier) -> usize {
        self.bindings.get(name)
    }

    pub(crate) fn value_reads(&self, name: &GoIdentifier) -> usize {
        self.reads(name).saturating_sub(self.assigned.get(name))
    }

    pub(crate) fn is_written(&self, name: &GoIdentifier) -> bool {
        self.writes.get(name) > 0
    }
}

impl VisitorMut for NameUses {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        match node {
            GoExpressionNode::Identifier(name) => self.reads.add(name),
            GoExpressionNode::Verbatim(text) => {
                for token in verbatim_identifiers(text) {
                    *self.opaque_reads.entry(token.to_string()).or_default() += 1;
                }
            }
            _ => {}
        }
    }

    fn local_binding(&mut self, name: &mut GoIdentifier, _site: BindingSite) {
        self.bindings.add(name);
    }

    fn assignment_target(&mut self, target: &GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = target {
            self.assigned.add(name);
        }
        target.visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                self.writes.add(name);
            }
        });
    }
}
