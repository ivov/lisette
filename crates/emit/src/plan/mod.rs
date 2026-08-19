pub(crate) mod bodies;
pub(crate) mod calls;
#[cfg(debug_assertions)]
pub(crate) mod invariants;
pub(crate) mod lower;
pub(crate) mod placement;
pub(crate) mod values;

use crate::Planner;
use crate::names::go_name;
use diagnostics::LisetteDiagnostic;
use syntax::program::File;

pub(crate) struct PackagePlan {
    pub(crate) package_name: String,
    pub(crate) collision_diagnostics: Vec<LisetteDiagnostic>,
}

impl Planner<'_> {
    /// Resolve package-wide names and collisions before any item is rendered.
    pub(crate) fn build_package_plan(&mut self, files: &[&File], package_id: &str) -> PackagePlan {
        debug_assert_eq!(self.facts.current_package(), package_id);
        self.collect_escape_remap(files);
        self.derive_package_go_consts(files);
        self.collect_generic_renames(files);
        let collision_diagnostics = self.detect_name_collisions(files);

        let package_name = if self.facts.is_entry_package(package_id) {
            self.facts.entry_package_name().to_string()
        } else {
            let raw = package_id.rsplit('/').next().unwrap_or(package_id);
            go_name::sanitize_package_name(raw).into_owned()
        };

        PackagePlan {
            package_name,
            collision_diagnostics,
        }
    }
}
