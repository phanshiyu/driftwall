//! What the commands answer: findings, views, baselines, moves and where-answers.
//!
//! This model may use [`crate::contract`] and [`crate::code`]; neither uses it.

mod baseline;
mod blind;
mod diff;
mod lint;
mod moved;
mod overview;
mod picture;
mod rules;
mod status;
mod where_answer;

pub use baseline::{Applied, Baseline, BaselineError, BaselineOutcome, DebtAdded, path_key};
pub use blind::{
    BlindFile, BlindRef, BlindSpots, BlindSummary, BlindTally, UnreadGroup, UnreadTotal,
};
pub use diff::ContractDiff;
pub use lint::{LintReport, Severity, SiteDetail, Violation, ViolationKind};
pub use moved::{BoundaryMove, MoveCause, Moved};
pub use overview::{
    BoundaryEdgeSummary, EdgeSummary, MAX_SITES, NodeSummary, NodeView, Overview, PackageSummary,
    Site, ViaEdge,
};
pub use picture::{MarkdownSync, PictureView};
pub use rules::{
    DenySummary, EachSummary, InterfaceEntry, IsolatedSummary, Layer, Measure, NodeBudget,
    RulesSummary,
};
pub use status::{Declaration, EdgeStatus, Seen, edge_status};
pub use where_answer::WhereAnswer;
