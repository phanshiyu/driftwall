//! Finds a cycle in a directed graph, the same one on every run.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The shortest cycle through the smallest vertex that is on any cycle, as the vertices in order:
/// `[a, b, c]` is the cycle `a -> b -> c -> a`. `None` when the graph is acyclic.
///
/// `graph` maps each vertex to the vertices it points at. A vertex that only appears as a target
/// has no outgoing edge, so it is on no cycle. Ties between shortest cycles go to the smallest
/// next vertex at every step, so the answer depends on the graph alone.
pub(super) fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    graph
        .keys()
        .find_map(|start| shortest_cycle_through(graph, start))
}

/// The shortest cycle that starts and ends at `start`, found by breadth first search.
fn shortest_cycle_through(
    graph: &BTreeMap<String, BTreeSet<String>>,
    start: &str,
) -> Option<Vec<String>> {
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue: VecDeque<&str> = VecDeque::from([start]);
    while let Some(vertex) = queue.pop_front() {
        for next in graph.get(vertex).into_iter().flatten() {
            if next == start {
                return Some(path_to(&parent, vertex, start));
            }
            if next != vertex && !parent.contains_key(next.as_str()) {
                parent.insert(next, vertex);
                queue.push_back(next);
            }
        }
    }
    None
}

/// The vertices from `start` to `end` along the `parent` links of a search that began at `start`.
fn path_to(parent: &BTreeMap<&str, &str>, end: &str, start: &str) -> Vec<String> {
    let mut path = vec![end.to_owned()];
    let mut at = end;
    while at != start {
        let Some(&before) = parent.get(at) else {
            break;
        };
        path.push(before.to_owned());
        at = before;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &str)]) -> BTreeMap<String, BTreeSet<String>> {
        let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for &(from, to) in edges {
            graph
                .entry(from.to_owned())
                .or_default()
                .insert(to.to_owned());
            graph.entry(to.to_owned()).or_default();
        }
        graph
    }

    fn cycle(edges: &[(&str, &str)]) -> Option<Vec<String>> {
        find_cycle(&graph(edges))
    }

    #[test]
    fn an_acyclic_graph_has_no_cycle() {
        assert_eq!(cycle(&[("a", "b"), ("b", "c"), ("a", "c")]), None);
        assert_eq!(cycle(&[]), None);
    }

    #[test]
    fn a_two_cycle_and_a_long_cycle_are_listed_in_order() {
        assert_eq!(
            cycle(&[("a", "b"), ("b", "a")]),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(
            cycle(&[("a", "b"), ("b", "c"), ("c", "a")]),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn the_cycle_is_the_shortest_through_the_smallest_vertex_on_one() {
        // why: `a` is on the long cycle a-b-c-d and `b` is on the short b-c-b; `a` decides.
        let found = cycle(&[("a", "b"), ("b", "c"), ("c", "d"), ("d", "a"), ("c", "b")]);
        assert_eq!(
            found,
            Some(vec!["a".into(), "b".into(), "c".into(), "d".into()])
        );
        // why: `a` points into a cycle but is on none, so the smallest vertex on one is `b`.
        let found = cycle(&[("a", "b"), ("b", "c"), ("c", "b")]);
        assert_eq!(found, Some(vec!["b".into(), "c".into()]));
    }

    #[test]
    fn a_shorter_cycle_wins_over_a_smaller_next_vertex() {
        let found = cycle(&[("a", "b"), ("b", "c"), ("c", "a"), ("a", "d"), ("d", "a")]);
        assert_eq!(found, Some(vec!["a".into(), "d".into()]));
    }
}
