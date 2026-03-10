//! Merge ordering algorithms.
//!
//! Provides two strategies for ordering sessions before sequential merge:
//! - **Completion-time**: preserves manifest order (identity function).
//! - **File-overlap**: greedy algorithm that picks sessions with least overlap
//!   against the already-merged file set, minimizing merge conflicts.

use std::collections::HashSet;

use crate::merge::types::{MergeOrderStrategy, MergePlan, PairwiseOverlap, SessionPlanEntry};
use crate::merge::CompletedSession;

/// Order sessions according to the given strategy.
///
/// Returns the reordered sessions and a [`MergePlan`] describing the ordering
/// decision for downstream display (Plan 03).
pub(crate) fn order_sessions(
    sessions: Vec<CompletedSession>,
    strategy: MergeOrderStrategy,
) -> (Vec<CompletedSession>, MergePlan) {
    match strategy {
        MergeOrderStrategy::CompletionTime => completion_time_order(sessions),
        MergeOrderStrategy::FileOverlap => file_overlap_order(sessions),
    }
}

/// Preserve manifest (input) order — identity function.
fn completion_time_order(sessions: Vec<CompletedSession>) -> (Vec<CompletedSession>, MergePlan) {
    let entries = sessions_to_plan_entries(&sessions);
    let plan = MergePlan {
        strategy: MergeOrderStrategy::CompletionTime,
        fell_back: false,
        sessions: entries,
        pairwise_overlaps: Vec::new(),
    };
    (sessions, plan)
}

/// Greedy file-overlap ordering: iteratively pick the session with the least
/// overlap against the already-merged file set. Tiebreak by original manifest
/// position (lower index wins).
fn file_overlap_order(sessions: Vec<CompletedSession>) -> (Vec<CompletedSession>, MergePlan) {
    let pairwise = compute_pairwise_overlaps(&sessions);

    // Check if overlap strategy can meaningfully differentiate:
    // If ALL pairwise overlaps are equal (including all-zero), fall back.
    let can_differentiate = if pairwise.len() <= 1 {
        false
    } else {
        let first = pairwise[0].overlap_count();
        pairwise.iter().any(|p| p.overlap_count() != first)
    };

    if !can_differentiate {
        let entries = sessions_to_plan_entries(&sessions);
        let plan = MergePlan {
            strategy: MergeOrderStrategy::FileOverlap,
            fell_back: true,
            sessions: entries,
            pairwise_overlaps: pairwise,
        };
        return (sessions, plan);
    }

    // Greedy: pick session with minimum overlap against merged_files each round.
    let mut merged_files: HashSet<String> = HashSet::new();
    let mut remaining: Vec<Option<CompletedSession>> =
        sessions.into_iter().map(Some).collect();
    let total = remaining.len();
    let mut ordered: Vec<CompletedSession> = Vec::with_capacity(total);

    for _ in 0..total {
        let mut best_idx = None;
        let mut best_overlap = usize::MAX;
        let mut best_original_index = usize::MAX;

        for (i, slot) in remaining.iter().enumerate() {
            let Some(session) = slot else { continue };
            let overlap = session
                .changed_files
                .intersection(&merged_files)
                .count();
            if overlap < best_overlap
                || (overlap == best_overlap && session.original_index < best_original_index)
            {
                best_overlap = overlap;
                best_original_index = session.original_index;
                best_idx = Some(i);
            }
        }

        let idx = best_idx.expect("remaining should have at least one session");
        let session = remaining[idx].take().unwrap();
        merged_files.extend(session.changed_files.iter().cloned());
        ordered.push(session);
    }

    let entries = sessions_to_plan_entries(&ordered);
    let plan = MergePlan {
        strategy: MergeOrderStrategy::FileOverlap,
        fell_back: false,
        sessions: entries,
        pairwise_overlaps: pairwise,
    };
    (ordered, plan)
}

/// Compute pairwise file overlap between all session pairs.
pub(crate) fn compute_pairwise_overlaps(sessions: &[CompletedSession]) -> Vec<PairwiseOverlap> {
    let mut overlaps = Vec::new();
    for i in 0..sessions.len() {
        for j in (i + 1)..sessions.len() {
            let mut files: Vec<String> = sessions[i]
                .changed_files
                .intersection(&sessions[j].changed_files)
                .cloned()
                .collect();
            files.sort();
            overlaps.push(PairwiseOverlap {
                session_a: sessions[i].session_name.clone(),
                session_b: sessions[j].session_name.clone(),
                overlapping_files: files,
            });
        }
    }
    overlaps
}

