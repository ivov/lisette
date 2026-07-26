use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use ecow::EcoString;
use syntax::ast::{Expression, IdentifierResolution, Span};

use crate::checker::infer::InferCtx;

struct ConstEntry<'a> {
    name: &'a EcoString,
    name_span: Span,
    body: &'a Expression,
}

struct ConstNode<'a> {
    dependencies: Vec<&'a EcoString>,
    span: Span,
}

impl InferCtx<'_> {
    pub fn check_const_cycles(&mut self, items_per_file: &[&[Expression]]) {
        let mut consts: Vec<ConstEntry<'_>> = Vec::new();
        for items in items_per_file {
            for item in *items {
                if let Expression::Const {
                    identifier,
                    identifier_span,
                    expression,
                    ..
                } = item
                    && let Some(expression) = expression.value()
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

        let mut nodes: HashMap<&EcoString, ConstNode<'_>> = HashMap::default();
        for entry in &consts {
            let mut refs: Vec<&EcoString> = Vec::new();
            collect_const_refs(entry.body, &const_names, &mut refs);
            refs.sort();
            refs.dedup();
            nodes.insert(
                entry.name,
                ConstNode {
                    dependencies: refs,
                    span: entry.name_span,
                },
            );
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
                    &nodes,
                    &mut color,
                    &mut path,
                    &mut reported,
                    &self.sink,
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
    nodes: &HashMap<&'a EcoString, ConstNode<'a>>,
    color: &mut HashMap<&'a EcoString, Color>,
    path: &mut Vec<&'a EcoString>,
    reported: &mut HashSet<&'a EcoString>,
    sink: &diagnostics::LocalSink,
) {
    color.insert(node, Color::Gray);
    path.push(node);

    if let Some(current) = nodes.get(node) {
        for next in &current.dependencies {
            match color.get(next).copied().unwrap_or(Color::White) {
                Color::White => dfs(next, nodes, color, path, reported, sink),
                Color::Gray => {
                    let start = path.iter().position(|n| *n == *next).unwrap_or(0);
                    let cycle: Vec<String> = path[start..].iter().map(|n| n.to_string()).collect();
                    let representative = path[start];
                    if reported.insert(representative)
                        && let Some(representative) = nodes.get(representative)
                    {
                        sink.push(diagnostics::infer::const_cycle(&cycle, representative.span));
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
        value,
        resolution: IdentifierResolution::Unresolved,
        ..
    } = expression
    {
        if let Some(&name) = const_names.get(value) {
            out.push(name);
        }
        return;
    }
    for child in expression.children() {
        collect_const_refs(child, const_names, out);
    }
}
