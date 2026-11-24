use std::cell::{Cell, RefCell};

use diagnostics::LisetteDiagnostic;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::Span;

#[derive(Clone, Default)]
pub(crate) struct Rejections {
    focus: Cell<Option<Span>>,
    found: RefCell<Vec<LisetteDiagnostic>>,
    seen: RefCell<HashSet<(Span, String)>>,
}

impl Rejections {
    pub(crate) fn enter(&self, span: Span) -> Option<Span> {
        self.focus.replace(Some(span))
    }

    pub(crate) fn leave(&self, previous: Option<Span>) {
        self.focus.set(previous);
    }

    pub(crate) fn reject(&self, key: &str, build: impl FnOnce(&Span) -> LisetteDiagnostic) {
        let span = self.focus.get().unwrap_or_else(Span::dummy);
        let mut seen = self.seen.borrow_mut();
        if seen
            .iter()
            .any(|(other, other_key)| other_key == key && overlaps(other, &span))
        {
            return;
        }
        seen.insert((span, key.to_string()));
        self.found.borrow_mut().push(build(&span));
    }

    pub(crate) fn into_diagnostics(self) -> Vec<LisetteDiagnostic> {
        self.found.into_inner()
    }
}

fn overlaps(a: &Span, b: &Span) -> bool {
    let end = |span: &Span| span.byte_offset + span.byte_length;
    a.file_id == b.file_id && a.byte_offset < end(b) && b.byte_offset < end(a)
}
