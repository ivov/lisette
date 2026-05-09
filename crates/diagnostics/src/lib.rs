mod diagnostic;
mod result;
mod sink;

pub mod attribute;
pub mod emit;
pub mod infer;
pub mod lint;
pub mod module_graph;
pub mod pattern;
pub mod render;

pub use diagnostic::{IndexedSource, LisetteDiagnostic, Report};
pub use result::SemanticResult;
pub use sink::LocalSink;

pub use lint::{IssueKind, UnusedExpressionKind};
pub use pattern::PatternIssue;
