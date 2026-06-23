use crate::cli_error;
use crate::output::{format_backticks, use_color};
use diagnostics::infer::levenshtein_distance;
use semantics::cache::go_stdlib::{GoModuleCache, try_load_go_stdlib_cache};
use semantics::cache::types::CachedDefinitionBody;
use stdlib::{
    Target, format_targets, get_go_stdlib_package_targets, get_go_stdlib_packages,
    get_go_stdlib_typedef,
};
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

fn build_index_from_source(source: &str) -> PreludeIndex {
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

fn build_prelude_index() -> PreludeIndex {
    let mut index = build_index_from_source(stdlib::LIS_PRELUDE_SOURCE);
    index.types.push(TypeInfo {
        name: "Unit".to_string(),
        generics: Vec::new(),
        definition: "type Unit".to_string(),
        doc: Some(
            "The type of `()`, returned when there is no meaningful value to produce.".to_string(),
        ),
        methods: Vec::new(),
        kind: TypeKind::Primitive,
    });
    index
}

fn build_test_prelude_index() -> PreludeIndex {
    build_index_from_source(stdlib::LIS_TEST_PRELUDE_SOURCE)
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
    if !use_color() {
        return s.to_string();
    }
    use owo_colors::OwoColorize;
    let mut out = String::new();
    let mut buf = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            if !buf.is_empty() {
                out.push_str(&buf.bright_magenta().to_string());
                buf.clear();
            }
            let mut inner = String::from("<");
            for ic in chars.by_ref() {
                inner.push(ic);
                if ic == '>' {
                    break;
                }
            }
            out.push_str(&inner.green().to_string());
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push_str(&buf.bright_magenta().to_string());
    }
    out
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
    println!("  Browse documentation on a symbol from the prelude or from the Go stdlib");

    println!();
    println!("  Examples:");
    let examples = [
        ("Slice", "Docs on Lisette's `Slice` type"),
        (
            "Slice.map",
            "Docs on `map` method on Lisette's `Slice` type",
        ),
        ("prelude", "List all Lisette prelude symbols"),
        ("test", "How to write and run tests"),
        ("go:", "List all Go stdlib packages"),
        ("go:os", "Docs on Go stdlib `os` package contents"),
        (
            "go:os.File",
            "Docs on `File` type in Go stdlib `os` package",
        ),
        ("-s append", "Search docs for `append`"),
    ];
    let arg_width = examples.iter().map(|(arg, _)| arg.len()).max().unwrap_or(0) + 4;
    for (arg, description) in examples {
        let padding = " ".repeat(arg_width - arg.len());
        let description = format_backticks(description, use_color());
        if use_color() {
            use owo_colors::OwoColorize;
            println!(
                "    {} {}{}{}",
                "lis doc".bright_magenta(),
                format_doc_arg(arg),
                padding,
                description
            );
        } else {
            println!("    lis doc {arg}{padding}{description}");
        }
    }

    let label_width = print_prelude_section(index);

    println!();
    println!("  Go stdlib:");
    print_go_stdlib_section(label_width);
}

fn print_prelude_section(index: &PreludeIndex) -> usize {
    let categories = prelude_categories(index);
    let label_width = categories
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 3;
    println!();
    println!("  Prelude:");
    for (label, leaves) in &categories {
        print_prelude_category(label, label_width, leaves);
    }
    label_width
}

fn print_prelude(index: &PreludeIndex) {
    println!();
    println!("  Lisette's prelude");
    println!();
    println!(
        "  Run {} to view a symbol",
        format_method_name("lis doc <symbol>")
    );
    println!();

    let categories = prelude_categories(index);
    let label_width = categories
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 3;
    for (label, leaves) in &categories {
        print_prelude_category(label, label_width, leaves);
    }
}

fn print_test_code(code: &str) {
    let lines: Vec<&str> = code.lines().collect();
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    println!();
    for line in lines {
        let stripped = if line.len() >= min_indent {
            &line[min_indent..]
        } else {
            line.trim_start()
        };
        if use_color() {
            use owo_colors::OwoColorize;
            println!("      {}", stripped.dimmed().italic());
        } else {
            println!("      {stripped}");
        }
    }
}

