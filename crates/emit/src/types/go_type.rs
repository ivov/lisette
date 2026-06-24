use crate::EmitEffects;
use crate::Planner;
use crate::definitions::structs::struct_field_go_name;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use crate::types::prelude::PreludeType;
use syntax::program::DefinitionBody;
use syntax::types::Type;

#[derive(Debug, Clone, Default)]
pub(crate) struct GoType {
    pub(crate) code: String,
    pub(crate) needs_stdlib: bool,
    pub(crate) go_imports: Vec<String>,
}

impl GoType {
    pub(crate) fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            needs_stdlib: false,
            go_imports: Vec::new(),
        }
    }

    pub(crate) fn stdlib(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            needs_stdlib: true,
            go_imports: Vec::new(),
        }
    }

    pub(crate) fn with_go_import(code: impl Into<String>, go_path: String) -> Self {
        Self {
            code: code.into(),
            needs_stdlib: false,
            go_imports: vec![go_path],
        }
    }

    pub(crate) fn merge(&mut self, other: &GoType) {
        self.needs_stdlib = self.needs_stdlib || other.needs_stdlib;
        self.go_imports.extend(other.go_imports.iter().cloned());
    }

    fn merge_all<'a>(&mut self, others: impl IntoIterator<Item = &'a GoType>) {
        for other in others {
            self.merge(other);
        }
    }
}

