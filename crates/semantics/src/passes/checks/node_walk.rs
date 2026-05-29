use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{BindingId, Expression};

use crate::facts::BindingFact;
use crate::passes::lints::ast_walk::visitor::visit_ast;
use crate::store::Store;

use super::{
    const_naming, decimal_file_mode, duplicate_bindings, empty_infinite_loop, empty_range,
    enum_variant_value, index_out_of_bounds, irrefutable_patterns, nan_comparison, newtype,
    oversized_shift, predeclared_shadowing, pub_type_export, receivers, repeated_if_condition,
    stringer_signature, temp_producing, unchanging_loop_condition,
};

type NodeCheck = fn(&Expression, &LocalSink);

const NODE_CHECKS: &[NodeCheck] = &[
    nan_comparison::check,
    empty_range::check,
    empty_infinite_loop::check,
    oversized_shift::check,
    repeated_if_condition::check,
    index_out_of_bounds::check,
    decimal_file_mode::check,
    duplicate_bindings::check,
    irrefutable_patterns::check,
    receivers::check,
    stringer_signature::check,
    predeclared_shadowing::check,
    pub_type_export::check,
    temp_producing::check,
];

pub(crate) fn run(
    items: &[Expression],
    store: &Store,
    bindings: &HashMap<BindingId, BindingFact>,
    is_d_lis: bool,
    sink: &LocalSink,
) {
    visit_ast(
        items,
        &mut |expression| {
            for check in NODE_CHECKS {
                check(expression, sink);
            }
            newtype::check(expression, store, sink);
            enum_variant_value::check(expression, store, sink);
            unchanging_loop_condition::check(expression, bindings, sink);
            if !is_d_lis {
                const_naming::check(expression, sink);
            }
        },
        &mut |_| {},
    );
}
