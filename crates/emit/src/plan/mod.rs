pub(crate) mod bodies;
pub(crate) mod calls;
#[cfg(debug_assertions)]
pub(crate) mod invariants;
pub(crate) mod lower;
pub(crate) mod placement;
pub(crate) mod values;

use crate::Planner;
use crate::names::go_name;
use crate::state::file_namespace::FileNamespace;
use diagnostics::LisetteDiagnostic;
use syntax::program::File;

pub(crate) struct ModulePlan {
    pub(crate) package_name: String,
    pub(crate) files: Vec<FilePlan>,
    /// Package-block Go-name collisions detected before rendering. Attached to
    /// the first file's diagnostics at the render boundary.
    pub(crate) collision_diagnostics: Vec<LisetteDiagnostic>,
}

pub(crate) struct FilePlan {
    pub(crate) file_id: u32,
    pub(crate) output_name: String,
    pub(crate) make_functions: Vec<MakeFunctionPlan>,
    pub(crate) namespace: FileNamespace,
}

pub(crate) struct MakeFunctionPlan {
    pub(crate) enum_id: String,
    pub(crate) variant_name: String,
}

impl Planner<'_> {
    /// Run module-level collection and fix per-file identity before any item
    /// is rendered.
    pub(crate) fn build_module_plan(&mut self, files: &[&File], module_id: &str) -> ModulePlan {
        self.facts.set_current_module(module_id);
        self.collect_local_method_facts(files);
        self.collect_escape_remap(files);
        self.collect_generic_renames(files);
        let collision_diagnostics = self.detect_name_collisions(files);
        let mut make_functions_by_file = self.collect_local_make_function_plans();

        let package_name = if self.facts.is_entry_module(module_id) {
            self.facts.entry_package_name().to_string()
        } else {
            let raw = module_id.rsplit('/').next().unwrap_or(module_id);
            go_name::sanitize_package_name(raw).into_owned()
        };

        let file_plans = files
            .iter()
            .map(|file| FilePlan {
                file_id: file.id,
                output_name: file.go_filename(),
                make_functions: make_functions_by_file.remove(&file.id).unwrap_or_default(),
                namespace: FileNamespace::build(
                    file,
                    self.facts.go_module(),
                    self.facts.unused_imports_for_current_module(),
                    self.facts.go_package_names(),
                ),
            })
            .collect();

        ModulePlan {
            package_name,
            files: file_plans,
            collision_diagnostics,
        }
    }
}
