#![deny(clippy::too_many_lines)]

mod lookup;
mod lowering;
mod output;
mod target;

pub use lowering::planner::{EmitOptions, Planner};
pub use output::render::output::OutputFile;
pub use target::names::go_name::PRELUDE_IMPORT_PATH;
pub use target::names::go_name::go_test_function_name;
pub use target::names::go_name::root_package_name;
