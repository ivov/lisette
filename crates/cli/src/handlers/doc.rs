use crate::cli_error;
use crate::output::{format_backticks, use_color};
use diagnostics::infer::levenshtein_distance;
use semantics::cache::go_stdlib::{GoModuleCache, try_load_go_stdlib_cache};
use semantics::cache::types::CachedDefinitionBody;
use stdlib::{Target, get_go_stdlib_packages, get_go_stdlib_typedef};
use syntax::ast::{Annotation, Binding, Expression, Generic, Pattern, StructKind, VariantFields};

#[derive(Debug, Clone, Copy)]
enum TypeKind {
    Primitive,
    Struct,
    Enum,
    Interface,
}

#[derive(Debug)]
struct TypeInfo {
    name: String,
    generics: Vec<String>,
    definition: String,
    doc: Option<String>,
    methods: Vec<MethodInfo>,
    kind: TypeKind,
}

#[derive(Debug)]
struct MethodInfo {
    name: String,
    signature: String,
    doc: Option<String>,
}

#[derive(Debug)]
struct FunctionInfo {
    name: String,
    signature: String,
    doc: Option<String>,
}

struct PreludeIndex {
    types: Vec<TypeInfo>,
    functions: Vec<FunctionInfo>,
}

#[derive(Debug)]
struct ConstInfo {
    name: String,
    signature: String,
    doc: Option<String>,
}

#[derive(Debug)]
struct VarInfo {
    name: String,
    signature: String,
    doc: Option<String>,
}

struct GoPackageIndex {
    package: String,
    types: Vec<TypeInfo>,
    functions: Vec<FunctionInfo>,
    constants: Vec<ConstInfo>,
    variables: Vec<VarInfo>,
}

