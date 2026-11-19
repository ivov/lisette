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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    const STOP_MACROS: [&str; 4] = ["panic!(", "unreachable!(", "assert!(", "assert_eq!("];

    fn count_stop_macros(dir: &Path) -> usize {
        let mut count = 0;
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_stop_macros(&path);
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let source = fs::read_to_string(&path).unwrap();
            for line in source
                .lines()
                .take_while(|line| line.trim() != "#[cfg(test)]")
            {
                let line = line.trim_start();
                if line.starts_with("//") {
                    continue;
                }
                count += STOP_MACROS
                    .iter()
                    .map(|name| {
                        line.match_indices(name)
                            .filter(|(at, _)| !line[..*at].ends_with('_'))
                            .count()
                    })
                    .sum::<usize>();
            }
        }
        count
    }

    #[test]
    fn emit_stop_macro_count_is_pinned() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert_eq!(
            count_stop_macros(&src),
            PINNED,
            "the count of panic!/unreachable!/assert!/assert_eq! in emit changed. \
             If a user program can reach the new site, call `rejections().reject` instead, \
             then set PINNED to the new count"
        );
    }

    const PINNED: usize = 100;
}
