use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;
use syntax::ast::{Expression, Span};

use crate::checker::TaskState;
use crate::store::Store;

struct ConstEntry<'a> {
    name: &'a EcoString,
    name_span: Span,
    body: &'a Expression,
}

impl TaskState<'_> {
    pub fn check_const_cycles(&mut self, store: &Store, items_per_file: &[&[Expression]]) {
        let module_const_names_empty = store
            .get_module(&self.cursor.module_id)
            .is_some_and(|m| m.const_names.is_empty());
        if module_const_names_empty {
            return;
        }

        let mut consts: Vec<ConstEntry<'_>> = Vec::new();
        for items in items_per_file {
            for item in *items {
                if let Expression::Const {
                    identifier,
                    identifier_span,
                    expression,
                    ..
                } = item
                {
                    consts.push(ConstEntry {
                        name: identifier,
                        name_span: *identifier_span,
                        body: expression,
                    });
                }
            }
        }

        if consts.is_empty() {
            return;
        }

        let const_names: HashSet<&EcoString> = consts.iter().map(|c| c.name).collect();

        let mut deps: HashMap<&EcoString, Vec<&EcoString>> = HashMap::default();
        let mut spans: HashMap<&EcoString, Span> = HashMap::default();
        for entry in &consts {
            let mut refs: Vec<&EcoString> = Vec::new();
            collect_const_refs(entry.body, &const_names, &mut refs);
            refs.sort();
            refs.dedup();
            deps.insert(entry.name, refs);
            spans.insert(entry.name, entry.name_span);
        }

        let mut color: HashMap<&EcoString, Color> = HashMap::default();
        for entry in &consts {
            color.insert(entry.name, Color::White);
        }

        let mut reported: HashSet<&EcoString> = HashSet::default();
        for entry in &consts {
            if color[&entry.name] == Color::White {
                let mut path: Vec<&EcoString> = Vec::new();
                dfs(
                    entry.name,
                    &deps,
                    &spans,
                    &mut color,
                    &mut path,
                    &mut reported,
                    self.sink,
                );
            }
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Color {
    White,
    Gray,
    Black,
}

fn dfs<'a>(
    node: &'a EcoString,
    deps: &HashMap<&'a EcoString, Vec<&'a EcoString>>,
    spans: &HashMap<&'a EcoString, Span>,
    color: &mut HashMap<&'a EcoString, Color>,
    path: &mut Vec<&'a EcoString>,
    reported: &mut HashSet<&'a EcoString>,
    sink: &diagnostics::LocalSink,
) {
    color.insert(node, Color::Gray);
    path.push(node);

    if let Some(neighbors) = deps.get(node) {
        for next in neighbors {
            match color.get(next).copied().unwrap_or(Color::White) {
                Color::White => dfs(next, deps, spans, color, path, reported, sink),
                Color::Gray => {
                    let start = path.iter().position(|n| *n == *next).unwrap_or(0);
                    let cycle: Vec<String> = path[start..].iter().map(|n| n.to_string()).collect();
                    let representative = path[start];
                    if reported.insert(representative)
                        && let Some(span) = spans.get(representative)
                    {
                        sink.push(diagnostics::infer::const_cycle(&cycle, *span));
                    }
                }
                Color::Black => {}
            }
        }
    }

    path.pop();
    color.insert(node, Color::Black);
}

fn collect_const_refs<'a>(
    expression: &'a Expression,
    const_names: &HashSet<&'a EcoString>,
    out: &mut Vec<&'a EcoString>,
) {
    if let Expression::Identifier {
        value, qualified, ..
    } = expression
    {
        if qualified.is_none()
            && let Some(&name) = const_names.get(value)
        {
            out.push(name);
        }
        return;
    }
    for child in expression.children() {
        collect_const_refs(child, const_names, out);
    }
}