fn annotation_to_string(ann: &Annotation) -> String {
    match ann {
        Annotation::Constructor { name, params, .. } => {
            if params.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    name,
                    params
                        .iter()
                        .map(annotation_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Annotation::Function {
            params,
            return_type,
            ..
        } => {
            let params_str = params
                .iter()
                .map(annotation_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let ret = annotation_to_string(return_type);
            if ret == "Unit" {
                format!("fn({})", params_str)
            } else {
                format!("fn({}) -> {}", params_str, ret)
            }
        }
        Annotation::Tuple { elements, .. } => {
            let inner = elements
                .iter()
                .map(annotation_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", inner)
        }
        Annotation::Unknown => "Unknown".to_string(),
        Annotation::Opaque { .. } => String::new(),
    }
}

fn generics_to_string(generics: &[Generic]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            generics
                .iter()
                .map(|g| g.name.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn binding_to_string(binding: &Binding) -> String {
    let name = match &binding.pattern {
        Pattern::Identifier { identifier, .. } => identifier.to_string(),
        _ => "_".to_string(),
    };
    match &binding.annotation {
        Some(ann) => {
            let annotation_string = annotation_to_string(ann);
            if name == "self" {
                if annotation_string.is_empty() {
                    "self".to_string()
                } else {
                    format!("self: {}", annotation_string)
                }
            } else {
                format!("{}: {}", name, annotation_string)
            }
        }
        None => name,
    }
}

fn function_signature(
    name: &str,
    generics: &[Generic],
    params: &[Binding],
    return_annotation: &Annotation,
) -> String {
    let generics_str = generics_to_string(generics);
    let params_str = params
        .iter()
        .map(binding_to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let ret = match return_annotation {
        Annotation::Unknown => "Unit".to_string(),
        other => annotation_to_string(other),
    };
    format!("fn {}{}({}) -> {}", name, generics_str, params_str, ret)
}

fn struct_definition(
    name: &str,
    gen_str: &str,
    fields: &[syntax::ast::StructFieldDefinition],
    kind: &StructKind,
    show_pub: bool,
) -> String {
    match kind {
        StructKind::Record => {
            if fields.is_empty() {
                format!("struct {}{} {{}}", name, gen_str)
            } else {
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|f| {
                        let vis = if show_pub && f.visibility.is_public() {
                            "pub "
                        } else {
                            ""
                        };
                        format!("{}{}: {}", vis, f.name, annotation_to_string(&f.annotation))
                    })
                    .collect();
                format!("struct {}{} {{ {} }}", name, gen_str, field_strs.join(", "))
            }
        }
        StructKind::Tuple => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|f| annotation_to_string(&f.annotation))
                .collect();
            format!("struct {}{}({})", name, gen_str, field_strs.join(", "))
        }
    }
}

fn enum_definition(name: &str, gen_str: &str, variants: &[syntax::ast::EnumVariant]) -> String {
    let is_compact = variants.len() <= 3
        && variants.iter().all(|v| {
            matches!(&v.fields, VariantFields::Unit)
                || matches!(&v.fields, VariantFields::Tuple(f) if f.len() <= 1)
        });

    if is_compact {
        let compact_variants: Vec<String> = variants
            .iter()
            .map(|v| {
                let fields_str = match &v.fields {
                    VariantFields::Unit => String::new(),
                    VariantFields::Tuple(fields) => {
                        let inner: Vec<String> = fields
                            .iter()
                            .map(|f| annotation_to_string(&f.annotation))
                            .collect();
                        format!("({})", inner.join(", "))
                    }
                    VariantFields::Struct(_) => String::new(),
                };
                format!("{}{}", v.name, fields_str)
            })
            .collect();
        format!(
            "enum {}{} {{ {} }}",
            name,
            gen_str,
            compact_variants.join(", ")
        )
    } else {
        let variant_strs: Vec<String> = variants
            .iter()
            .map(|v| {
                let fields_str = match &v.fields {
                    VariantFields::Unit => String::new(),
                    VariantFields::Tuple(fields) => {
                        let inner: Vec<String> = fields
                            .iter()
                            .map(|f| annotation_to_string(&f.annotation))
                            .collect();
                        format!("({})", inner.join(", "))
                    }
                    VariantFields::Struct(fields) => {
                        let inner: Vec<String> = fields
                            .iter()
                            .map(|f| format!("{}: {}", f.name, annotation_to_string(&f.annotation)))
                            .collect();
                        format!(" {{ {} }}", inner.join(", "))
                    }
                };
                format!("    {}{}", v.name, fields_str)
            })
            .collect();
        format!(
            "enum {}{} {{\n{}\n}}",
            name,
            gen_str,
            variant_strs.join(",\n")
        )
    }
}

fn interface_definition(name: &str, gen_str: &str, method_signatures: &[Expression]) -> String {
    let method_strs: Vec<String> = method_signatures
        .iter()
        .filter_map(|m| {
            if let Expression::Function {
                name: mname,
                generics: mgen,
                params,
                return_annotation,
                ..
            } = m
            {
                Some(function_signature(mname, mgen, params, return_annotation))
            } else {
                None
            }
        })
        .collect();

    format!(
        "interface {}{} {{ {} }}",
        name,
        gen_str,
        method_strs.join(", ")
    )
}

fn build_prelude_index() -> PreludeIndex {
    let source = stdlib::LIS_PRELUDE_SOURCE;
    let parse_result = syntax::parse::Parser::lex_and_parse_file(source, 0);

    let mut types: Vec<TypeInfo> = Vec::new();
    let mut functions: Vec<FunctionInfo> = Vec::new();

    for expression in &parse_result.ast {
        match expression {
            Expression::TypeAlias {
                doc,
                name,
                generics,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = format!("type {}{}", name, gen_str);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Primitive,
                });
            }

            Expression::Struct {
                doc,
                name,
                generics,
                fields,
                kind,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = struct_definition(name, &gen_str, fields, kind, true);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Struct,
                });
            }

            Expression::Enum {
                doc,
                name,
                generics,
                variants,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = enum_definition(name, &gen_str, variants);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Enum,
                });
            }

            Expression::Interface {
                doc,
                name,
                generics,
                method_signatures,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = interface_definition(name, &gen_str, method_signatures);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Interface,
                });
            }

            Expression::ImplBlock {
                receiver_name,
                methods,
                generics,
                annotation,
                ..
            } => {
                let base_name = annotation
                    .get_name()
                    .unwrap_or_else(|| receiver_name.to_string());

                let impl_annotation_str = annotation_to_string(annotation);

                let is_specialized =
                    impl_annotation_str != base_name && !impl_annotation_str.is_empty() && {
                        match annotation {
                            Annotation::Constructor { params, .. } => {
                                params.len() != generics.len()
                                    || params.iter().zip(generics.iter()).any(|(p, g)| {
                                        p.get_name().map(|n| n != g.name.as_str()).unwrap_or(true)
                                    })
                            }
                            _ => false,
                        }
                    };

                let suffix = if is_specialized {
                    format!(" (on {})", impl_annotation_str)
                } else {
                    String::new()
                };

                for method in methods {
                    if let Expression::Function {
                        doc,
                        name,
                        generics: mgen,
                        params,
                        return_annotation,
                        ..
                    } = method
                    {
                        let sig = function_signature(name, mgen, params, return_annotation);
                        if let Some(type_info) = types.iter_mut().find(|t| t.name == base_name) {
                            type_info.methods.push(MethodInfo {
                                name: name.to_string(),
                                signature: sig,
                                doc: doc.clone(),
                            });
                            if !suffix.is_empty()
                                && let Some(last) = type_info.methods.last_mut()
                            {
                                last.signature = format!("{}{}", last.signature, suffix);
                            }
                        }
                    }
                }
            }

            Expression::Function {
                doc,
                name,
                generics,
                params,
                return_annotation,
                ..
            } => {
                let sig = function_signature(name, generics, params, return_annotation);
                functions.push(FunctionInfo {
                    name: name.to_string(),
                    signature: sig,
                    doc: doc.clone(),
                });
            }

            _ => {}
        }
    }

    PreludeIndex { types, functions }
}

