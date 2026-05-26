//! Warning-grade diagnostics emitted after inference. Three flavors:
//!
//! - `ast_walk` are syntactic pattern matches on the typed AST. One visitor
//!   traversal feeds many per-expression check functions. Use this when the
//!   trigger is "expression shape plus optionally its type".
//! - `from_facts` reads facts produced earlier by `passes::fact_producers`
//!   and emits diagnostics. Use this when the trigger is a property of the
//!   whole binding/expression set (used/unused, mutated/unmutated, reachable).
//! - `ref_graph` reads the cross-module reference graph. Use this when the
//!   trigger is "is this item referenced from somewhere".
//!
//! Errors live in `passes::checks` and in the `checker/` (inference) stage.

pub(crate) mod ast_walk;
pub(crate) mod from_facts;
pub(crate) mod ref_graph;

pub use from_facts::Lint;