/// Build plan entries from a session slice (preserving current order).
fn sessions_to_plan_entries(sessions: &[CompletedSession]) -> Vec<SessionPlanEntry> {
    sessions
        .iter()
        .map(|s| {
            let mut files: Vec<String> = s.changed_files.iter().cloned().collect();
            files.sort();
            SessionPlanEntry {
                session_name: s.session_name.clone(),
                branch_name: s.branch_name.clone(),
                changed_files: files,
                original_index: s.original_index,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(name: &str, files: &[&str], index: usize) -> CompletedSession {
        CompletedSession {
            session_name: name.to_string(),
            branch_name: format!("smelt/{name}"),
            task_description: None,
            changed_files: files.iter().map(|f| f.to_string()).collect(),
            original_index: index,
        }
    }

    #[test]
    fn completion_time_preserves_input_order() {
        let sessions = vec![
            make_session("alpha", &["a.rs"], 0),
            make_session("beta", &["b.rs"], 1),
            make_session("gamma", &["c.rs"], 2),
        ];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::CompletionTime);

        assert_eq!(ordered[0].session_name, "alpha");
        assert_eq!(ordered[1].session_name, "beta");
        assert_eq!(ordered[2].session_name, "gamma");
        assert_eq!(plan.strategy, MergeOrderStrategy::CompletionTime);
        assert!(!plan.fell_back);
        assert!(plan.pairwise_overlaps.is_empty());
    }

    #[test]
    fn file_overlap_no_overlaps_falls_back() {
        let sessions = vec![
            make_session("alpha", &["a.rs"], 0),
            make_session("beta", &["b.rs"], 1),
            make_session("gamma", &["c.rs"], 2),
        ];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::FileOverlap);

        // Falls back — preserves manifest order
        assert_eq!(ordered[0].session_name, "alpha");
        assert_eq!(ordered[1].session_name, "beta");
        assert_eq!(ordered[2].session_name, "gamma");
        assert_eq!(plan.strategy, MergeOrderStrategy::FileOverlap);
        assert!(plan.fell_back);
        // Pairwise overlaps computed but all zero
        assert!(plan.pairwise_overlaps.iter().all(|p| p.overlap_count() == 0));
    }

    #[test]
    fn file_overlap_reorders_correctly() {
        // Session A: {x.rs, y.rs}  index 0
        // Session B: {y.rs, z.rs}  index 1 — overlaps with A on y.rs
        // Session C: {w.rs}        index 2 — no overlap
        let sessions = vec![
            make_session("A", &["x.rs", "y.rs"], 0),
            make_session("B", &["y.rs", "z.rs"], 1),
            make_session("C", &["w.rs"], 2),
        ];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::FileOverlap);

        // Round 1: merged_files = {}, all have 0 overlap. Tiebreak by index → A wins.
        // Round 2: merged_files = {x.rs, y.rs}. B has 1 overlap (y.rs), C has 0 → C wins.
        // Round 3: merged_files = {x.rs, y.rs, w.rs}. B has 1 overlap (y.rs) → B last.
        assert_eq!(ordered[0].session_name, "A");
        assert_eq!(ordered[1].session_name, "C");
        assert_eq!(ordered[2].session_name, "B");
        assert_eq!(plan.strategy, MergeOrderStrategy::FileOverlap);
        assert!(!plan.fell_back);
    }

    #[test]
    fn empty_changed_files_have_zero_overlap() {
        let sessions = vec![
            make_session("empty", &[], 0),
            make_session("has-files", &["a.rs", "b.rs"], 1),
        ];

        let overlaps = compute_pairwise_overlaps(&sessions);

        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].overlap_count(), 0);
        assert!(overlaps[0].overlapping_files.is_empty());
    }

    #[test]
    fn tiebreak_by_original_index() {
        // All sessions have the same files → identical overlap with merged set at every step.
        // Tiebreak should pick by original_index ascending.
        let sessions = vec![
            make_session("third", &["shared.rs"], 2),
            make_session("first", &["shared.rs"], 0),
            make_session("second", &["shared.rs"], 1),
        ];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::FileOverlap);

        // All pairwise overlaps are equal (all 1) → falls back
        assert!(plan.fell_back);
        // Fallback preserves input order (not sorted by original_index)
        assert_eq!(ordered[0].session_name, "third");
        assert_eq!(ordered[1].session_name, "first");
        assert_eq!(ordered[2].session_name, "second");
    }

    #[test]
    fn tiebreak_by_original_index_when_overlaps_differ() {
        // Two sessions with no overlap, one with overlap — first two tie on overlap
        // but differ in original_index.
        let sessions = vec![
            make_session("B", &["unique-b.rs"], 1),
            make_session("A", &["unique-a.rs"], 0),
            make_session("C", &["unique-a.rs", "unique-b.rs"], 2),
        ];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::FileOverlap);

        // Round 1: merged = {}, all 0 overlap. Tiebreak: A (index 0) wins.
        assert_eq!(ordered[0].session_name, "A");
        // Round 2: merged = {unique-a.rs}. B has 0 overlap, C has 1. B wins.
        assert_eq!(ordered[1].session_name, "B");
        // Round 3: C last
        assert_eq!(ordered[2].session_name, "C");
        assert!(!plan.fell_back);
    }

    #[test]
    fn single_session_preserves_order() {
        let sessions = vec![make_session("only", &["file.rs"], 0)];

        let (ordered, plan) = order_sessions(sessions, MergeOrderStrategy::FileOverlap);

        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].session_name, "only");
        // Single session → 0 pairs → can't differentiate → falls back
        assert!(plan.fell_back);
    }

    #[test]
    fn pairwise_overlaps_computed_for_all_pairs() {
        let sessions = vec![
            make_session("A", &["x.rs", "y.rs"], 0),
            make_session("B", &["y.rs", "z.rs"], 1),
            make_session("C", &["z.rs", "w.rs"], 2),
        ];

        let overlaps = compute_pairwise_overlaps(&sessions);

        // 3 choose 2 = 3 pairs
        assert_eq!(overlaps.len(), 3);

        // A-B: y.rs
        let ab = overlaps.iter().find(|p| p.session_a == "A" && p.session_b == "B").unwrap();
        assert_eq!(ab.overlap_count(), 1);
        assert!(ab.overlapping_files.contains(&"y.rs".to_string()));

        // A-C: no overlap
        let ac = overlaps.iter().find(|p| p.session_a == "A" && p.session_b == "C").unwrap();
        assert_eq!(ac.overlap_count(), 0);

        // B-C: z.rs
        let bc = overlaps.iter().find(|p| p.session_a == "B" && p.session_b == "C").unwrap();
        assert_eq!(bc.overlap_count(), 1);
        assert!(bc.overlapping_files.contains(&"z.rs".to_string()));
    }
}
