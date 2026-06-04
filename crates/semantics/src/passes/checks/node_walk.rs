use syntax::ast::Expression;

use crate::passes::walk::{NodeCheck, NodeCtx, walk_nodes};

use super::{
    const_naming, decimal_file_mode, duplicate_bindings, empty_infinite_loop, empty_range,
    enum_variant_value, index_out_of_bounds, irrefutable_patterns, nan_comparison, newtype,
    oversized_shift, predeclared_shadowing, pub_type_export, receivers, repeated_if_condition,
    stringer_signature, temp_producing, unchanging_loop_condition,
};

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
    newtype::check,
    enum_variant_value::check,
    unchanging_loop_condition::check,
    const_naming::check,
];

pub(crate) fn run(items: &[Expression], ctx: &NodeCtx) {
    walk_nodes(items, ctx, NODE_CHECKS, &[]);
}
