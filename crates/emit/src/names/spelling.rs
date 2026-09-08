use crate::Planner;
use crate::names::go_name;

impl Planner<'_> {
    pub(crate) fn method_go_name(&self, method: &str, is_public: bool) -> String {
        if is_public || self.method_needs_export(method) {
            go_name::snake_to_camel(method)
        } else {
            go_name::unexported_method_go_name(method)
        }
    }

    pub(crate) fn free_method_base_name(
        &self,
        receiver_name: &str,
        method: &str,
        is_public: bool,
    ) -> String {
        let exported = is_public || self.method_needs_export(method);
        format!(
            "{}_{}",
            receiver_name,
            go_name::free_method_part(method, exported)
        )
    }

    pub(crate) fn free_function_go_name(&self, name: &str, is_public: bool) -> String {
        if is_public {
            go_name::snake_to_camel(name)
        } else {
            self.package
                .escape_remap(name)
                .map(str::to_string)
                .unwrap_or_else(|| go_name::escape_reserved(name).into_owned())
        }
    }

    pub(crate) fn const_go_name(&self, identifier: &str) -> String {
        self.package
            .escape_remap(identifier)
            .map(str::to_string)
            .unwrap_or_else(|| go_name::screaming_snake_to_camel(identifier))
    }
}
