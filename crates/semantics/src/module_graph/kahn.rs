use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Span;

use super::{DependencyGraph, ModuleId};

#[derive(Debug, Clone)]
pub struct CycleHop {
    pub module: ModuleId,
    pub span: Span,
}

pub type Cycle = Vec<CycleHop>;

pub fn topological_sort(edges: &DependencyGraph) -> (Vec<ModuleId>, Vec<Cycle>) {
    let mut in_degree: HashMap<ModuleId, usize> = HashMap::default();
    let mut order = Vec::new();

    for module in edges.modules() {
        in_degree.entry(module.clone()).or_insert(0);
        for import in edges.dependencies(module) {
            *in_degree.entry(import.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<_> = in_degree
        .iter()
        .filter(|&(_, deg)| *deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    queue.sort();

    while let Some(module) = queue.pop() {
        order.push(module.clone());

        for import in edges.dependencies(&module) {
            if let Some(degree) = in_degree.get_mut(import) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push(import.clone());
                    queue.sort();
                }
            }
        }
    }

    let cycles = if order.len() < edges.len() {
        find_cycles(edges, &order)
    } else {
        vec![]
    };

    order.reverse();

    (order, cycles)
}

fn find_cycles(edges: &DependencyGraph, processed: &[ModuleId]) -> Vec<Cycle> {
    let processed_set: HashSet<_> = processed.iter().collect();
    let unprocessed: Vec<_> = edges
        .modules()
        .filter(|k| !processed_set.contains(k))
        .collect();

    let mut cycles = Vec::new();
    let mut visited = HashSet::default();

    for start in unprocessed {
        if visited.contains(start) {
            continue;
        }

        let mut stack: Vec<(&ModuleId, Cycle)> = vec![(start, Vec::new())];

        while let Some((node, path)) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }

            for (import, span) in edges.imports(node) {
                let hop = CycleHop {
                    module: node.clone(),
                    span,
                };
                let closes_on = path
                    .iter()
                    .chain([&hop])
                    .position(|walked| &walked.module == import);
                if let Some(position) = closes_on {
                    let mut cycle: Cycle = path[position..].to_vec();
                    cycle.push(hop);
                    rotate_to_first_module(&mut cycle);
                    cycles.push(cycle);
                } else if !visited.contains(import) {
                    let mut extended = path.clone();
                    extended.push(hop);
                    stack.push((import, extended));
                }
            }
        }
    }

    cycles
}

/// So the reported chain does not depend on where the walk started.
fn rotate_to_first_module(cycle: &mut Cycle) {
    if let Some((position, _)) = cycle
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.module.cmp(&right.module))
    {
        cycle.rotate_left(position);
    }
}
