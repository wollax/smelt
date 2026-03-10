//! DAG construction from manifest, validation, and ready-set computation.

use std::collections::{HashMap, HashSet};

use petgraph::algo::is_cyclic_directed;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;

use crate::error::{Result, SmeltError};
use crate::session::manifest::Manifest;

/// A directed acyclic graph of session names, used for dependency-ordered execution.
pub type SessionDag = DiGraph<String, ()>;

/// Build a DAG from the manifest's session definitions.
///
/// Nodes are session names. Edges go from dependency to dependent
/// (i.e., from a session that must complete first to the session that depends on it).
///
/// When `parallel_by_default` is `false`, sessions without explicit `depends_on`
/// are chained sequentially in manifest order.
///
/// Returns an error if the resulting graph contains a cycle.
pub fn build_dag(manifest: &Manifest) -> Result<SessionDag> {
    let mut graph = SessionDag::new();
    let mut name_to_idx: HashMap<&str, NodeIndex> = HashMap::new();

    // Add nodes
    for session in &manifest.sessions {
        let idx = graph.add_node(session.name.clone());
        name_to_idx.insert(&session.name, idx);
    }

    // Add explicit dependency edges
    for session in &manifest.sessions {
        if let Some(ref deps) = session.depends_on {
            let to = name_to_idx[session.name.as_str()];
            for dep in deps {
                let from = *name_to_idx.get(dep.as_str()).ok_or_else(|| {
                    SmeltError::ManifestParse(format!(
                        "session '{}' depends on unknown session '{dep}'",
                        session.name
                    ))
                })?;
                graph.add_edge(from, to, ());
            }
        }
    }

    // Add implicit sequential edges when parallel_by_default=false
    if !manifest.manifest.parallel_by_default {
        let no_deps: Vec<&str> = manifest
            .sessions
            .iter()
            .filter(|s| s.depends_on.is_none())
            .map(|s| s.name.as_str())
            .collect();

        for pair in no_deps.windows(2) {
            let from = name_to_idx[pair[0]];
            let to = name_to_idx[pair[1]];
            graph.add_edge(from, to, ());
        }
    }

    // Validate: no cycles
    if is_cyclic_directed(&graph) {
        return Err(SmeltError::DependencyCycle {
            details: "dependency cycle detected in session DAG".to_string(),
        });
    }

    Ok(graph)
}

/// Find sessions that are ready to execute.
///
/// A session is ready when:
/// - It is not already completed, in-flight, or skipped.
/// - All of its incoming neighbors (dependencies) are in `completed` or `skipped`.
pub fn ready_set(
    dag: &SessionDag,
    completed: &HashSet<NodeIndex>,
    in_flight: &HashSet<NodeIndex>,
    skipped: &HashSet<NodeIndex>,
) -> Vec<NodeIndex> {
    dag.node_indices()
        .filter(|n| !completed.contains(n) && !in_flight.contains(n) && !skipped.contains(n))
        .filter(|&n| {
            dag.neighbors_directed(n, Direction::Incoming)
                .all(|pred| completed.contains(&pred) || skipped.contains(&pred))
        })
        .collect()
}

/// Mark all transitive dependents of a failed node as skipped.
///
/// Performs a BFS from `failed_node` through outgoing edges, adding
/// all reachable nodes to the `skipped` set.
pub fn mark_skipped_dependents(
    dag: &SessionDag,
    failed_node: NodeIndex,
    skipped: &mut HashSet<NodeIndex>,
) {
    let mut queue = std::collections::VecDeque::new();

    // Seed with direct dependents
    for dependent in dag.neighbors_directed(failed_node, Direction::Outgoing) {
        if skipped.insert(dependent) {
            queue.push_back(dependent);
        }
    }

    // BFS through transitive dependents
    while let Some(node) = queue.pop_front() {
        for dependent in dag.neighbors_directed(node, Direction::Outgoing) {
            if skipped.insert(dependent) {
                queue.push_back(dependent);
            }
        }
    }
}

