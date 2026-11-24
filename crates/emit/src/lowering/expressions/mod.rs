pub(in crate::lowering) mod access;
mod identifiers;
pub(in crate::lowering) mod literals;
pub(in crate::lowering) mod never;
pub(in crate::lowering) mod operators;
pub(in crate::lowering) mod staging;
pub(in crate::lowering) mod top_items;
pub(in crate::lowering) mod values;

pub(in crate::lowering) use operators::{BinaryOperand, flip_comparison};