fn build_go_package_index(source: &str, package: &str) -> GoPackageIndex {
    let parse_result = syntax::parse::Parser::lex_and_parse_file(source, 0);

    let mut types: Vec<TypeInfo> = Vec::new();
    let mut functions: Vec<FunctionInfo> = Vec::new();
    let mut constants: Vec<ConstInfo> = Vec::new();
    let mut variables: Vec<VarInfo> = Vec::new();

    for expression in &parse_result.ast {
        match expression {
            Expression::TypeAlias {
                doc,
                name,
                generics,
                annotation,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = if let Annotation::Opaque { .. } = annotation {
                    format!("type {}{}", name, gen_str)
                } else {
                    format!(
                        "type {}{} = {}",
                        name,
                        gen_str,
                        annotation_to_string(annotation)
                    )
                };
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Primitive,
                });
            }

            Expression::Struct {
                doc,
                name,
                generics,
                fields,
                kind,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = struct_definition(name, &gen_str, fields, kind, false);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Struct,
                });
            }

            Expression::Enum {
                doc,
                name,
                generics,
                variants,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = enum_definition(name, &gen_str, variants);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Enum,
                });
            }

            Expression::Interface {
                doc,
                name,
                generics,
                method_signatures,
                ..
            } => {
                let gen_names: Vec<String> = generics.iter().map(|g| g.name.to_string()).collect();
                let gen_str = generics_to_string(generics);
                let definition = interface_definition(name, &gen_str, method_signatures);
                types.push(TypeInfo {
                    name: name.to_string(),
                    generics: gen_names,
                    definition,
                    doc: doc.clone(),
                    methods: Vec::new(),
                    kind: TypeKind::Interface,
                });
            }

            Expression::Function {
                doc,
                name,
                generics,
                params,
                return_annotation,
                ..
            } => {
                let sig = function_signature(name, generics, params, return_annotation);
                functions.push(FunctionInfo {
                    name: name.to_string(),
                    signature: sig,
                    doc: doc.clone(),
                });
            }

            Expression::Const {
                doc,
                identifier,
                annotation,
                ..
            } => {
                let sig = if let Some(ann) = annotation {
                    format!("const {}: {}", identifier, annotation_to_string(ann))
                } else {
                    format!("const {}", identifier)
                };
                constants.push(ConstInfo {
                    name: identifier.to_string(),
                    signature: sig,
                    doc: doc.clone(),
                });
            }

            Expression::VariableDeclaration {
                doc,
                name,
                annotation,
                ..
            } => {
                let sig = format!("var {}: {}", name, annotation_to_string(annotation));
                variables.push(VarInfo {
                    name: name.to_string(),
                    signature: sig,
                    doc: doc.clone(),
                });
            }

            Expression::ImplBlock {
                receiver_name,
                methods,
                annotation,
                generics,
                ..
            } => {
                let base_name = annotation
                    .get_name()
                    .unwrap_or_else(|| receiver_name.to_string());

                let impl_annotation_str = annotation_to_string(annotation);

                let is_specialized =
                    impl_annotation_str != base_name && !impl_annotation_str.is_empty() && {
                        match annotation {
                            Annotation::Constructor { params, .. } => {
                                params.len() != generics.len()
                                    || params.iter().zip(generics.iter()).any(|(p, g)| {
                                        p.get_name().map(|n| n != g.name.as_str()).unwrap_or(true)
                                    })
                            }
                            _ => false,
                        }
                    };

                let suffix = if is_specialized {
                    format!(" (on {})", impl_annotation_str)
                } else {
                    String::new()
                };

                for method in methods {
                    if let Expression::Function {
                        doc,
                        name,
                        generics: mgen,
                        params,
                        return_annotation,
                        ..
                    } = method
                    {
                        let sig = function_signature(name, mgen, params, return_annotation);
                        if let Some(type_info) = types.iter_mut().find(|t| t.name == base_name) {
                            type_info.methods.push(MethodInfo {
                                name: name.to_string(),
                                signature: sig,
                                doc: doc.clone(),
                            });
                            if !suffix.is_empty()
                                && let Some(last) = type_info.methods.last_mut()
                            {
                                last.signature = format!("{}{}", last.signature, suffix);
                            }
                        }
                    }
                }
            }

            _ => {}
        }
    }

    GoPackageIndex {
        package: package.to_string(),
        types,
        functions,
        constants,
        variables,
    }
}

fn format_method_name(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.bright_magenta().to_string()
    } else {
        s.to_string()
    }
}

fn colorize_definition(definition: &str) -> String {
    if !use_color() {
        return definition.to_string();
    }
    use owo_colors::OwoColorize;

    let mut result = String::new();
    let chars: Vec<char> = definition.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = chars[i];
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "enum" | "struct" | "type" | "interface" | "fn" | "pub" => {
                    result.push_str(&word.blue().to_string());
                }
                "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16"
                | "uint32" | "uint64" | "uintptr" | "byte" | "bool" | "string" | "rune"
                | "float32" | "float64" | "complex64" | "complex128" | "Unit" | "Unknown"
                | "Never" => {
                    result.push_str(&word.bright_cyan().to_string());
                }
                _ if word.starts_with(char::is_uppercase) => {
                    result.push_str(&word.bright_cyan().to_string());
                }
                _ => result.push_str(&word),
            }
        } else {
            result.push(ch);
            i += 1;
        }
    }

    result
}

