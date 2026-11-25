#![deny(
    clippy::too_many_lines,
    clippy::redundant_clone,
    clippy::assigning_clones,
    clippy::implicit_clone,
    clippy::clone_on_ref_ptr
)]

mod lookup;
mod lowering;
mod output;
mod target;

pub use lowering::driver::EmitOptions;
pub use lowering::planner::Planner;
pub use output::render::output_file::OutputFile;
pub use target::names::go_name::go_test_function_name;
pub use target::names::go_name::root_package_name;
