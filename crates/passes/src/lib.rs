mod analyze;
pub mod call_target;
mod passes;

pub use analyze::{AnalyzeOutput, analyze};
pub use passes::{Lint, run};