fn colorize_signature(sig: &str) -> String {
    if !use_color() {
        return sig.to_string();
    }
    use owo_colors::OwoColorize;

    let mut result = String::new();
    let chars: Vec<char> = sig.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut after_fn_keyword = false;
    let mut fn_name_done = false;

    while i < len {
        let ch = chars[i];
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "fn" => {
                    result.push_str(&word.blue().to_string());
                    after_fn_keyword = true;
                }
                "self" => {
                    result.push_str(&word);
                }
                _ if after_fn_keyword && !fn_name_done => {
                    result.push_str(&word.bright_magenta().to_string());
                    fn_name_done = true;
                }
                _ if word.starts_with(char::is_uppercase) => {
                    result.push_str(&word.bright_cyan().to_string());
                }
                _ => result.push_str(&word),
            }
        } else {
            if after_fn_keyword && !fn_name_done && ch == ' ' {
                result.push(ch);
                i += 1;
                continue;
            }
            result.push(ch);
            i += 1;
        }
    }

    result
}

fn format_type_with_generics_plain(name: &str, generics: &[String]) -> String {
    if generics.is_empty() {
        name.to_string()
    } else {
        format!("{}<{}>", name, generics.join(", "))
    }
}

fn print_all(index: &PreludeIndex) {
    println!();
    println!(
        "  Prelude types and functions. Use {} to learn more.",
        format_method_name("lis doc <type[.method]>")
    );
    println!(
        "  For Go stdlib packages, try e.g. {} or {}",
        format_method_name("lis doc go:os"),
        format_method_name("lis doc go:net/http")
    );

    let primitives: Vec<&TypeInfo> = index
        .types
        .iter()
        .filter(|t| matches!(t.kind, TypeKind::Primitive) && t.generics.is_empty())
        .collect();
    let generic_primitives: Vec<&TypeInfo> = index
        .types
        .iter()
        .filter(|t| matches!(t.kind, TypeKind::Primitive) && !t.generics.is_empty())
        .collect();
    let structs: Vec<&TypeInfo> = index
        .types
        .iter()
        .filter(|t| matches!(t.kind, TypeKind::Struct))
        .collect();
    let enums: Vec<&TypeInfo> = index
        .types
        .iter()
        .filter(|t| matches!(t.kind, TypeKind::Enum))
        .collect();

    println!();
    println!("  Primitive types");

    let signed_types: Vec<&str> = primitives
        .iter()
        .filter(|t| t.name.starts_with("int") || t.name == "rune")
        .map(|t| t.name.as_str())
        .collect();
    if !signed_types.is_empty() {
        println!("    {}", format_dim(&signed_types.join(", ")));
    }

    let unsigned_types: Vec<&str> = primitives
        .iter()
        .filter(|t| t.name.starts_with("uint") || t.name == "uintptr" || t.name == "byte")
        .map(|t| t.name.as_str())
        .collect();
    if !unsigned_types.is_empty() {
        println!("    {}", format_dim(&unsigned_types.join(", ")));
    }

    let float_types: Vec<&str> = primitives
        .iter()
        .filter(|t| t.name.starts_with("float") || t.name.starts_with("complex"))
        .map(|t| t.name.as_str())
        .collect();
    if !float_types.is_empty() {
        println!("    {}", format_dim(&float_types.join(", ")));
    }

    let basic_types: Vec<&str> = primitives
        .iter()
        .filter(|t| matches!(t.name.as_str(), "bool" | "string"))
        .map(|t| t.name.as_str())
        .collect();
    if !basic_types.is_empty() {
        println!("    {}", format_dim(&basic_types.join(", ")));
    }

    println!();
    println!("  Compound types");

    let enums_formatted: Vec<String> = enums
        .iter()
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    if !enums_formatted.is_empty() {
        println!("    {}", format_dim(&enums_formatted.join(", ")));
    }

    let collections: Vec<String> = generic_primitives
        .iter()
        .filter(|t| matches!(t.name.as_str(), "Slice" | "Map"))
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    if !collections.is_empty() {
        println!("    {}", format_dim(&collections.join(", ")));
    }

    let refs: Vec<String> = generic_primitives
        .iter()
        .filter(|t| matches!(t.name.as_str(), "Ref"))
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    if !refs.is_empty() {
        println!("    {}", format_dim(&refs.join(", ")));
    }

    let channels: Vec<String> = generic_primitives
        .iter()
        .filter(|t| matches!(t.name.as_str(), "Channel" | "Sender" | "Receiver"))
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    if !channels.is_empty() {
        println!("    {}", format_dim(&channels.join(", ")));
    }

    let ranges: Vec<String> = structs
        .iter()
        .filter(|t| t.name.contains("Range"))
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    if !ranges.is_empty() {
        println!("    {}", format_dim(&ranges.join(", ")));
    }

    println!();
    println!("  Special types");
    println!(
        "    {}",
        format_dim("Unit, Unknown, Never, VarArgs<T>, PanicValue, error")
    );

    println!();
    println!("  Functions");
    let fn_names: Vec<String> = index
        .functions
        .iter()
        .map(|f| format!("{}()", f.name))
        .collect();
    println!("    {}", format_dim(&fn_names.join(", ")));
}

