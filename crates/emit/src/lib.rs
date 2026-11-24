#![deny(clippy::too_many_lines)]

mod lookup;
mod lowering;
mod output;
mod target;

pub(crate) use lookup::facts::context::LineIndex;
pub(crate) use lookup::facts::context::SharedEmitContext;
pub(crate) use lookup::facts::emit_facts::EmitFacts;
pub(crate) use lookup::facts::queries::is_order_sensitive;
pub(crate) use lookup::state::enclosing::ReturnContext;
pub(crate) use output::render::Renderer;
pub(crate) use output::render::output::OutputCollector;
pub(crate) use target::names::go_name;
pub(crate) use target::names::go_name::escape_local;
pub(crate) use target::types::enum_layout::EnumLayout;

pub use lowering::planner::{EmitOptions, Planner};
pub use output::render::output::OutputFile;
pub use target::names::go_name::PRELUDE_IMPORT_PATH;
pub use target::names::go_name::go_test_function_name;
pub use target::names::go_name::root_package_name;
