mod go_modules;
mod target;

pub use go_modules::{get_go_stdlib_packages, get_go_stdlib_typedef};
pub use target::Target;

pub const LIS_PRELUDE_SOURCE: &str = include_str!("../prelude.d.lis");

include!(concat!(env!("OUT_DIR"), "/stdlib_hash.rs"));