fn format_dim(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.dimmed().to_string()
    } else {
        s.to_string()
    }
}

fn split_doc_and_example(doc: &str) -> (&str, Option<&str>) {
    if let Some(position) = doc.find("\nExample:\n") {
        let description = doc[..position].trim_end();
        let example = doc[position + "\nExample:\n".len()..].trim_end();
        (description, Some(example))
    } else {
        (doc, None)
    }
}

fn print_example(example: &str) {
    let min_indent = example
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    for line in example.lines() {
        let stripped = if line.len() > min_indent {
            &line[min_indent..]
        } else {
            line.trim_start()
        };
        if use_color() {
            use owo_colors::OwoColorize;
            println!("      {}", stripped.dimmed().italic());
        } else {
            println!("      {}", stripped);
        }
    }
}

fn print_doc(doc: &str) {
    let (description, example) = split_doc_and_example(doc);
    for line in description.lines() {
        println!("    {}", format_backticks(line, use_color()));
    }
    if let Some(example) = example {
        println!();
        print_example(example);
    }
}

fn print_type_header(type_info: &TypeInfo) {
    println!();
    for line in colorize_definition(&type_info.definition).lines() {
        println!("  {}", line);
    }
    if let Some(doc) = &type_info.doc {
        print_doc(doc);
    }
}

fn print_type(type_info: &TypeInfo) {
    print_type_header(type_info);
    for method in &type_info.methods {
        println!();
        println!("    {}", colorize_signature(&method.signature));
        if let Some(doc) = &method.doc {
            for line in doc.lines() {
                println!("      {}", format_backticks(line, use_color()));
            }
        }
    }
}

fn print_method(type_info: &TypeInfo, method: &MethodInfo) {
    print_type_header(type_info);
    println!();
    println!("    {}", colorize_signature(&method.signature));
    if let Some(doc) = &method.doc {
        for line in doc.lines() {
            println!("      {}", format_backticks(line, use_color()));
        }
    }
}

fn print_function(func: &FunctionInfo) {
    println!();
    println!("  {}", colorize_signature(&func.signature));
    if let Some(doc) = &func.doc {
        print_doc(doc);
    }
}

fn suggest_type_or_function<'a>(query: &str, index: &'a PreludeIndex) -> Option<&'a str> {
    let all_names = index
        .types
        .iter()
        .map(|t| t.name.as_str())
        .chain(index.functions.iter().map(|f| f.name.as_str()));

    all_names
        .filter(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()) <= 2)
        .min_by_key(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()))
}

fn suggest_method<'a>(query: &str, type_info: &'a TypeInfo) -> Option<&'a str> {
    type_info
        .methods
        .iter()
        .map(|m| m.name.as_str())
        .filter(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()) <= 2)
        .min_by_key(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()))
}

fn print_go_package_header(package: &str) {
    println!();
    if use_color() {
        use owo_colors::OwoColorize;
        println!(
            "  {} {}",
            "package".blue(),
            format!("go:{}", package).bright_cyan()
        );
    } else {
        println!("  package go:{}", package);
    }
}

fn print_go_package_all(index: &GoPackageIndex) {
    print_go_package_header(&index.package);
    println!();
    println!(
        "  Use {} to learn more about a specific item.",
        format_method_name(&format!("lis doc go:{}.<item>", index.package))
    );

    if !index.types.is_empty() {
        println!();
        println!("  Types");
        for ti in &index.types {
            println!("    {}", format_dim(&ti.name));
        }
    }

    if !index.functions.is_empty() {
        println!();
        println!("  Functions");
        let fn_names: Vec<String> = index
            .functions
            .iter()
            .map(|f| format!("{}()", f.name))
            .collect();
        let mut line = String::from("    ");
        for (i, name) in fn_names.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            if line.len() + name.len() > 80 {
                println!("{}", format_dim(&line));
                line = String::from("    ");
            }
            line.push_str(name);
        }
        if line.len() > 4 {
            println!("{}", format_dim(&line));
        }
    }

    if !index.constants.is_empty() {
        println!();
        println!("  Constants");
        let const_names: Vec<&str> = index.constants.iter().map(|c| c.name.as_str()).collect();
        let mut line = String::from("    ");
        for (i, name) in const_names.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            if line.len() + name.len() > 80 {
                println!("{}", format_dim(&line));
                line = String::from("    ");
            }
            line.push_str(name);
        }
        if line.len() > 4 {
            println!("{}", format_dim(&line));
        }
    }

    if !index.variables.is_empty() {
        println!();
        println!("  Variables");
        let var_names: Vec<&str> = index.variables.iter().map(|v| v.name.as_str()).collect();
        let mut line = String::from("    ");
        for (i, name) in var_names.iter().enumerate() {
            if i > 0 {
                line.push_str(", ");
            }
            if line.len() + name.len() > 80 {
                println!("{}", format_dim(&line));
                line = String::from("    ");
            }
            line.push_str(name);
        }
        if line.len() > 4 {
            println!("{}", format_dim(&line));
        }
    }
}

