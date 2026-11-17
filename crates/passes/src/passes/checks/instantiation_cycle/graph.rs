pub(super) fn components(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for root in 0..edges.len() {
        let mut pending = vec![(root, false)];
        while let Some((node, finished)) = pending.pop() {
            if finished {
                order.push(node);
            } else if !visited[node] {
                visited[node] = true;
                pending.push((node, true));
                pending.extend(edges[node].iter().map(|&next| (next, false)));
            }
        }
    }
    let mut reverse = vec![Vec::new(); edges.len()];
    for (from, targets) in edges.iter().enumerate() {
        for &to in targets {
            reverse[to].push(from);
        }
    }
    let mut components = vec![usize::MAX; edges.len()];
    for root in order.into_iter().rev() {
        if components[root] != usize::MAX {
            continue;
        }
        components[root] = root;
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            for &next in &reverse[node] {
                if components[next] == usize::MAX {
                    components[next] = root;
                    pending.push(next);
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    #[test]
    fn distinguishes_cycles_from_one_way_dependencies() {
        let groups = super::components(&[vec![1], vec![0, 2], vec![3], vec![2, 4], vec![]]);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(groups[2], groups[3]);
        assert_ne!(groups[1], groups[2]);
        assert_ne!(groups[3], groups[4]);
    }

    #[test]
    fn long_chain_does_not_use_the_call_stack() {
        let mut edges: Vec<_> = (1..20_000).map(|next| vec![next]).collect();
        edges.push(vec![]);
        let groups = super::components(&edges);
        assert!(groups.windows(2).all(|pair| pair[0] != pair[1]));
        edges.last_mut().unwrap().push(0);
        let groups = super::components(&edges);
        assert!(groups.iter().all(|&group| group == groups[0]));
    }
}
