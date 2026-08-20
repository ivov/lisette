mod analysis;
pub mod cache;
pub mod checker;
mod closed_domain;
pub(crate) mod diagnostics;
pub mod facts;
pub mod generics;
pub mod loader;
pub mod package_graph;
pub mod path;
pub mod prelude;
pub mod store;
pub mod zero;

pub use analysis::{
    AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, InferenceOutput, PARALLEL_THRESHOLD,
    ProjectKind, RecoverTarget, run_inference,
};