fn print_go_type(index: &GoPackageIndex, type_info: &TypeInfo) {
    print_go_package_header(&index.package);
    print_type(type_info);
}

fn print_go_function(index: &GoPackageIndex, func: &FunctionInfo) {
    print_go_package_header(&index.package);
    print_function(func);
}

fn print_go_const(index: &GoPackageIndex, c: &ConstInfo) {
    print_go_package_header(&index.package);
    println!();
    println!("  {}", colorize_signature(&c.signature));
    if let Some(doc) = &c.doc {
        print_doc(doc);
    }
}

fn print_go_var(index: &GoPackageIndex, v: &VarInfo) {
    print_go_package_header(&index.package);
    println!();
    println!("  {}", colorize_signature(&v.signature));
    if let Some(doc) = &v.doc {
        print_doc(doc);
    }
}

fn suggest_go_item<'a>(query: &str, index: &'a GoPackageIndex) -> Option<&'a str> {
    let all_names = index
        .types
        .iter()
        .map(|t| t.name.as_str())
        .chain(index.functions.iter().map(|f| f.name.as_str()))
        .chain(index.constants.iter().map(|c| c.name.as_str()))
        .chain(index.variables.iter().map(|v| v.name.as_str()));

    all_names
        .filter(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()) <= 2)
        .min_by_key(|name| levenshtein_distance(&query.to_lowercase(), &name.to_lowercase()))
}

fn suggest_go_package(query: &str) -> Option<&'static str> {
    let packages = get_go_stdlib_packages(Target::host());
    packages
        .into_iter()
        .filter(|pkg| levenshtein_distance(&query.to_lowercase(), &pkg.to_lowercase()) <= 2)
        .min_by_key(|pkg| levenshtein_distance(&query.to_lowercase(), &pkg.to_lowercase()))
}

fn print_go_packages_list() {
    println!();
    println!(
        "  Go stdlib packages. Use {} to learn more.",
        format_method_name("lis doc go:<package>")
    );
    println!();

    let packages = get_go_stdlib_packages(Target::host());

    let max_width = packages.iter().map(|p| p.len()).max().unwrap_or(0);
    let col_width = max_width + 2;
    let term_width = 80;
    let cols = ((term_width - 2) / col_width).max(1);

    for chunk in packages.chunks(cols) {
        print!("  ");
        for (i, pkg) in chunk.iter().enumerate() {
            if i < chunk.len() - 1 {
                let padded = format!("{:width$}", pkg, width = col_width);
                print!("{}", format_dim(&padded));
            } else {
                print!("{}", format_dim(pkg));
            }
        }
        println!();
    }
}

fn doc_go_package(query: &str) -> i32 {
    let without_prefix = query.strip_prefix("go:").unwrap_or(query);

    if without_prefix.is_empty() {
        print_go_packages_list();
        return 0;
    }

    let parts: Vec<&str> = without_prefix.splitn(2, '.').collect();
    let package = parts[0];
    let item_name = parts.get(1).copied();

    let Some(source) = get_go_stdlib_typedef(package, Target::host()) else {
        let help = if let Some(s) = suggest_go_package(package) {
            format!("Did you mean `lis doc go:{}`?", s)
        } else {
            "Run `lis doc go:` to see available Go packages".to_string()
        };
        cli_error!(
            format!("`go:{}` is not a known Go stdlib package", package),
            "The package name does not match any Go stdlib package",
            help
        );
        return 1;
    };

    let index = build_go_package_index(source, package);

    match item_name {
        None => {
            print_go_package_all(&index);
            0
        }
        Some(item) => {
            if let Some(ti) = index
                .types
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(item))
            {
                print_go_type(&index, ti);
                return 0;
            }

            if let Some(fi) = index
                .functions
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(item))
            {
                print_go_function(&index, fi);
                return 0;
            }

            if let Some(ci) = index
                .constants
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(item))
            {
                print_go_const(&index, ci);
                return 0;
            }

            if let Some(vi) = index
                .variables
                .iter()
                .find(|v| v.name.eq_ignore_ascii_case(item))
            {
                print_go_var(&index, vi);
                return 0;
            }

            let help = if let Some(s) = suggest_go_item(item, &index) {
                format!("Did you mean `lis doc go:{}.{}`?", package, s)
            } else {
                format!("Run `lis doc go:{}` to see available items", package)
            };

            cli_error!(
                format!("`{}` is not found in `go:{}`", item, package),
                format!(
                    "The name does not match any type, function, constant, or variable in `go:{}`",
                    package
                ),
                help
            );
            1
        }
    }
}