fn print_test_topic() {
    let paragraph = |text: &str| {
        for line in text.lines() {
            println!("  {}", format_backticks(line, use_color()));
        }
    };

    println!();
    paragraph("Write and run tests for a Lisette project.");
    println!();
    paragraph(
        "Tests live in `.test.lis` files beside the code they cover. Mark a\n\
         function with `#[test]` and check it with `assert`:",
    );
    print_test_code(
        "        #[test]
        fn adds_two_numbers() {
          assert add(2, 3) == 5
        }",
    );
    println!();
    paragraph("Bind and check a pattern in one step with `let assert`:");
    print_test_code(
        "        #[test]
        fn parses_a_port() {
          let assert Some(port) = parse_port(\"8080\")
          assert port == 8080
        }",
    );
    println!();
    paragraph("Give a test a readable title, and a doc comment for context:");
    print_test_code(
        "        /// A zero-length input is rejected before any work begins.
        #[test(\"rejects empty input\")]
        fn rejects_empty() {
          let assert Err(message) = run(\"\")
          assert message.contains(\"empty\")
        }",
    );
    println!();
    paragraph(
        "Return `Result<(), error>` to use `?` inside a test. Returning `Err`\n\
         fails it:",
    );
    print_test_code(
        "        #[test]
        fn loads_config() -> Result<(), error> {
          let config = load(\"app.toml\")?
          assert config.port == 8080
          Ok(())
        }",
    );

    println!();
    println!("  {}", format_bold("The test handle"));
    println!();
    paragraph("Take a `t` parameter to reach the test context, for subtests and more:");
    print_test_code(
        "        #[test]
        fn groups_cases(t) {
          let _ = t.run(\"small inputs\", |t| {
            assert normalize(1) == 1
          })
        }",
    );

    if let Some(type_info) = build_test_prelude_index()
        .types
        .iter()
        .find(|t| t.name == "TestContext")
    {
        print_type(type_info);
    }

    println!();
    paragraph("Run them with `lis test`, or `lis test --help` for filters and flags.");
    println!();
}

fn prelude_categories(index: &PreludeIndex) -> Vec<(&'static str, Vec<String>)> {
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

    let pick_primitives = |predicate: &dyn Fn(&str) -> bool| -> Vec<String> {
        primitives
            .iter()
            .filter(|t| predicate(&t.name))
            .map(|t| t.name.clone())
            .collect()
    };
    let mut primitive_leaves = pick_primitives(&|n| n.starts_with("int") || n == "rune");
    primitive_leaves.extend(pick_primitives(&|n| {
        n.starts_with("uint") || n == "uintptr" || n == "byte"
    }));
    primitive_leaves.extend(pick_primitives(&|n| {
        n.starts_with("float") || n.starts_with("complex")
    }));
    primitive_leaves.extend(pick_primitives(&|n| n == "bool" || n == "string"));

    let mut composite_leaves: Vec<String> = enums
        .iter()
        .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
        .collect();
    let pick_generic = |names: &[&str]| -> Vec<String> {
        generic_primitives
            .iter()
            .filter(|t| names.contains(&t.name.as_str()))
            .map(|t| format_type_with_generics_plain(&t.name, &t.generics))
            .collect()
    };
    composite_leaves.extend(pick_generic(&["Slice", "Map"]));
    composite_leaves.extend(pick_generic(&["Ref"]));
    composite_leaves.extend(pick_generic(&["Channel", "Sender", "Receiver"]));
    composite_leaves.extend(
        structs
            .iter()
            .filter(|t| t.name.contains("Range"))
            .map(|t| format_type_with_generics_plain(&t.name, &t.generics)),
    );

    let function_leaves: Vec<String> = index
        .functions
        .iter()
        .map(|f| format!("{}()", f.name))
        .collect();

    let constraint_leaves: Vec<String> = ["Comparable", "Ordered"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let special_leaves: Vec<String> = [
        "Unit",
        "Unknown",
        "Never",
        "VarArgs<T>",
        "PanicValue",
        "error",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    vec![
        ("primitives", primitive_leaves),
        ("composites", composite_leaves),
        ("functions", function_leaves),
        ("others", special_leaves),
        ("constraints", constraint_leaves),
    ]
    .into_iter()
    .filter(|(_, leaves)| !leaves.is_empty())
    .collect()
}

const WRAP_WIDTH: usize = 78;

fn wrap_into_lines(items: &[String], area: usize) -> Vec<Vec<&String>> {
    let mut lines: Vec<Vec<&String>> = Vec::new();
    let mut current: Vec<&String> = Vec::new();
    let mut current_width = 0;
    for item in items {
        let item_width = item.chars().count();
        let separator = if current.is_empty() { 0 } else { 3 };
        if !current.is_empty() && current_width + separator + item_width > area {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current_width += if current.is_empty() {
            item_width
        } else {
            3 + item_width
        };
        current.push(item);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn print_prelude_category(label: &str, label_width: usize, leaves: &[String]) {
    let area = WRAP_WIDTH.saturating_sub(4 + label_width).max(20);
    let lines = wrap_into_lines(leaves, area);

    for (line_index, line) in lines.iter().enumerate() {
        let rendered = line
            .iter()
            .map(|leaf| format_magenta(leaf))
            .collect::<Vec<_>>()
            .join(" · ");
        let trailing = if line_index + 1 < lines.len() {
            " ·"
        } else {
            ""
        };
        if line_index == 0 {
            let padded_label = format_dim(&format!("{label:<label_width$}"));
            println!("    {padded_label}{rendered}{trailing}");
        } else {
            println!("    {}{rendered}{trailing}", " ".repeat(label_width));
        }
    }
}

fn print_go_stdlib_section(label_width: usize) {
    let area = WRAP_WIDTH.saturating_sub(4 + label_width).max(20);
    let label = "packages";

    let mut items: Vec<String> = [
        "fmt", "os", "io", "bufio", "bytes", "strings", "strconv", "slices", "maps", "errors",
        "time", "context", "sync", "regexp",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    items.push("etc".to_string());
    let lines = wrap_into_lines(&items, area);

    for (line_index, line) in lines.iter().enumerate() {
        let rendered = line
            .iter()
            .map(|item| {
                if item.as_str() == "etc" {
                    format_white(item)
                } else {
                    format_magenta(item)
                }
            })
            .collect::<Vec<_>>()
            .join(" · ");
        let trailing = if line_index + 1 < lines.len() {
            " ·"
        } else {
            ""
        };
        if line_index == 0 {
            let padded_label = format_dim(&format!("{label:<label_width$}"));
            println!("    {padded_label}{rendered}{trailing}");
        } else {
            println!("    {}{rendered}{trailing}", " ".repeat(label_width));
        }
    }
}

fn format_dim(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.dimmed().to_string()
    } else {
        s.to_string()
    }
}

fn format_magenta(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.bright_magenta().to_string()
    } else {
        s.to_string()
    }
}

fn format_white(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.white().to_string()
    } else {
        s.to_string()
    }
}

fn format_bold(s: &str) -> String {
    if use_color() {
        use owo_colors::OwoColorize;
        s.bold().to_string()
    } else {
        s.to_string()
    }
}

fn format_doc_arg(arg: &str) -> String {
    if !use_color() {
        return arg.to_string();
    }
    use owo_colors::OwoColorize;
    arg.split(' ')
        .map(|token| {
            if token.starts_with('-') {
                token.blue().to_string()
            } else {
                token.green().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    println!();
    println!(
        "  Run {} to view an item",
        format_method_name(&format!("lis doc go:{}.<item>", index.package))
    );

    let types: Vec<String> = index.types.iter().map(|t| t.name.clone()).collect();
    let functions: Vec<String> = index
        .functions
        .iter()
        .map(|f| format!("{}()", f.name))
        .collect();
    let constants: Vec<String> = index.constants.iter().map(|c| c.name.clone()).collect();
    let variables: Vec<String> = index.variables.iter().map(|v| v.name.clone()).collect();

    let categories: Vec<(&str, Vec<String>)> = vec![
        ("types", types),
        ("functions", functions),
        ("constants", constants),
        ("variables", variables),
    ]
    .into_iter()
    .filter(|(_, items)| !items.is_empty())
    .collect();

    let label_width = categories
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0)
        + 3;

    println!();
    for (label, items) in &categories {
        print_prelude_category(label, label_width, items);
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
    println!("  Go stdlib");
    println!();
    println!(
        "  Run {} to view a package",
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
                print!("{}", format_magenta(&padded));
            } else {
                print!("{}", format_magenta(pkg));
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

    if without_prefix.contains('/') && deps::is_third_party(without_prefix) {
        cli_error!(
            format!(
                "`go:{}` is a third-party module, not a Go stdlib package",
                without_prefix
            ),
            "`lis doc` browses only the Lisette prelude and the Go stdlib",
            "You can browse generated typedefs for a third-party module under `target/.lisette/typedefs/`"
        );
        return 1;
    }

    let parts: Vec<&str> = without_prefix.splitn(2, '.').collect();
    let package = parts[0];
    let item_name = parts.get(1).copied();

    let host = Target::host();
    let Some(source) = get_go_stdlib_typedef(package, host) else {
        if let Some(targets) = get_go_stdlib_package_targets(package) {
            cli_error!(
                format!("`go:{}` is not available on `{}`", package, host),
                "This Go stdlib package exists, but its surface differs across platforms and your host is not in the supported set",
                format!("Available on: {}", format_targets(targets))
            );
            return 1;
        }
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
    println!("  Prelude:");
    if prelude_matches.is_empty() {
        println!("    {}", format_dim("(no matches)"));
    } else {
        for m in &prelude_matches {
            println!("{}", m.display);
        }
    }

    println!();
    println!("  Go stdlib:");
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
        Some(q) if q.eq_ignore_ascii_case("prelude") => {
            let index = build_prelude_index();
            print_prelude(&index);
            0
        }
        Some(q) if q.eq_ignore_ascii_case("test") || q.eq_ignore_ascii_case("tests") => {
            print_test_topic();
            0
        }
        Some(q)
            if q.eq_ignore_ascii_case("TestContext")
                || q.to_lowercase().starts_with("testcontext.") =>
        {
            let index = build_test_prelude_index();
            let Some(type_info) = index.types.iter().find(|t| t.name == "TestContext") else {
                print_test_topic();
                return 0;
            };
            match q.split_once('.') {
                None => {
                    print_type(type_info);
                    0
                }
                Some((_, method)) => {
                    if let Some(method_info) = type_info
                        .methods
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(method))
                    {
                        print_method(type_info, method_info);
                        0
                    } else {
                        let help = match suggest_method(method, type_info) {
                            Some(s) => format!("Did you mean `lis doc TestContext.{}`?", s),
                            None => "Run `lis doc test` to see the test handle".to_string(),
                        };
                        cli_error!(
                            format!("`TestContext` has no method `{}`", method),
                            format!("`{}` is not a method on `TestContext`", method),
                            help
                        );
                        1
                    }
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prelude_exposes_the_handle_methods() {
        let index = build_test_prelude_index();
        let context = index
            .types
            .iter()
            .find(|t| t.name == "TestContext")
            .expect("`TestContext` is parsed from the test prelude");

        let methods: Vec<&str> = context.methods.iter().map(|m| m.name.as_str()).collect();
        for expected in ["run", "parallel", "skip", "log"] {
            assert!(
                methods.contains(&expected),
                "the `lis doc test` handle section documents `{expected}`, got: {methods:?}"
            );
        }
    }
}
