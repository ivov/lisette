use std::sync::LazyLock;

use syntax::ast::Expression;
use syntax::ast::ExpressionKind::*;

use crate::passes::walk::{CheckTable, NodeCtx, walk_nodes};

use super::{
    cast_nan_to_int, const_naming, decimal_file_mode, duplicate_bindings, empty_infinite_loop,
    empty_range, enum_variant_value, impossible_comparison, index_out_of_bounds,
    irrefutable_patterns, min_max, nan_comparison, newtype, oversized_shift, predeclared_shadowing,
    pub_type_export, receivers, repeated_if_condition, stringer_signature, temp_producing,
    unchanging_loop_condition,
};

static NODE_CHECKS: LazyLock<CheckTable> = LazyLock::new(|| {
    CheckTable::new(
        &[
            (nan_comparison::check, &[Binary]),
            (min_max::check, &[Call]),
            (cast_nan_to_int::check, &[Cast]),
            (impossible_comparison::check, &[Binary]),
            (empty_range::check, &[Range]),
            (empty_infinite_loop::check, &[Loop]),
            (oversized_shift::check, &[Binary]),
            (repeated_if_condition::check, &[If]),
            (index_out_of_bounds::check, &[IndexedAccess]),
            (decimal_file_mode::check, &[Literal]),
            (
                duplicate_bindings::check,
                &[Let, For, IfLet, WhileLet, Match, Select, Function, Lambda],
            ),
            (
                irrefutable_patterns::check,
                &[Let, For, Function, Lambda, Select],
            ),
            (receivers::check, &[ImplBlock]),
            (stringer_signature::check, &[ImplBlock]),
            (
                predeclared_shadowing::check,
                &[Enum, Struct, TypeAlias, Interface, Function, ImplBlock],
            ),
            (
                pub_type_export::check,
                &[Struct, Enum, TypeAlias, Interface],
            ),
            (
                temp_producing::check,
                &[
                    Call,
                    StructCall,
                    Binary,
                    Unary,
                    Reference,
                    Cast,
                    If,
                    While,
                    IndexedAccess,
                    Range,
                    Literal,
                ],
            ),
            (newtype::check, &[Assignment, Reference]),
            (enum_variant_value::check, &[Identifier, DotAccess]),
            (unchanging_loop_condition::check, &[While]),
            (const_naming::check, &[Const]),
        ],
        &[],
    )
});

pub(crate) fn run(items: &[Expression], ctx: &NodeCtx) {
    walk_nodes(items, ctx, &NODE_CHECKS);
}
