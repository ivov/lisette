mod diagnostic;
mod fix;
mod sink;

pub mod attribute;
pub mod embed;
pub mod emit;
pub mod infer;
pub mod lint;
pub mod module_graph;
pub mod pattern;
pub mod render;

pub use diagnostic::{IndexedSource, LisetteDiagnostic, Report};
pub use fix::{Edit, Fix, FixApplicationOutcome, apply_fixes};
pub use sink::{DiagnosticCheckpoint, LocalSink};

pub use lint::{IssueKind, UnusedExpressionKind};
