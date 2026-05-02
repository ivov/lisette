use crate::Emitter;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use crate::types::prelude::PreludeType;
use syntax::ast::Annotation;
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

    fn merge(&mut self, other: &GoType) {
        self.needs_stdlib = self.needs_stdlib || other.needs_stdlib;
        self.go_imports.extend(other.go_imports.iter().cloned());
    }

    pub(crate) fn merge_from(&mut self, other: &GoType) {
        self.merge(other);
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

impl Emitter<'_> {
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
            Type::Function {
                params,
                return_type,
                ..
            } => self.emit_function_type(params, return_type),
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
            let mut result = GoType::new(format!("[]{}", type_args));
            result.merge_all(&param_types);
            return result;
        }

        if kind == CompoundKind::VarArgs {
            let mut result = GoType::new(format!("...{}", type_args));
            result.merge_all(&param_types);
            return result;
        }

        if args.is_empty() {
            return GoType::new(kind.leaf_name().to_string());
        }

        let mut result = GoType::new(format!("{}[{}]", kind.leaf_name(), type_args));
        result.merge_all(&param_types);
        result
    }

    pub(crate) fn go_type_as_string(&mut self, ty: &Type) -> String {
        let result = self.go_type(ty);
        if result.needs_stdlib {
            self.flags.needs_stdlib = true;
        }
        for go_import in &result.go_imports {
            self.ensure_imported.insert(go_import.clone());
        }
        result.code
    }

    pub(crate) fn format_type_args(&mut self, params: &[Type]) -> String {
        if params.is_empty() {
            return String::new();
        }
        let args: Vec<String> = params.iter().map(|p| self.go_type_as_string(p)).collect();
        format!("[{}]", args.join(", "))
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
        let name = self.unqualify_name(qualified_name);

        if ty.is_unit() {
            return GoType::new("struct{}");
        }

        if (name == "Ref" || name.ends_with(".Ref"))
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
            let mut result = GoType::new(format!("[]{}", type_args));
            result.merge_all(&param_types);
            return result;
        }

        if name == "VarArgs" {
            let mut result = GoType::new(format!("...{}", type_args));
            result.merge_all(&param_types);
            return result;
        }

        if let Some(rest) = qualified_name.strip_prefix(go_name::GO_IMPORT_PREFIX)
            && let Some((go_path, _)) = rest.rsplit_once('.')
        {
            let mut result = if params.is_empty() {
                GoType::with_go_import(name, go_path.to_string())
            } else {
                GoType::with_go_import(format!("{}[{}]", name, type_args), go_path.to_string())
            };
            result.merge_all(&param_types);
            return result;
        }

        if let Some((module, _)) = qualified_name.split_once('.')
            && module != self.current_module
            && module != go_name::PRELUDE_MODULE
            && !go_name::is_go_import(module)
        {
            let go_path = format!("{}/{}", self.ctx.go_module, module);
            let mut result = if params.is_empty() {
                GoType::with_go_import(name.clone(), go_path)
            } else {
                GoType::with_go_import(format!("{}[{}]", name, type_args), go_path)
            };
            result.merge_all(&param_types);
            return result;
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

        let mut result = GoType::new(format!("{}[{}]", name, type_args));
        result.merge_all(&param_types);
        result
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

        let mut result = GoType::new(native.emit_type_syntax(&type_args));
        result.merge_all(&arg_types);
        result
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

        if module == self.current_module || module == go_name::PRELUDE_MODULE {
            escaped.into_owned()
        } else {
            let pkg = self.go_pkg_qualifier(module);
            format!("{}.{}", pkg, escaped)
        }
    }

    pub(crate) fn format_type_args_from_annotations(&mut self, type_args: &[Annotation]) -> String {
        if type_args.is_empty() {
            return String::new();
        }

        let args: Vec<String> = type_args
            .iter()
            .map(|ta| self.annotation_to_go_type(ta))
            .collect();

        format!("[{}]", args.join(", "))
    }

    /// Format type args by combining receiver type params with explicit type args.
    /// Used by native method and UFCS call sites where the receiver's generic
    /// params must be prepended to the explicit type args.
    pub(crate) fn format_type_args_with_receiver(
        &mut self,
        receiver_ty: &Type,
        type_args: &[Annotation],
    ) -> String {
        let mut go_type_strs = Vec::new();
        if let Some(params) = receiver_ty.get_type_params() {
            let params = params.to_vec();
            for param in &params {
                go_type_strs.push(self.go_type_as_string(param));
            }
        }
        for ta in type_args {
            go_type_strs.push(self.annotation_to_go_type(ta));
        }
        if go_type_strs.is_empty() {
            self.format_type_args_from_annotations(type_args)
        } else {
            format!("[{}]", go_type_strs.join(", "))
        }
    }

    pub(crate) fn zero_value(&self, ty: &Type) -> String {
        if self.as_interface(ty).is_some() {
            return "nil".to_string();
        }

        let go_ty = self.go_type(ty);

        match go_ty.code.as_str() {
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
        }
    }

    pub(crate) fn annotation_to_go_type(&mut self, annotation: &Annotation) -> String {
        let result = self.go_type_from_annotation(annotation);
        if result.needs_stdlib {
            self.flags.needs_stdlib = true;
        }
        for go_import in &result.go_imports {
            self.ensure_imported.insert(go_import.clone());
        }
        result.code
    }

    pub(crate) fn go_type_from_annotation(&self, annotation: &Annotation) -> GoType {
        match annotation {
            Annotation::Constructor { .. } if annotation.is_unit() => GoType::new("struct{}"),
            Annotation::Constructor { name, params, .. } => {
                self.constructor_annotation_to_go(name, params)
            }
            Annotation::Function {
                params,
                return_type,
                ..
            } => {
                let param_types: Vec<GoType> = params
                    .iter()
                    .map(|p| self.go_type_from_annotation(p))
                    .collect();

                let lowered = self.classify_annotation_direct_emission(return_type);
                let (return_go_type, is_void) = match &lowered {
                    Some(shape) => (
                        self.lowered_return_go_type_from_annotation(shape, return_type),
                        false,
                    ),
                    None => {
                        let is_omission = matches!(return_type.as_ref(), Annotation::Unknown);
                        let return_go_type = self.go_type_from_annotation(return_type);
                        let is_void = is_omission || return_go_type.code == "struct{}";
                        (return_go_type, is_void)
                    }
                };

                let args = param_types
                    .iter()
                    .map(|t| t.code.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                let code = if is_void {
                    format!("func({})", args)
                } else {
                    format!("func({}) {}", args, return_go_type.code)
                };

                let mut result = GoType::new(code);
                result.merge_all(&param_types);
                if !is_void {
                    result.merge(&return_go_type);
                }
                result
            }
            Annotation::Unknown => GoType::new("any"),
            Annotation::Tuple { elements, .. } => {
                let arity = elements.len();
                let element_types: Vec<GoType> = elements
                    .iter()
                    .map(|e| self.go_type_from_annotation(e))
                    .collect();

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
            Annotation::Opaque { .. } => {
                unreachable!("Annotation::Opaque should not be emitted as a Go type")
            }
        }
    }

    /// Lower a non-unit `Annotation::Constructor` into its Go equivalent.
    /// Checks in order: native Go types (slice/map/etc.), `Ref<T>` as `*T`,
    /// prelude containers (Option/Result/...), then generic instantiation.
    fn constructor_annotation_to_go(&self, name: &str, params: &[Annotation]) -> GoType {
        let base_name = self.unqualify_name(name);

        if let Some(native_type) = NativeGoType::from_name(&base_name) {
            let (param_types, type_params) = self.lower_annotation_params(params);
            let mut result = GoType::new(native_type.emit_type_syntax(&type_params));
            result.merge_all(&param_types);
            return result;
        }

        if base_name == "Ref" && params.len() == 1 {
            let inner = self.go_type_from_annotation(&params[0]);
            let mut result = GoType::new(format!("*{}", inner.code));
            result.merge(&inner);
            return result;
        }

        if let Some(prelude) = PreludeType::from_name(&base_name) {
            let (param_types, type_params) = self.lower_annotation_params(params);
            let mut result = GoType::stdlib(prelude.emit_type(&type_params));
            result.merge_all(&param_types);
            return result;
        }

        if params.is_empty() {
            return GoType::new(base_name);
        }
        let (param_types, type_params) = self.lower_annotation_params(params);
        let mut result = GoType::new(format!("{}[{}]", base_name, type_params.join(", ")));
        result.merge_all(&param_types);
        result
    }

    /// Lower each annotation param to a `GoType`, returning both the full
    /// `GoType` values (needed for import merging) and their code strings
    /// (needed for type-parameter formatting).
    fn lower_annotation_params(&self, params: &[Annotation]) -> (Vec<GoType>, Vec<String>) {
        let param_types: Vec<GoType> = params
            .iter()
            .map(|p| self.go_type_from_annotation(p))
            .collect();
        let type_params: Vec<String> = param_types.iter().map(|t| t.code.clone()).collect();
        (param_types, type_params)
    }
}
