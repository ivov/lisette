//! Post-inference validators.
//!
//! Each validator is an independent read-only walk over the frozen typed AST
//! emitting diagnostics via a shared `DiagnosticSink`. Validators do not read
//! the checker's `TypeEnv` (the freeze pass substitutes bound type variables
//! before this stage runs) and do not share state with each other.
//!
//! Adding a validator: create a new submodule with a `pub(crate) fn run`
//! taking `(&[Expression], &DiagnosticSink)` (plus `&Facts` if needed) and
//! register it in `run_all` below.

use diagnostics::DiagnosticSink;
use syntax::ast::Expression;
use syntax::program::CoercionInfo;

use crate::facts::Facts;
use crate::store::Store;

mod duplicate_bindings;
mod generics;
mod irrefutable_patterns;
mod native_value_usage;
mod newtype;
mod post_inference;
mod prelude_shadowing;
mod receivers;
pub(crate) mod temp_producing;
mod unused_expressions;
mod visibility;

pub struct ValidatorContext<'a> {
    pub typed_ast: &'a [Expression],
    pub is_typedef: bool,
    pub module_id: &'a str,
    pub store: &'a Store,
    pub facts: &'a mut Facts,
    pub coercions: &'a CoercionInfo,
    pub sink: &'a DiagnosticSink,
}

pub fn run_all(ctx: &mut ValidatorContext<'_>) {
    duplicate_bindings::run(ctx.typed_ast, ctx.sink);
    irrefutable_patterns::run(ctx.typed_ast, ctx.sink);
    receivers::run(ctx.typed_ast, ctx.sink);
    prelude_shadowing::run(ctx.typed_ast, ctx.is_typedef, ctx.store, ctx.sink);
    generics::run(
        ctx.typed_ast,
        ctx.is_typedef,
        ctx.module_id,
        ctx.store,
        ctx.facts,
        ctx.sink,
    );
    newtype::run(ctx.typed_ast, ctx.store, ctx.sink);
    native_value_usage::run(ctx.typed_ast, ctx.module_id, ctx.store, ctx.sink);
    temp_producing::run(ctx.typed_ast, ctx.coercions, ctx.sink);
    unused_expressions::run(ctx.typed_ast, ctx.module_id, ctx.store, ctx.facts);
    visibility::run_module(ctx.module_id, ctx.store, ctx.sink);
    post_inference::run(ctx.facts, ctx.sink);
}
