use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::{GeneratedPackage, GoExport};
use crate::target::types::display_form::{DisplayForm, UNIT_DISPLAY};
use rustc_hash::FxHashSet;
use syntax::ast::{Attribute, StructFieldDefinition};
use syntax::go_names;
use syntax::go_names::{ENUM_GO_STRINGER_METHOD, ENUM_STRINGER_METHOD};
use syntax::program::{Definition, DefinitionBody};
use syntax::program::{MethodOrigin, interface_requirements};
use syntax::types::Type;

const DEBUG_STRING_METHOD: &str = "DebugString";
const STRINGER_METHODS: [&str; 2] = ["string", ENUM_STRINGER_METHOD];

pub(crate) fn should_synthesize_stringer(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|a| a.name == "display")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StructShape {
    Record,
    Tuple,
}

#[derive(Clone, Copy)]
pub(crate) enum SynthesizedMethod {
    Format(StringFormat<'static>),
    StringerShadow,
    Json,
    ToString,
    Equals,
}

#[derive(Clone, Copy)]
pub(crate) enum StringFormat<'a> {
    Display { method: &'a str, qualified: bool },
    Debug,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum StringerKind {
    Synthesized,
    Foreign,
}

fn definition_emits_go_string_field(definition: &Definition) -> bool {
    let DefinitionBody::Struct { fields, .. } = &definition.body else {
        return false;
    };
    let forces_export = definition.is_serialized();
    fields
        .iter()
        .any(|field| go_names::struct_field_go_name(field, forces_export) == ENUM_STRINGER_METHOD)
}

impl<'a> StringFormat<'a> {
    pub(crate) fn method(self) -> &'a str {
        match self {
            StringFormat::Display { method, .. } => method,
            StringFormat::Debug { .. } => DEBUG_STRING_METHOD,
        }
    }

    pub(crate) fn prefix(self, type_name: &str) -> String {
        match self {
            StringFormat::Display {
                qualified: true, ..
            } => format!("{type_name}."),
            StringFormat::Display { .. } | StringFormat::Debug => String::new(),
        }
    }

    pub(crate) fn verb(self, field: DisplayForm) -> &'static str {
        match (self, field) {
            (_, DisplayForm::Unit) => UNIT_DISPLAY,
            (_, DisplayForm::Function) => "%p",
            (StringFormat::Display { .. }, DisplayForm::Char) => "%c",
            (StringFormat::Display { .. }, _) => "%v",
            (StringFormat::Debug { .. }, _) => "%s",
        }
    }

    pub(crate) fn format_argument(
        self,
        value: GoExpression,
        field: DisplayForm,
    ) -> Option<GoExpression> {
        match (self, field) {
            (_, DisplayForm::Unit) => None,
            (StringFormat::Display { .. }, _) | (StringFormat::Debug, DisplayForm::Function) => {
                Some(value)
            }
            (StringFormat::Debug, _) => Some(GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                vec![value],
            )),
        }
    }
}

