use diagnostics::LocalSink;
use syntax::ast::Expression;

use crate::passes::lints::ast_walk::visitor::visit_ast;
use crate::store::Store;

use super::{
    const_naming, decimal_file_mode, duplicate_bindings, empty_infinite_loop, empty_range,
    enum_variant_value, index_out_of_bounds, irrefutable_patterns, nan_comparison, newtype,
    oversized_shift, predeclared_shadowing, pub_type_export, receivers, repeated_if_condition,
    stringer_signature, temp_producing,
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

pub(crate) fn run(items: &[Expression], store: &Store, is_d_lis: bool, sink: &LocalSink) {
    visit_ast(
        items,
        &mut |expression| {
            for check in NODE_CHECKS {
                check(expression, sink);
            }
            newtype::check(expression, store, sink);
            enum_variant_value::check(expression, store, sink);
            if !is_d_lis {
                const_naming::check(expression, sink);
            }
        },
        &mut |_| {},
    );
}
