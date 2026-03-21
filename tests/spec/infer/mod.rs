#[allow(unused_imports)]
pub use crate::_harness::{
    InferResult, MockFileSystem, bool_type, con_type, float_type, float32_type, fun_type, infer,
    infer_module, int_type, int8_type, int16_type, ref_type, rune_type, slice_type, string_type,
    tuple_type, unit_type,
};

mod basics;
mod control_flow;
mod equality;
mod expressions;
mod functions;
mod post_inference;
mod recover;
mod refutability;
mod r#try;
mod types;
