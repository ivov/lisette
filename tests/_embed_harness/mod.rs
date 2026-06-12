pub mod compare;
pub mod go_answerer;
pub mod lisette_answer;
pub mod random_scenarios;
pub mod render_go;
pub mod render_lis;
pub mod run_check;
pub mod runner;
pub mod scenario;

#[cfg(test)]
pub mod corpus;
#[cfg(test)]
pub mod fixtures;

use scenario::NodeId;

#[derive(Clone, Debug)]
pub struct PrintedQuestion {
    pub root: NodeId,
    pub member: String,
    /// Also exercise the `Root.member` method-expression form. Set only for a
    /// public, value-receiver method on a non-generic root and declaring type,
    /// where `Root.member` is valid and `g(r)` needs no addressing.
    pub method_expression: bool,
}
