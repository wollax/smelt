//! Session summary and scope isolation analysis.

pub mod analysis;
pub mod scope;
pub mod types;

pub use analysis::collect_summary;
pub use scope::check_scope;
pub use types::{FileStat, ScopeViolation, SessionSummary, SummaryReport, SummaryTotals};
