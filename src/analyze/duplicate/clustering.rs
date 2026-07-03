//! Union-find over similarity pairs → connected components.

pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    pub fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut current = x;
        while self.parent[current] != root {
            let next = self.parent[current];
            self.parent[current] = root;
            current = next;
        }
        root
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let (root_a, root_b) = (self.find(a), self.find(b));
        if root_a == root_b {
            return;
        }
        match self.rank[root_a].cmp(&self.rank[root_b]) {
            std::cmp::Ordering::Less => self.parent[root_a] = root_b,
            std::cmp::Ordering::Greater => self.parent[root_b] = root_a,
            std::cmp::Ordering::Equal => {
                self.parent[root_b] = root_a;
                self.rank[root_a] += 1;
            }
        }
    }
}

/// Group ids into components using the given edges. Components are sorted
/// by their smallest member and each component's members are sorted, so
/// output is deterministic regardless of edge order.
pub fn connected_components(size: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut uf = UnionFind::new(size);
    for &(a, b) in edges {
        uf.union(a, b);
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    let mut in_edges = vec![false; size];
    for &(a, b) in edges {
        in_edges[a] = true;
        in_edges[b] = true;
    }
    for (id, present) in in_edges.iter().enumerate() {
        if *present {
            groups.entry(uf.find(id)).or_default().push(id);
        }
    }
    let mut components: Vec<Vec<usize>> = groups.into_values().collect();
    components.sort_by_key(|c| c[0]);
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitive_chains_merge() {
        // a-b, b-c => one component {a,b,c}
        let components = connected_components(5, &[(0, 1), (1, 2)]);
        assert_eq!(components, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn disconnected_components_stay_apart() {
        let components = connected_components(6, &[(0, 1), (3, 4)]);
        assert_eq!(components, vec![vec![0, 1], vec![3, 4]]);
    }

    #[test]
    fn stable_ordering_regardless_of_edge_order() {
        let a = connected_components(6, &[(4, 3), (1, 0), (0, 2)]);
        let b = connected_components(6, &[(0, 1), (2, 0), (3, 4)]);
        assert_eq!(a, b);
        assert_eq!(a, vec![vec![0, 1, 2], vec![3, 4]]);
    }

    #[test]
    fn isolated_nodes_are_not_components() {
        assert!(connected_components(3, &[]).is_empty());
    }
}