fn has_go_module_matches(module_cache: &GoModuleCache, query_lower: &str) -> bool {
    for (def_name, def) in &module_cache.definitions {
        if matches!(def.body, CachedDefinitionBody::Value { .. })
            && def_name.to_lowercase().contains(query_lower)
        {
            return true;
        }
        match &def.body {
            CachedDefinitionBody::TypeAlias { methods, .. }
            | CachedDefinitionBody::Enum { methods, .. }
            | CachedDefinitionBody::ValueEnum { methods, .. }
            | CachedDefinitionBody::Struct { methods, .. } => {
                if methods
                    .keys()
                    .any(|m| m.to_lowercase().contains(query_lower))
                {
                    return true;
                }
            }
            CachedDefinitionBody::Interface { definition, .. } => {
                if definition
                    .methods
                    .keys()
                    .any(|m| m.to_lowercase().contains(query_lower))
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn format_search_line(qualifier: &str, func_name: &str, signature: &str) -> String {
    let without_fn = signature.strip_prefix("fn ").unwrap_or(signature);
    let name_end = without_fn.find(['(', '<']).unwrap_or(without_fn.len());
    let after_name = &without_fn[name_end..];

    if use_color() {
        use owo_colors::OwoColorize;
        let colored_rest = colorize_definition(after_name);
        if qualifier.is_empty() {
            format!("    {}{}", func_name.bright_magenta(), colored_rest)
        } else {
            format!(
                "    {}.{}{}",
                qualifier,
                func_name.bright_magenta(),
                colored_rest
            )
        }
    } else if qualifier.is_empty() {
        format!("    {}{}", func_name, after_name)
    } else {
        format!("    {}.{}{}", qualifier, func_name, after_name)
    }
}

struct SearchMatch {
    name: String,
    display: String,
    doc_path: String,
}

pub fn doc_search(query: &str) -> i32 {
    if query.is_empty() {
        cli_error!(
            "missing search query",
            "`lis doc -s` requires a search term",
            "Try e.g. `lis doc -s split` or `lis doc -s contains`"
        );
        return 1;
    }

    let query_lower = query.to_lowercase();

    let prelude_index = build_prelude_index();
    let mut prelude_matches: Vec<SearchMatch> = Vec::new();

    for ti in &prelude_index.types {
        let type_qual = format_type_with_generics_plain(&ti.name, &ti.generics);
        for mi in &ti.methods {
            if mi.name.to_lowercase().contains(&query_lower) {
                prelude_matches.push(SearchMatch {
                    name: mi.name.clone(),
                    display: format_search_line(&type_qual, &mi.name, &mi.signature),
                    doc_path: format!("{}.{}", ti.name, mi.name),
                });
            }
        }
    }
    for fi in &prelude_index.functions {
        if fi.name.to_lowercase().contains(&query_lower) {
            prelude_matches.push(SearchMatch {
                name: fi.name.clone(),
                display: format_search_line("", &fi.name, &fi.signature),
                doc_path: fi.name.clone(),
            });
        }
    }

    let mut go_matches: Vec<SearchMatch> = Vec::new();
    let target = Target::host();
    let go_cache = try_load_go_stdlib_cache(target);

    for pkg in get_go_stdlib_packages(target) {
        if let Some(ref cache) = go_cache {
            let module_id = format!("go:{}", pkg);
            if let Some(module_cache) = cache.modules.get(&module_id)
                && !has_go_module_matches(module_cache, &query_lower)
            {
                continue;
            }
        }

        let Some(source) = get_go_stdlib_typedef(pkg, target) else {
            continue;
        };
        let index = build_go_package_index(source, pkg);

        for fi in &index.functions {
            if fi.name.to_lowercase().contains(&query_lower) {
                go_matches.push(SearchMatch {
                    name: fi.name.clone(),
                    display: format_search_line(pkg, &fi.name, &fi.signature),
                    doc_path: format!("{}.{}", pkg, fi.name),
                });
            }
        }
        for ti in &index.types {
            let type_qual = format!("{}.{}", pkg, ti.name);
            for mi in &ti.methods {
                if mi.name.to_lowercase().contains(&query_lower) {
                    go_matches.push(SearchMatch {
                        name: mi.name.clone(),
                        display: format_search_line(&type_qual, &mi.name, &mi.signature),
                        doc_path: format!("{}.{}.{}", pkg, ti.name, mi.name),
                    });
                }
            }
        }
    }

    let rank = |name: &str| -> u8 {
        let lower = name.to_lowercase();
        if lower == query_lower {
            0
        } else if lower.starts_with(&query_lower) {
            1
        } else {
            2
        }
    };
    prelude_matches.sort_by_cached_key(|m| rank(&m.name));
    go_matches.sort_by_cached_key(|m| rank(&m.name));

    if prelude_matches.is_empty() && go_matches.is_empty() {
        println!();
        println!("  No matches found.");

        let mut best_name = String::new();
        let mut best_dist = usize::MAX;

        for ti in &prelude_index.types {
            for mi in &ti.methods {
                let d = levenshtein_distance(&query_lower, &mi.name.to_lowercase());
                if d <= 2 && d < best_dist {
                    best_name = mi.name.clone();
                    best_dist = d;
                }
            }
        }
        for fi in &prelude_index.functions {
            let d = levenshtein_distance(&query_lower, &fi.name.to_lowercase());
            if d <= 2 && d < best_dist {
                best_name = fi.name.clone();
                best_dist = d;
            }
        }

        if best_dist > 0 {
            'outer: for pkg in get_go_stdlib_packages(target) {
                let Some(source) = get_go_stdlib_typedef(pkg, target) else {
                    continue;
                };
                let index = build_go_package_index(source, pkg);
                for fi in &index.functions {
                    let d = levenshtein_distance(&query_lower, &fi.name.to_lowercase());
                    if d <= 2 && d < best_dist {
                        best_name = fi.name.clone();
                        best_dist = d;
                        if d == 0 {
                            break 'outer;
                        }
                    }
                }
                for ti in &index.types {
                    for mi in &ti.methods {
                        let d = levenshtein_distance(&query_lower, &mi.name.to_lowercase());
                        if d <= 2 && d < best_dist {
                            best_name = mi.name.clone();
                            best_dist = d;
                            if d == 0 {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        if best_dist <= 2 {
            println!();
            println!(
                "  hint: Did you mean {}?",
                format_method_name(&format!("lis doc -s {}", best_name))
            );
        } else {
            println!();
            println!(
                "  hint: Run {} to browse prelude types, or {} to list Go packages",
                format_method_name("lis doc"),
                format_method_name("lis doc go:")
            );
        }
        return 0;
    }

    println!();
    println!("  Prelude");
    if prelude_matches.is_empty() {
        println!("    {}", format_dim("(no matches)"));
    } else {
        for m in &prelude_matches {
            println!("{}", m.display);
        }
    }

    println!();
    println!("  Go stdlib");
    if go_matches.is_empty() {
        println!("    {}", format_dim("(no matches)"));
    } else {
        let cap = 10;
        for m in go_matches.iter().take(cap) {
            println!("{}", m.display);
        }
        if go_matches.len() > cap {
            println!(
                "    {}",
                format_dim(&format!("... and {} more", go_matches.len() - cap))
            );
        }
    }

    let total = prelude_matches.len() + go_matches.len();
    if total <= 20 {
        let prelude_example = prelude_matches.first().map(|m| &m.doc_path);
        let go_example = go_matches
            .iter()
            .find(|m| m.doc_path.matches('.').count() == 1)
            .or(go_matches.first());

        let hints: Vec<String> = prelude_example
            .map(|p| format_method_name(&format!("lis doc {}", p)))
            .into_iter()
            .chain(go_example.map(|m| format_method_name(&format!("lis doc go:{}", m.doc_path))))
            .collect();

        if !hints.is_empty() {
            println!();
            println!("  hint: Run {} to learn more", hints.join(" or "));
        }
    }

    println!();
    0
}

pub fn doc(query: Option<String>) -> i32 {
    match query {
        None => {
            let index = build_prelude_index();
            print_all(&index);
            0
        }
        Some(q) if q.starts_with("go:") => doc_go_package(&q),
        Some(q) => {
            let index = build_prelude_index();
            let parts: Vec<&str> = q.splitn(2, '.').collect();
            let type_name = parts[0];
            let method_name = parts.get(1).copied();

            let type_info = index
                .types
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(type_name));

            if method_name.is_none()
                && let Some(func) = index
                    .functions
                    .iter()
                    .find(|f| f.name.eq_ignore_ascii_case(type_name))
            {
                print_function(func);
                return 0;
            }

            match (type_info, method_name) {
                (Some(ti), None) => {
                    print_type(ti);
                    0
                }
                (Some(ti), Some(method)) => {
                    if let Some(mi) = ti
                        .methods
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(method))
                    {
                        print_method(ti, mi);
                        0
                    } else {
                        let help = if let Some(s) = suggest_method(method, ti) {
                            format!("Did you mean `lis doc {}.{}`?", ti.name, s)
                        } else {
                            format!("Run `lis doc {}` to see available methods", ti.name)
                        };
                        cli_error!(
                            format!("`{}` has no method `{}`", ti.name, method),
                            format!("`{}` is not a method on `{}`", method, ti.name),
                            help
                        );
                        1
                    }
                }
                (None, Some(_)) => {
                    let help = if let Some(s) = suggest_type_or_function(type_name, &index) {
                        format!("Did you mean `{}`?", s)
                    } else {
                        "Run `lis doc` to see available prelude types".to_string()
                    };
                    cli_error!(
                        format!("`{}` is not a prelude type", type_name),
                        "The name does not match any type in the prelude",
                        help
                    );
                    1
                }
                (None, None) => {
                    let help = if let Some(s) = suggest_type_or_function(type_name, &index) {
                        format!("Did you mean `{}`?", s)
                    } else {
                        "Run `lis doc` to see available prelude types and functions".to_string()
                    };
                    cli_error!(
                        format!("`{}` is not a prelude type or function", type_name),
                        "The name does not match any type or function in the prelude",
                        help
                    );
                    1
                }
            }
        }
    }
}
