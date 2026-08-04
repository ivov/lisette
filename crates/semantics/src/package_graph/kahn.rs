use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::Span;

use super::{DependencyGraph, PackageId};

#[derive(Debug, Clone)]
pub struct CycleHop {
    pub package: PackageId,
    pub span: Span,
}

pub type Cycle = Vec<CycleHop>;

pub fn topological_sort(edges: &DependencyGraph) -> (Vec<PackageId>, Vec<Cycle>) {
    let mut in_degree: HashMap<PackageId, usize> = HashMap::default();
    let mut order = Vec::new();

    for package in edges.packages() {
        in_degree.entry(package.clone()).or_insert(0);
        for import in edges.dependencies(package) {
            *in_degree.entry(import.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<_> = in_degree
        .iter()
        .filter(|&(_, deg)| *deg == 0)
        .map(|(id, _)| id.clone())
        .collect();

    queue.sort();

    while let Some(package) = queue.pop() {
        order.push(package.clone());

        for import in edges.dependencies(&package) {
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

fn find_cycles(edges: &DependencyGraph, processed: &[PackageId]) -> Vec<Cycle> {
    let processed_set: HashSet<_> = processed.iter().collect();
    let unprocessed: Vec<_> = edges
        .packages()
        .filter(|k| !processed_set.contains(k))
        .collect();

    let mut cycles = Vec::new();
    let mut visited = HashSet::default();

    for start in unprocessed {
        if visited.contains(start) {
            continue;
        }

        let mut stack: Vec<(&PackageId, Cycle)> = vec![(start, Vec::new())];

        while let Some((node, path)) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }

            for (import, span) in edges.imports(node) {
                let hop = CycleHop {
                    package: node.clone(),
                    span,
                };
                let closes_on = path
                    .iter()
                    .chain([&hop])
                    .position(|walked| &walked.package == import);
                if let Some(position) = closes_on {
                    let mut cycle: Cycle = path[position..].to_vec();
                    cycle.push(hop);
                    rotate_to_first_package(&mut cycle);
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
fn rotate_to_first_package(cycle: &mut Cycle) {
    if let Some((position, _)) = cycle
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.package.cmp(&right.package))
    {
        cycle.rotate_left(position);
    }
}
