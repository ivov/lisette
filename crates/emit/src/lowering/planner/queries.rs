use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::file_namespace::FileNamespace;
use crate::lookup::state::scope::ScopeState;

#[derive(Clone, Copy)]
pub(in crate::lowering) struct PlannerQueries<'p, 'a> {
    pub(in crate::lowering) facts: &'p EmitFacts<'a>,
    pub(in crate::lowering) names: &'a PackageNames,
    pub(in crate::lowering) scope: &'p ScopeState,
    pub(in crate::lowering) namespace: &'p FileNamespace,
}
