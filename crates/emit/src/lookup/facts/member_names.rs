use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::names::go_name;
use syntax::types::Type;

impl EmitFacts<'_> {
    pub(crate) fn package_member_go_name(&self, package: &str, member: &str) -> String {
        if go_name::is_go_import(package) {
            member.to_string()
        } else if self.is_const(&format!("{package}.{member}")) {
            go_name::screaming_snake_to_camel(member)
        } else {
            go_name::snake_to_camel(member)
        }
    }

    pub(crate) fn method_go_name(&self, method: &str, is_public: bool) -> String {
        if is_public || self.method_needs_export(method) {
            go_name::snake_to_camel(method)
        } else {
            go_name::unexported_method_go_name(method)
        }
    }

    pub(crate) fn member_go_name(&self, owner: &Type, member: &str, is_exported: bool) -> String {
        go_name::member_go_name(
            owner,
            member,
            is_exported,
            self.embedded_field(owner, member),
        )
    }

    pub(crate) fn receiver_method_go_name(
        &self,
        receiver: &Type,
        method: &str,
        is_public: bool,
        tagged: bool,
    ) -> String {
        let owner = self.without_references(receiver);
        self.method_selector_go_name(owner.get_qualified_id(), method, is_public, tagged)
    }

    pub(crate) fn method_selector_go_name(
        &self,
        owner_id: Option<&str>,
        method: &str,
        is_public: bool,
        tagged: bool,
    ) -> String {
        let name = if go_name::keeps_go_spelling(owner_id, method) {
            method.to_string()
        } else {
            self.method_go_name(method, is_public)
        };
        if tagged {
            go_name::tagged_twin(&name)
        } else {
            name
        }
    }

    pub(crate) fn interface_method_go_name(&self, method: &str, is_public: bool) -> String {
        self.method_selector_go_name(
            None,
            method,
            is_public,
            self.method_uses_tagged_return(method),
        )
    }
}
