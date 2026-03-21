use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::ModuleId;

pub fn topological_sort(
    edges: &HashMap<ModuleId, HashSet<ModuleId>>,
) -> (Vec<ModuleId>, Vec<Vec<ModuleId>>) {
    let mut in_degree: HashMap<ModuleId, usize> = HashMap::default();
    let mut order = Vec::new();

    for (module, imports) in edges {
        in_degree.entry(module.clone()).or_insert(0);
        for import in imports {
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

        if let Some(imports) = edges.get(&module) {
            for import in imports {
                if let Some(degree) = in_degree.get_mut(import) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push(import.clone());
                        queue.sort();
                    }
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

fn find_cycles(
    edges: &HashMap<ModuleId, HashSet<ModuleId>>,
    processed: &[ModuleId],
) -> Vec<Vec<ModuleId>> {
    let processed_set: HashSet<_> = processed.iter().collect();
    let unprocessed: Vec<_> = edges
        .keys()
        .filter(|k| !processed_set.contains(k))
        .collect();

    let mut cycles = Vec::new();
    let mut visited = HashSet::default();

    for start in unprocessed {
        if visited.contains(start) {
            continue;
        }

        let mut stack = vec![(start, vec![start.clone()])];
        let mut on_stack: HashSet<ModuleId> = HashSet::default();

        while let Some((node, path)) = stack.pop() {
            if on_stack.contains(node) {
                continue;
            }
            on_stack.insert(node.clone());
            visited.insert(node.clone());

            if let Some(imports) = edges.get(node) {
                for import in imports {
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
    }

    cycles
}