impl EmitFacts<'_> {
    pub(crate) fn declared_field_go_name(
        &self,
        struct_id: &str,
        field: &StructFieldDefinition,
    ) -> String {
        let forces_export = self.definition(struct_id).is_serialized();
        go_names::struct_field_go_name(field, forces_export).into_owned()
    }

    fn declares_string_selector(&self, id: &str) -> bool {
        let definition = self.definition(id);
        if matches!(definition.body, DefinitionBody::Interface { .. }) {
            return self.interface_has_string_selector(id);
        }
        STRINGER_METHODS
            .iter()
            .any(|method| self.emits_go_method(id, method))
    }

    fn interface_has_string_selector(&self, id: &str) -> bool {
        let interface_ty = Type::Nominal {
            id: id.into(),
            params: vec![],
            writable: false,
        };
        interface_requirements(&interface_ty, |id| Some(self.definition(id)))
            .iter()
            .any(|requirement| STRINGER_METHODS.contains(&requirement.name.as_str()))
    }

    fn should_synthesize_to_string(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.method(&qualified, "to_string")
            .is_some_and(|method| method.origin == MethodOrigin::Synthesized)
    }

    fn should_synthesize_equals(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.synthesizes_equals(qualified.as_str())
    }

    pub(crate) fn is_newtype_definition(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.definition(qualified.as_str()).is_newtype()
    }

    fn is_pointer_backed_newtype(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        self.definition(qualified.as_str())
            .is_pointer_backed_newtype(|id| Some(self.definition(id)))
    }

    fn stringer_overrides(&self, name: &str) -> (bool, bool) {
        let qualified = self.qualified_current(name);
        let declares = |methods: &[&str]| {
            methods
                .iter()
                .any(|method| self.has_stringer_signature(&qualified, method))
        };
        (
            declares(&STRINGER_METHODS),
            declares(&["goString", ENUM_GO_STRINGER_METHOD]),
        )
    }

    fn debug_string_override(&self, name: &str) -> bool {
        let qualified = self.qualified_current(name);
        (self.method_needs_export("debug_string")
            && self.has_stringer_signature(&qualified, "debug_string"))
            || self.has_stringer_signature(&qualified, DEBUG_STRING_METHOD)
    }

    fn synthesizes_debug_string(&self, name: &str) -> bool {
        self.tests_enabled() && !self.debug_string_override(name)
    }

    pub(crate) fn to_string_method_go_name(&self) -> String {
        self.method_go_name("to_string", GoExport::Unexported)
    }

    pub(crate) fn equals_method_go_name(&self) -> String {
        self.method_go_name("equals", GoExport::Unexported)
    }

    fn stringer_method_name(&self, name: &str, attributes: &[Attribute]) -> Option<&'static str> {
        if !should_synthesize_stringer(attributes) {
            return None;
        }
        match self.stringer_overrides(name) {
            (true, true) => None,
            (true, false) => Some(ENUM_GO_STRINGER_METHOD),
            _ => Some(ENUM_STRINGER_METHOD),
        }
    }

    pub(crate) fn enum_synthesized_methods(
        &self,
        name: &str,
        attributes: &[Attribute],
    ) -> Vec<SynthesizedMethod> {
        let mut methods = Vec::new();
        if should_synthesize_stringer(attributes) {
            let (has_user_string, has_user_go_string) = self.stringer_overrides(name);
            if !has_user_string {
                methods.push(SynthesizedMethod::Format(StringFormat::Display {
                    method: ENUM_STRINGER_METHOD,
                    qualified: false,
                }));
            }
            if !has_user_go_string {
                methods.push(SynthesizedMethod::Format(StringFormat::Display {
                    method: ENUM_GO_STRINGER_METHOD,
                    qualified: true,
                }));
            }
        }
        if attributes.iter().any(|attribute| attribute.name == "json") {
            methods.push(SynthesizedMethod::Json);
        }
        if self.synthesizes_debug_string(name) {
            methods.push(SynthesizedMethod::Format(StringFormat::Debug));
        }
        if self.should_synthesize_to_string(name) {
            methods.push(SynthesizedMethod::ToString);
        }
        if self.should_synthesize_equals(name) {
            methods.push(SynthesizedMethod::Equals);
        }
        methods
    }

    pub(crate) fn synthesized_method_go_names(&self, method: SynthesizedMethod) -> Vec<String> {
        match method {
            SynthesizedMethod::Format(format) => vec![format.method().to_string()],
            SynthesizedMethod::StringerShadow => vec![ENUM_STRINGER_METHOD.to_string()],
            SynthesizedMethod::Json => vec!["MarshalJSON".to_string(), "UnmarshalJSON".to_string()],
            SynthesizedMethod::ToString => vec![self.to_string_method_go_name()],
            SynthesizedMethod::Equals => vec![self.equals_method_go_name()],
        }
    }

    pub(crate) fn synthesizes_embedded_stringer_shadow(&self, name: &str) -> bool {
        let id = self.qualified_current(name);
        let definition = self.definition(&id);
        !definition.is_display()
            && self.stringer_kind_of(&id, &mut FxHashSet::default())
                == Some(StringerKind::Synthesized)
    }

    fn stringer_kind(&self, ty: &Type, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let id = &self.receiver_nominal_id(ty)?;
        if !visited.insert(id.to_string()) {
            return None;
        }
        let kind = self.stringer_kind_of(id.as_str(), visited);
        visited.remove(id.as_str());
        kind
    }

    fn stringer_kind_of(&self, id: &str, visited: &mut FxHashSet<String>) -> Option<StringerKind> {
        let definition = self.definition(id);
        if self.declares_string_selector(id) {
            return Some(StringerKind::Foreign);
        }
        if matches!(definition.body, DefinitionBody::Interface { .. }) {
            return None;
        }
        if definition_emits_go_string_field(definition) {
            return Some(StringerKind::Foreign);
        }
        if definition.is_display() {
            return Some(StringerKind::Synthesized);
        }
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return None;
        };
        self.promoted_stringer_kind(fields, visited)
    }

    fn promoted_stringer_kind(
        &self,
        fields: &[StructFieldDefinition],
        visited: &mut FxHashSet<String>,
    ) -> Option<StringerKind> {
        let mut kinds = fields
            .iter()
            .filter(|f| f.is_embedded())
            .filter_map(|f| self.stringer_kind(&f.ty, visited));
        let first = kinds.next()?;
        if kinds.next().is_some() {
            return None;
        }
        matches!(first, StringerKind::Synthesized).then_some(StringerKind::Synthesized)
    }

    pub(crate) fn struct_synthesized_methods(
        &self,
        name: &str,
        attributes: &[Attribute],
        shape: StructShape,
    ) -> Vec<SynthesizedMethod> {
        if shape == StructShape::Tuple && self.is_pointer_backed_newtype(name) {
            return Vec::new();
        }
        let mut methods = Vec::new();
        if let Some(method) = self.stringer_method_name(name, attributes) {
            methods.push(SynthesizedMethod::Format(StringFormat::Display {
                method,
                qualified: false,
            }));
        }
        if self.synthesizes_debug_string(name) {
            methods.push(SynthesizedMethod::Format(StringFormat::Debug));
        }
        if self.should_synthesize_to_string(name) {
            methods.push(SynthesizedMethod::ToString);
        }
        if shape == StructShape::Record {
            if self.should_synthesize_equals(name) {
                methods.push(SynthesizedMethod::Equals);
            }
            if self.synthesizes_embedded_stringer_shadow(name) {
                methods.push(SynthesizedMethod::StringerShadow);
            }
        }
        methods
    }
}
