use syntax::ast::{Expression, Pattern};

use crate::passes::walk::{
    FunctionRole, NodeCtx, PatternRole, apply_expression_checks, walk_nodes,
};

use super::{
    cast_nan_to_int, const_naming, decimal_file_mode, duplicate_bindings, empty_infinite_loop,
    empty_range, enum_variant_value, impossible_comparison, index_out_of_bounds,
    irrefutable_patterns, map_key, min_max, nan_comparison, newtype, oversized_shift,
    pub_type_export, receivers, repeated_if_condition, stringer_signature, temp_producing,
    unchanging_loop_condition,
};

fn run_expression_checks(expression: &Expression, ctx: &NodeCtx<'_>, _role: FunctionRole<'_>) {
    apply_expression_checks!(
        expression,
        ctx,
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
        (map_key::check, &[Let, TypeAlias, Call]),
        (newtype::check, &[Assignment, Reference]),
        (enum_variant_value::check, &[Identifier, DotAccess]),
        (unchanging_loop_condition::check, &[While]),
        (const_naming::check, &[Const]),
    );
}

fn ignore_patterns(_: &Pattern, _: &NodeCtx<'_>, _: PatternRole) {}

pub(crate) fn run<'a>(items: &'a [Expression], ctx: &NodeCtx<'a>) {
    walk_nodes(items, ctx, run_expression_checks, ignore_patterns);
}
