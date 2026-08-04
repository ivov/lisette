use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::{DependencyGraph, PackageId};

pub fn topological_sort(edges: &DependencyGraph) -> (Vec<PackageId>, Vec<Vec<PackageId>>) {
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

fn find_cycles(edges: &DependencyGraph, processed: &[PackageId]) -> Vec<Vec<PackageId>> {
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

        let mut stack = vec![(start, vec![start.clone()])];

        while let Some((node, path)) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }

            for import in edges.dependencies(node) {
                if let Some(position) = path.iter().position(|p| p == import) {
                    let mut cycle_path: Vec<_> = path[position..].to_vec();
                    cycle_path.push(import.clone());
                    cycles.push(cycle_path);
                } else if !visited.contains(import) {
                    let mut new_path = path.clone();
                    new_path.push(import.clone());
                    stack.push((import, new_path));
                }
            }
        }
    }

    cycles
}