/// Find the node index for a session by name.
pub fn node_by_name(dag: &SessionDag, name: &str) -> Option<NodeIndex> {
    dag.node_indices().find(|&n| dag[n] == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::manifest::{Manifest, ManifestMeta, SessionDef};

    fn make_manifest(
        parallel_by_default: bool,
        sessions: Vec<(&str, Option<Vec<&str>>)>,
    ) -> Manifest {
        Manifest {
            manifest: ManifestMeta {
                name: "test".to_string(),
                base_ref: "HEAD".to_string(),
                merge_strategy: None,
                parallel_by_default,
                on_failure: None,
            },
            sessions: sessions
                .into_iter()
                .map(|(name, deps)| SessionDef {
                    name: name.to_string(),
                    task: Some("task".to_string()),
                    task_file: None,
                    file_scope: None,
                    base_ref: None,
                    timeout_secs: None,
                    env: None,
                    depends_on: deps.map(|d| d.into_iter().map(String::from).collect()),
                    script: None,
                })
                .collect(),
        }
    }

    #[test]
    fn build_dag_parallel_sessions() {
        let manifest = make_manifest(true, vec![("a", None), ("b", None), ("c", None)]);
        let dag = build_dag(&manifest).expect("build_dag");
        assert_eq!(dag.node_count(), 3);
        assert_eq!(dag.edge_count(), 0);
    }

    #[test]
    fn build_dag_linear_chain() {
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", Some(vec!["b"])),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");
        assert_eq!(dag.node_count(), 3);
        assert_eq!(dag.edge_count(), 2);
    }

    #[test]
    fn build_dag_diamond() {
        // A -> {B, C} -> D
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", Some(vec!["a"])),
                ("d", Some(vec!["b", "c"])),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");
        assert_eq!(dag.node_count(), 4);
        assert_eq!(dag.edge_count(), 4); // a->b, a->c, b->d, c->d
    }

    #[test]
    fn build_dag_implicit_sequential() {
        // parallel_by_default=false creates chain: a->b->c
        let manifest = make_manifest(false, vec![("a", None), ("b", None), ("c", None)]);
        let dag = build_dag(&manifest).expect("build_dag");
        assert_eq!(dag.node_count(), 3);
        assert_eq!(dag.edge_count(), 2); // a->b, b->c
    }

    #[test]
    fn build_dag_mixed_explicit_implicit() {
        // parallel_by_default=false; "d" has explicit deps, "a","b","c" don't.
        // Implicit chain for no-dep sessions: a->b->c
        // Explicit edge: c->d
        let manifest = make_manifest(
            false,
            vec![
                ("a", None),
                ("b", None),
                ("c", None),
                ("d", Some(vec!["c"])),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");
        assert_eq!(dag.node_count(), 4);
        assert_eq!(dag.edge_count(), 3); // a->b, b->c (implicit), c->d (explicit)
    }

    #[test]
    fn ready_set_returns_roots() {
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", None),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");

        let completed = HashSet::new();
        let in_flight = HashSet::new();
        let skipped = HashSet::new();
        let ready = ready_set(&dag, &completed, &in_flight, &skipped);

        // "a" and "c" are roots (no incoming edges)
        let ready_names: HashSet<&str> = ready.iter().map(|&n| dag[n].as_str()).collect();
        assert_eq!(ready_names, HashSet::from(["a", "c"]));
    }

    #[test]
    fn ready_set_after_completion() {
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", Some(vec!["a"])),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");

        let a_idx = node_by_name(&dag, "a").unwrap();
        let completed: HashSet<NodeIndex> = [a_idx].into_iter().collect();
        let in_flight = HashSet::new();
        let skipped = HashSet::new();
        let ready = ready_set(&dag, &completed, &in_flight, &skipped);

        let ready_names: HashSet<&str> = ready.iter().map(|&n| dag[n].as_str()).collect();
        assert_eq!(ready_names, HashSet::from(["b", "c"]));
    }

    #[test]
    fn ready_set_excludes_in_flight() {
        let manifest = make_manifest(true, vec![("a", None), ("b", None)]);
        let dag = build_dag(&manifest).expect("build_dag");

        let a_idx = node_by_name(&dag, "a").unwrap();
        let completed = HashSet::new();
        let in_flight: HashSet<NodeIndex> = [a_idx].into_iter().collect();
        let skipped = HashSet::new();
        let ready = ready_set(&dag, &completed, &in_flight, &skipped);

        let ready_names: HashSet<&str> = ready.iter().map(|&n| dag[n].as_str()).collect();
        assert_eq!(ready_names, HashSet::from(["b"]));
    }

    #[test]
    fn ready_set_skipped_dep_satisfies() {
        // If "a" is skipped, "b" (depends on "a") should still become ready
        let manifest = make_manifest(
            true,
            vec![("a", None), ("b", Some(vec!["a"]))],
        );
        let dag = build_dag(&manifest).expect("build_dag");

        let a_idx = node_by_name(&dag, "a").unwrap();
        let completed = HashSet::new();
        let in_flight = HashSet::new();
        let skipped: HashSet<NodeIndex> = [a_idx].into_iter().collect();
        let ready = ready_set(&dag, &completed, &in_flight, &skipped);

        let ready_names: HashSet<&str> = ready.iter().map(|&n| dag[n].as_str()).collect();
        assert_eq!(ready_names, HashSet::from(["b"]));
    }

    #[test]
    fn mark_skipped_dependents_transitive() {
        // a -> b -> c
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", Some(vec!["b"])),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");

        let a_idx = node_by_name(&dag, "a").unwrap();
        let mut skipped = HashSet::new();
        mark_skipped_dependents(&dag, a_idx, &mut skipped);

        let skipped_names: HashSet<&str> = skipped.iter().map(|&n| dag[n].as_str()).collect();
        assert_eq!(skipped_names, HashSet::from(["b", "c"]));
    }

    #[test]
    fn mark_skipped_dependents_partial() {
        // a -> b, c (independent)
        let manifest = make_manifest(
            true,
            vec![
                ("a", None),
                ("b", Some(vec!["a"])),
                ("c", None),
            ],
        );
        let dag = build_dag(&manifest).expect("build_dag");

        let a_idx = node_by_name(&dag, "a").unwrap();
        let mut skipped = HashSet::new();
        mark_skipped_dependents(&dag, a_idx, &mut skipped);

        let skipped_names: HashSet<&str> = skipped.iter().map(|&n| dag[n].as_str()).collect();
        // Only "b" is skipped (depends on "a"); "c" is independent
        assert_eq!(skipped_names, HashSet::from(["b"]));
        assert!(!skipped_names.contains("c"));
    }

    #[test]
    fn node_by_name_found() {
        let manifest = make_manifest(true, vec![("alpha", None), ("beta", None)]);
        let dag = build_dag(&manifest).expect("build_dag");
        assert!(node_by_name(&dag, "alpha").is_some());
        assert!(node_by_name(&dag, "beta").is_some());
    }

    #[test]
    fn node_by_name_not_found() {
        let manifest = make_manifest(true, vec![("alpha", None)]);
        let dag = build_dag(&manifest).expect("build_dag");
        assert!(node_by_name(&dag, "nonexistent").is_none());
    }
}
