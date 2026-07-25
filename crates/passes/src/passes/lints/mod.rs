//! Warning-grade diagnostics emitted after inference.

pub(crate) mod ast_walk;
pub(crate) mod from_facts;
pub(crate) mod ref_graph;
pub(crate) mod span_edit;
mod suppression;

pub use from_facts::Lint;
