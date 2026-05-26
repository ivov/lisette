mod bool_literal_comparison;
mod double_negation;
mod duplicate_logical_operand;
mod empty_match_arm;
mod excess_parens_on_condition;
mod helpers;
mod identical_if_branches;
mod invisible_in_string;
mod match_literal_collection;
mod naming;
mod replaceable_with_zero_fill;
mod rest_only_slice_pattern;
mod self_assignment;
mod self_comparison;
mod single_arm_match;
mod uninterpolated_fstring;
mod unnecessary_raw_string;
mod unsigned_comparison;
mod verbose_failure_propagation;

pub use bool_literal_comparison::check_bool_literal_comparison;
pub use double_negation::check_double_negation;
pub use duplicate_logical_operand::check_duplicate_logical_operand;
pub use empty_match_arm::check_empty_match_arm;
pub use excess_parens_on_condition::check_excess_parens_on_condition;
pub use identical_if_branches::check_identical_if_branches;
pub use invisible_in_string::{
    check_invisible_in_string_expression, check_invisible_in_string_pattern,
};
pub use match_literal_collection::check_match_literal_collection;
pub use naming::{check_expression_naming, check_pattern_naming};
pub use replaceable_with_zero_fill::check_replaceable_with_zero_fill;
pub use rest_only_slice_pattern::check_rest_only_slice_pattern;
pub use self_assignment::check_self_assignment;
pub use self_comparison::check_self_comparison;
pub use single_arm_match::check_single_arm_match;
pub use uninterpolated_fstring::check_uninterpolated_fstring;
pub use unnecessary_raw_string::{
    check_unnecessary_raw_string_expression, check_unnecessary_raw_string_pattern,
};
pub use unsigned_comparison::check_unsigned_comparison;
pub use verbose_failure_propagation::check_verbose_failure_propagation;
