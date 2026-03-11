---
id: "041"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# Clone bound on MergeRunner is contagious

`MergeRunner<G: GitOps + Clone>` exists because it constructs `WorktreeManager::new(self.git.clone(), ...)`. Consider `Arc<G>` or passing `WorktreeManager` as dependency.