impl std::fmt::Display for GoType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl Planner<'_> {
    pub(crate) fn go_type(&self, ty: &Type) -> GoType {
        match ty {
            Type::Nominal { id, params, .. } => self.emit_constructor(id, params, ty),
            Type::Simple(kind) => {
                if matches!(kind, syntax::types::SimpleKind::Unit) {
                    GoType::new("struct{}")
                } else {
                    GoType::new(kind.leaf_name().to_string())
                }
            }
            Type::Compound { kind, args } => self.emit_compound(*kind, args, ty),
            Type::Function(f) => self.emit_function_type(&f.params, &f.return_type),
            Type::Var { .. } => GoType::new("any"),
            Type::Forall { .. } => GoType::new("any"),
            Type::Parameter(name) => GoType::new(name.to_string()),
            Type::Never => GoType::new("struct{}"),
            Type::Error => unreachable!("Type::Error should not reach the emitter"),
            Type::Tuple(elements) => self.emit_tuple_type(elements),
            Type::ImportNamespace(_) => {
                unreachable!("Type::ImportNamespace should not reach the emitter's go_type")
            }
            Type::ReceiverPlaceholder => GoType::new("any"),
        }
    }

    fn emit_compound(&self, kind: syntax::types::CompoundKind, args: &[Type], ty: &Type) -> GoType {
        use syntax::types::CompoundKind;

        if kind == CompoundKind::Ref
            && let Some(inner) = args.first()
        {
            let inner_type = self.go_type(inner);
            let mut result = GoType::new(format!("*{}", inner_type.code));
            result.merge(&inner_type);
            return result;
        }

        if let Some(native) = NativeGoType::from_type(ty) {
            return self.emit_native_type(native, ty);
        }

        let param_types: Vec<GoType> = args.iter().map(|p| self.go_type(p)).collect();
        let type_args = param_types
            .iter()
            .map(|t| t.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        if kind == CompoundKind::EnumeratedSlice {
            return build_param_typed(format!("[]{}", type_args), &param_types);
        }
        if kind == CompoundKind::VarArgs {
            return build_param_typed(format!("...{}", type_args), &param_types);
        }
        if args.is_empty() {
            return GoType::new(kind.leaf_name().to_string());
        }
        build_param_typed(format!("{}[{}]", kind.leaf_name(), type_args), &param_types)
    }

    /// Render a type to Go text, recording its stdlib + Go-import effects.
    pub(crate) fn go_type_string(&self, ty: &Type) -> String {
        let result = self.go_type(ty);
        self.note_go_type(&result);
        result.code
    }

    pub(crate) fn format_type_args(&mut self, params: &[Type]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let args: Vec<String> = params.iter().map(|p| self.go_type_string(p)).collect();
        format!("[{}]", args.join(", "))
    }

    pub(crate) fn reconstruct_collapsed_type_args(
        &mut self,
        recipe: &str,
        mapping: &rustc_hash::FxHashMap<String, Type>,
    ) -> Option<String> {
        let mut parts = Vec::new();
        for entry in split_top_level_commas(recipe) {
            parts.push(self.render_recipe_entry(entry.trim(), mapping)?);
        }
        (!parts.is_empty()).then(|| format!("[{}]", parts.join(", ")))
    }

    fn render_recipe_entry(
        &mut self,
        entry: &str,
        mapping: &rustc_hash::FxHashMap<String, Type>,
    ) -> Option<String> {
        if let Some(elem) = entry
            .strip_prefix("Slice<")
            .and_then(|s| s.strip_suffix('>'))
        {
            let ty = mapping.get(elem.trim())?;
            return Some(format!("[]{}", self.go_type_string(ty)));
        }
        if let Some(inner) = entry.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
            let (key, value) = inner.split_once(',')?;
            let key_ty = mapping.get(key.trim())?;
            let value_ty = mapping.get(value.trim())?;
            return Some(format!(
                "map[{}]{}",
                self.go_type_string(key_ty),
                self.go_type_string(value_ty)
            ));
        }
        Some(self.go_type_string(mapping.get(entry)?))
    }

    fn emit_tuple_type(&self, elements: &[Type]) -> GoType {
        let arity = elements.len();
        let element_types: Vec<GoType> = elements.iter().map(|e| self.go_type(e)).collect();

        let mut result = GoType::stdlib(format!(
            "{}.Tuple{}[{}]",
            go_name::GO_STDLIB_PKG,
            arity,
            element_types
                .iter()
                .map(|t| t.code.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        result.merge_all(&element_types);
        result
    }

    fn emit_constructor(&self, qualified_name: &str, params: &[Type], ty: &Type) -> GoType {
        if let Some(go) = self.anon_struct_go_type(qualified_name) {
            return go;
        }

        let name = self.unqualify_name(qualified_name);

        if ty.is_unit() {
            return GoType::new("struct{}");
        }

        if qualified_name == "prelude.Ref"
            && let Some(inner) = params.first()
        {
            let inner_type = self.go_type(inner);
            let mut result = GoType::new(format!("*{}", inner_type.code));
            result.merge(&inner_type);
            return result;
        }

        if let Some(native) = NativeGoType::from_type(ty) {
            return self.emit_native_type(native, ty);
        }

        let param_types: Vec<GoType> = params.iter().map(|p| self.go_type(p)).collect();
        let type_args = param_types
            .iter()
            .map(|t| t.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        if name == "EnumeratedSlice" {
            return build_param_typed(format!("[]{}", type_args), &param_types);
        }
        if name == "VarArgs" {
            return build_param_typed(format!("...{}", type_args), &param_types);
        }

        if let Some(go_path) = self.resolve_go_import_path(qualified_name) {
            let code = if params.is_empty() {
                name.clone()
            } else {
                format!("{}[{}]", name, type_args)
            };
            return build_go_import_typed(code, go_path, &param_types);
        }

        if let Some(name) = qualified_name.strip_prefix(go_name::PRELUDE_PREFIX)
            && let Some(prelude) = PreludeType::from_name(name)
        {
            let type_arg_vec: Vec<String> = param_types.iter().map(|t| t.code.clone()).collect();
            let mut result = GoType::stdlib(prelude.emit_type(&type_arg_vec));
            result.merge_all(&param_types);
            return result;
        }

        if params.is_empty() {
            return GoType::new(name);
        }
        build_param_typed(format!("{}[{}]", name, type_args), &param_types)
    }

    /// Resolve a Go-import path for a nominal constructor: either an explicit
    /// `lisette/go/...` prefix on `qualified_name`, or an implicit one via a
    /// foreign module mapped to a Go import. Returns `None` for prelude,
    /// stdlib, or local nominal types.
    fn resolve_go_import_path(&self, qualified_name: &str) -> Option<String> {
        if let Some(rest) = qualified_name.strip_prefix(go_name::GO_IMPORT_PREFIX)
            && let Some((go_path, _)) = rest.rsplit_once('.')
        {
            return Some(go_path.to_string());
        }
        let (module, _) = qualified_name.split_once('.')?;
        if self.facts.is_foreign_module(module) && !go_name::is_go_import(module) {
            return Some(self.facts.go_import_path(module));
        }
        None
    }

    fn emit_native_type(&self, native: NativeGoType, ty: &Type) -> GoType {
        if !native.has_type_params() {
            return GoType::new(native.emit_type_syntax(&[]));
        }

        let stripped = ty.strip_refs();
        let args = stripped
            .get_type_params()
            .expect("native type with type params must have type args");

        let arg_types: Vec<GoType> = args.iter().map(|a| self.go_type(a)).collect();
        let type_args: Vec<String> = arg_types.iter().map(|t| t.code.clone()).collect();

        build_param_typed(native.emit_type_syntax(&type_args), &arg_types)
    }

    fn emit_function_type(&self, params: &[Type], return_ty: &Type) -> GoType {
        let param_types: Vec<GoType> = params.iter().map(|p| self.go_type(p)).collect();

        let lowered = self.classify_direct_emission(return_ty);
        let return_type = match &lowered {
            Some(shape) => self.lowered_return_go_type(shape, return_ty),
            None => self.go_type(return_ty),
        };

        let args = param_types
            .iter()
            .map(|t| t.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let is_void = lowered.is_none() && (return_ty.is_unit() || return_type.code == "struct{}");

        let code = if is_void {
            format!("func({})", args)
        } else {
            format!("func({}) {}", args, return_type.code)
        };

        let mut result = GoType::new(code);
        result.merge_all(&param_types);
        if !is_void {
            result.merge(&return_type);
        }
        result
    }

    fn unqualify_name(&self, id: &str) -> String {
        if id == "Unknown" {
            return "any".to_string();
        }
        let (module, unqualified) = if let Some(rest) = id.strip_prefix(go_name::GO_IMPORT_PREFIX) {
            let Some((path, ty)) = rest.rsplit_once('.') else {
                return go_name::escape_keyword(id).into_owned();
            };
            (&id[..go_name::GO_IMPORT_PREFIX.len() + path.len()], ty)
        } else {
            let Some(split) = id.split_once('.') else {
                return go_name::escape_keyword(id).into_owned();
            };
            split
        };

        if unqualified == "Unknown" {
            return "any".to_string();
        }

        let escaped = go_name::escape_keyword(unqualified);

        if self.facts.is_foreign_module(module) {
            // A non-exported foreign type name (first char not uppercase) is an
            // opaque handle that reached a type position and cannot be spelled
            // from another package. Backstop for any inferred misuse the checker
            // did not reject.
            assert!(
                escaped.starts_with(char::is_uppercase),
                "emit invariant violated: opaque Go handle `{id}` reached a type position. \
                 A handle value may only flow by inference as a bare value or direct call \
                 argument; it cannot be stored, returned, wrapped, or otherwise placed where \
                 its unexported Go type must be spelled."
            );
            let pkg = self.go_pkg_qualifier(module);
            format!("{}.{}", pkg, escaped)
        } else {
            escaped.into_owned()
        }
    }

    /// Prepend the receiver's generic params to the explicit type args (for
    /// native-method and UFCS call sites).
    pub(crate) fn format_type_args_with_receiver(
        &mut self,
        receiver_ty: &Type,
        type_args: &[Type],
    ) -> String {
        let mut go_type_strs = Vec::new();
        if let Some(params) = receiver_ty.get_type_params() {
            let params = params.to_vec();
            for param in &params {
                go_type_strs.push(self.go_type_string(param));
            }
        }
        for ta in type_args {
            go_type_strs.push(self.go_type_string(ta));
        }
        if go_type_strs.is_empty() {
            self.format_type_args(type_args)
        } else {
            format!("[{}]", go_type_strs.join(", "))
        }
    }

    pub(crate) fn zero_value(&self, ty: &Type) -> (String, EmitEffects) {
        let mut effects = EmitEffects::default();
        if self.facts.is_interface(ty) {
            return ("nil".to_string(), effects);
        }

        let go_ty = self.go_type(ty);
        effects.merge_from_go_type(&go_ty);

        let value = match go_ty.code.as_str() {
            "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16"
            | "uint32" | "uint64" | "uintptr" | "byte" | "rune" => "0".to_string(),
            "float32" | "float64" => "0.0".to_string(),
            "bool" => "false".to_string(),
            "string" => "\"\"".to_string(),
            "struct{}" => "struct{}{}".to_string(),
            s if s.starts_with("[]")
                || s.starts_with("map[")
                || s.starts_with("chan ")
                || s.starts_with("chan<-")
                || s.starts_with("<-chan")
                || s.starts_with("*")
                || s.starts_with("func") =>
            {
                "nil".to_string()
            }
            _ => format!("*new({})", go_ty.code),
        };
        (value, effects)
    }

    /// Render a `#[go(anon_struct)]` stand-in as inline `struct{...}`: its name
    /// has no Go counterpart, so `pkg.Name` would not compile.
    pub(crate) fn anon_struct_go_type(&self, id: &str) -> Option<GoType> {
        let definition = self.facts.definition(id)?;
        if !definition.is_anon_struct() {
            return None;
        }
        let DefinitionBody::Struct { fields, .. } = &definition.body else {
            return None;
        };
        let mut result = GoType::default();
        let rendered: Vec<String> = fields
            .iter()
            .map(|f| {
                let field_ty = self.go_type(&f.ty);
                result.merge(&field_ty);
                format!("{} {}", struct_field_go_name(f, &[]), field_ty.code)
            })
            .collect();
        result.code = if rendered.is_empty() {
            "struct{}".to_string()
        } else {
            format!("struct {{ {} }}", rendered.join("; "))
        };
        Some(result)
    }
}

fn build_param_typed(code: String, param_types: &[GoType]) -> GoType {
    let mut result = GoType::new(code);
    result.merge_all(param_types);
    result
}

fn build_go_import_typed(code: String, go_path: String, param_types: &[GoType]) -> GoType {
    let mut result = GoType::with_go_import(code, go_path);
    result.merge_all(param_types);
    result
}

/// Split a recipe like `Map<K, V>, K, V` on commas outside angle brackets, so a
/// nested `Map<K, V>` stays one entry.
fn split_top_level_commas(recipe: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in recipe.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                entries.push(&recipe[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    entries.push(&recipe[start..]);
    entries
}
