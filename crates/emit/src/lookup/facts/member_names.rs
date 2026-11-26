use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::names::go_name::{self, GoExport, MemberSpelling};
use syntax::program::is_go_import;
use syntax::types::Type;

impl EmitFacts<'_> {
    pub(crate) fn package_member_go_name(&self, package: &str, member: &str) -> String {
        if is_go_import(package) {
            member.to_string()
        } else if self.definition(&format!("{package}.{member}")).is_const() {
            go_name::screaming_snake_to_camel(member)
        } else {
            go_name::snake_to_camel(member)
        }
    }

    pub(crate) fn method_go_name(&self, method: &str, export: GoExport) -> String {
        if export == GoExport::Exported || self.method_needs_export(method) {
            go_name::snake_to_camel(method)
        } else {
            go_name::unexported_method_go_name(method)
        }
    }

    pub(crate) fn member_go_name(&self, owner: &Type, member: &str, export: GoExport) -> String {
        go_name::member_go_name(owner, member, export, self.embedded_field(owner, member))
    }

    pub(crate) fn method_selector_go_name(
        &self,
        owner_id: Option<&str>,
        method: &str,
        export: GoExport,
        spelling: MemberSpelling,
    ) -> String {
        let name = if go_name::keeps_go_spelling(owner_id, method) {
            method.to_string()
        } else {
            self.method_go_name(method, export)
        };
        match spelling {
            MemberSpelling::TaggedCounterpart => go_name::tagged_counterpart(&name),
            MemberSpelling::Plain => name,
        }
    }

    pub(crate) fn interface_method_go_name(&self, method: &str, export: GoExport) -> String {
        let spelling = if self.method_uses_tagged_return(method) {
            MemberSpelling::TaggedCounterpart
        } else {
            MemberSpelling::Plain
        };
        self.method_selector_go_name(None, method, export, spelling)
    }
}
