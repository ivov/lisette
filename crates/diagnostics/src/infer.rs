use crate::LisetteDiagnostic;
use syntax::ast::{Annotation, BinaryOperator, Span};
use syntax::types::{SimpleKind, Type};

pub fn blank_import_non_go(blank_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid import")
        .with_resolve_code("blank_import_non_go")
        .with_span_label(&blank_span, "only allowed for Go modules")
        .with_help(
            "Remove the underscore. Blank imports are allowed only for Go imports, \
             because Lisette modules have no `init()` side effects.",
        )
}

pub fn import_conflict(
    alias: &str,
    path1: &str,
    path2: &str,
    name_span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Import conflict")
        .with_resolve_code("import_conflict")
        .with_span_label(
            &name_span,
            format!("conflicts with prior import `{}`", alias),
        )
        .with_help(format!(
            "`{}` and `{}` resolve to the same name. Add an alias to at least one of them: \
             `import my_{} \"{}\"`",
            path1, path2, alias, path2
        ))
}

pub fn reserved_import_alias(alias: &str, alias_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved import alias")
        .with_resolve_code("reserved_import_alias")
        .with_span_label(&alias_span, "reserved name")
        .with_help(format!(
            "`{}` is a reserved name and cannot be used as an import alias. \
             Choose a different alias, e.g. `import my_{} \"...\"`",
            alias, alias
        ))
}

pub fn duplicate_import_path(path: &str, name_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate import")
        .with_resolve_code("duplicate_import")
        .with_span_label(&name_span, "already imported above")
        .with_help(format!(
            "Module `{}` is already imported. Remove the duplicate import.",
            path
        ))
}

pub fn definition_shadows_import(
    name: &str,
    import_path: &str,
    name_span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Definition shadows import")
        .with_resolve_code("definition_shadows_import")
        .with_span_label(
            &name_span,
            format!("conflicts with imported module `{}`", import_path),
        )
        .with_help(format!(
            "`{}` is already used as a module alias for `{}`. \
             Rename this definition or use a different import alias.",
            name, import_path
        ))
}

pub fn statement_as_tail(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Statement used as value")
        .with_infer_code("statement_as_tail")
        .with_span_label(&span, "this is a statement, not an expression")
        .with_help(
            "The last item in this block must be an expression that produces a value. \
             Statements like `let`, `=`, `task`, and `defer` do not produce values.",
        )
}

pub fn invalid_map_initialization(key: &Type, value: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `Map` initialization")
        .with_infer_code("invalid_map_initialization")
        .with_span_label(&span, "invalid syntax")
        .with_help(format!(
            "To initialize a `Map`, use `Map.new<{}, {}>()`",
            key, value
        ))
}

pub fn self_type_not_supported(span: Span) -> LisetteDiagnostic {
    let name_span = Span::new(span.file_id, span.byte_offset, 4); // "Self" is 4 chars
    LisetteDiagnostic::error("Use of `Self` type")
        .with_resolve_code("self_type_not_supported")
        .with_span_label(&name_span, "invalid type")
        .with_help(
            "Use a type parameter instead, e.g. `interface Comparable<T> { fn compare(self, other: T) -> int }`",
        )
}

pub fn type_not_found(type_name: &str, annotation_span: Span) -> LisetteDiagnostic {
    let simple_name = type_name.rsplit('.').next().unwrap_or(type_name);
    let name_span = Span::new(
        annotation_span.file_id,
        annotation_span.byte_offset,
        simple_name.len() as u32,
    );

    let looks_like_type_param = simple_name.len() == 1
        && simple_name.chars().next().is_some_and(|c| c.is_uppercase())
        || ["Key", "Value", "Item", "Error", "Elem", "In", "Out"].contains(&simple_name);

    if looks_like_type_param {
        return LisetteDiagnostic::error("Undeclared type parameter")
            .with_resolve_code("type_not_found")
            .with_span_label(&name_span, "undeclared")
            .with_help(format!(
                "Declare the type parameter, e.g. `impl<{t}>` or `fn foo<{t}>`",
                t = simple_name
            ));
    }

    LisetteDiagnostic::error("Type not found")
        .with_resolve_code("type_not_found")
        .with_span_label(&name_span, "type not found in scope")
        .with_help("Define or import this type")
}

pub fn undeclared_impl_type_param(
    type_name: &str,
    annotation_span: Span,
    receiver_name: &str,
) -> LisetteDiagnostic {
    let name_span = Span::new(
        annotation_span.file_id,
        annotation_span.byte_offset,
        type_name.len() as u32,
    );

    LisetteDiagnostic::error("Undeclared type parameter")
        .with_resolve_code("type_not_found")
        .with_span_label(&name_span, "undeclared")
        .with_help(format!(
            "Declare the type parameter: `impl<{t}> {r}<{t}>`",
            t = type_name,
            r = receiver_name
        ))
}

pub fn type_param_with_args(type_arg_count: usize, span: Span) -> LisetteDiagnostic {
    let noun = if type_arg_count == 1 {
        "type argument"
    } else {
        "type arguments"
    };

    LisetteDiagnostic::error("Invalid type argument")
        .with_infer_code("type_param_with_args")
        .with_span_label(&span, "type is not parameterized")
        .with_help(format!("Remove {}", noun))
}

pub fn type_args_on_non_generic(type_arg_count: usize, span: Span) -> LisetteDiagnostic {
    let noun = if type_arg_count == 1 {
        "type argument"
    } else {
        "type arguments"
    };

    LisetteDiagnostic::error("Unexpected type arguments")
        .with_infer_code("type_arg_on_non_generic")
        .with_span_label(&span, "accepts no type arguments")
        .with_help(format!("Remove the {} from this call", noun))
}

pub fn circular_type_alias(type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Circular type alias")
        .with_resolve_code("circular_type_alias")
        .with_span_label(&span, format!("`{}` references itself", type_name))
        .with_help("Type aliases cannot be recursive")
}

pub fn const_disallows_composite(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Composite value in `const`")
        .with_infer_code("const_disallows_composite")
        .with_span_label(&span, "not allowed")
        .with_help("`const` only accepts primitive values: `bool`, `int`, `float`, and `string`")
}

pub fn const_cycle(cycle: &[String], span: Span) -> LisetteDiagnostic {
    let mut diagnostic = LisetteDiagnostic::error("`const` init cycle")
        .with_infer_code("const_cycle")
        .with_help(
            "`const` initializers cannot refer to themselves, either directly or transitively",
        );
    diagnostic = if cycle.len() == 1 {
        diagnostic.with_span_label(&span, "self-reference")
    } else {
        let chain = cycle
            .iter()
            .map(|name| format!("`{}`", name))
            .collect::<Vec<_>>()
            .join(" → ");
        diagnostic.with_span_label(&span, format!("cycle: {} → `{}`", chain, cycle[0]))
    };
    diagnostic
}

pub fn name_not_found(
    variable_name: &str,
    span: Span,
    available_names: &[String],
) -> LisetteDiagnostic {
    if matches!(variable_name, "nil" | "null" | "Nil" | "undefined") {
        return LisetteDiagnostic::error(format!("`{}` is not supported", variable_name))
            .with_resolve_code("nil_not_supported")
            .with_span_label(&span, "does not exist")
            .with_help("Absence is encoded with `Option<T>` in Lisette. Use `None` to represent absent values.");
    }

    if let Some(hint) = go_builtin_hint(variable_name) {
        return LisetteDiagnostic::error("Name not found")
            .with_resolve_code("name_not_found")
            .with_span_label(&span, "name not found in scope")
            .with_help(hint);
    }

    let mut diagnostic = LisetteDiagnostic::error("Name not found")
        .with_resolve_code("name_not_found")
        .with_span_label(&span, "name not found in scope");

    let suggestion = available_names
        .iter()
        .filter_map(|c| {
            let d = levenshtein_distance(variable_name, c);
            (d <= 2).then_some((c, d))
        })
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c.clone());

    if let Some(suggestion) = suggestion {
        diagnostic = diagnostic.with_help(format!("Did you mean `{}`?", suggestion));
    } else {
        diagnostic = diagnostic.with_help(format!("Define or import `{}`.", variable_name));
    }

    diagnostic
}

pub fn self_in_static_method(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `self`")
        .with_resolve_code("self_in_static_method")
        .with_span_label(&span, "`self` is not available here")
        .with_help("Add a `self` parameter to the method if you need an instance method")
}

pub fn static_method_called_on_instance(
    method_name: &str,
    type_name: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Static method called on instance")
        .with_infer_code("static_method_on_instance")
        .with_span_label(&span, format!("`{}` is a static method", method_name))
        .with_help(format!(
            "Call it as `{}.{}(...)` on the type, not on an instance",
            type_name, method_name
        ))
}

pub fn function_or_value_not_found_in_module(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Name not found")
        .with_resolve_code("not_found_in_module")
        .with_span_label(&span, format!("`{}` not found in module", name))
        .with_help("Ensure the name is exported and spelled correctly")
}

pub fn receiver_type_mismatch(
    impl_type: &str,
    receiver_type: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("receiver_type_mismatch")
        .with_span_label(
            &span,
            format!(
                "expected `{}` or `Ref<{}>`, found `{}`",
                impl_type, impl_type, receiver_type
            ),
        )
        .with_help(format!(
            "Change the receiver type to `{}` or `Ref<{}>`",
            impl_type, impl_type
        ))
}

pub fn receiver_must_be_named_self(actual_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid receiver name")
        .with_infer_code("receiver_not_self")
        .with_span_label(&span, "expected `self`")
        .with_help(format!(
            "Rename `{}` to `self`. In an instance method definition, Lisette expects the first parameter to be named `self`",
            actual_name
        ))
}

pub fn stringer_signature_mismatch(method_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reserved method signature")
        .with_infer_code("stringer_signature_mismatch")
        .with_span_label(
            &span,
            format!("`{}` must have signature `(self) -> string`", method_name),
        )
        .with_help(format!(
            "`{}` is reserved for the Go `fmt.Stringer` (or `fmt.GoStringer`) interface and is auto-emitted by Lisette. Either change the signature to `(self) -> string`, or rename the method",
            method_name
        ))
}

pub fn disallowed_mutation(
    variable_name: &str,
    span: Span,
    self_type_name: Option<&str>,
    is_match_arm_binding: bool,
    is_const_binding: bool,
) -> LisetteDiagnostic {
    if variable_name == "self" {
        if let Some(type_name) = self_type_name {
            LisetteDiagnostic::error("Immutable receiver")
                .with_infer_code("value_receiver_immutable")
                .with_span_label(&span, "receiver is immutable")
                .with_help(format!(
                    "Use `self: Ref<{type_name}>` to make the receiver mutable"
                ))
        } else {
            LisetteDiagnostic::error("Immutable receiver")
                .with_infer_code("value_receiver_immutable")
                .with_span_label(&span, "receiver is immutable")
                .with_help("Use `self: Ref<Self>` to make the receiver mutable")
        }
    } else if is_const_binding {
        LisetteDiagnostic::error("Cannot mutate `const`")
            .with_infer_code("immutable")
            .with_span_label(&span, "cannot mutate a `const`")
            .with_help(format!(
                "`const` bindings are immutable. Rebind with `let mut {variable_name} = {variable_name}` to mutate a local copy"
            ))
    } else if is_match_arm_binding {
        LisetteDiagnostic::error("Immutable variable")
            .with_infer_code("immutable")
            .with_span_label(&span, "cannot mutate an immutable variable")
            .with_help(format!(
                "Pattern bindings are immutable; rebind with `let mut {variable_name} = {variable_name}` to mutate"
            ))
    } else {
        LisetteDiagnostic::error("Immutable variable")
            .with_infer_code("immutable")
            .with_span_label(&span, "cannot mutate an immutable variable")
            .with_help(format!(
                "Declare using `let mut {variable_name}` to make the variable mutable"
            ))
    }
}

pub fn self_reference_in_assignment(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot reassign variable while taking its reference")
        .with_infer_code("self_reference_in_assignment")
        .with_span_label(&span, "disallowed")
        .with_help("Separate the reassignment from reference taking, or use a different variable")
}

pub fn uppercase_binding(span: Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid binding name")
        .with_infer_code("uppercase_binding")
        .with_span_label(&span, "binding names must start with a lowercase letter")
        .with_help(format!("Use a lowercase name instead of `{}`", name))
}

pub fn enum_variant_constructor_not_found(
    span: Span,
    enum_info: Option<(&str, &[String])>,
    variant_name: &str,
) -> LisetteDiagnostic {
    let help = if let Some((enum_name, variants)) = enum_info {
        if variants.iter().any(|v| v == variant_name) {
            format!("Use `{}.{}` to match this variant", enum_name, variant_name)
        } else if let Some(closest) = variants
            .iter()
            .filter_map(|v| {
                let d = levenshtein_distance(variant_name, v);
                (d <= 2).then_some((v, d))
            })
            .min_by_key(|(_, d)| *d)
            .map(|(v, _)| v)
        {
            format!("Did you mean `{}.{}`?", enum_name, closest)
        } else {
            let variants_fmt = format_list(variants, |v| format!("`{}.{}`", enum_name, v));
            format!(
                "Available variants for `{}` are {}",
                enum_name, variants_fmt
            )
        }
    } else {
        "Check that the variant is defined in the enum and spelled correctly".to_string()
    };

    LisetteDiagnostic::error("Variant not found")
        .with_resolve_code("variant_not_found")
        .with_span_label(&span, "not found")
        .with_help(help)
}

pub fn value_enum_in_source_file(enum_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid value enum")
        .with_infer_code("value_enum_outside_typedef")
        .with_span_label(&span, "not allowed in .lis files")
        .with_help(format!(
            "Use a regular enum instead: `enum {} {{ A, B, C }}`. Value enums exist only to represent Go's enums in typedefs.",
            enum_name
        ))
}

pub fn arity_mismatch(
    expected: &[Type],
    actual: &[Type],
    generic_params: &[String],
    is_constructor: bool,
    span: Span,
) -> LisetteDiagnostic {
    let expected_str = if !generic_params.is_empty() {
        generic_params.join(", ")
    } else {
        expected
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let actual_str = actual
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let expected_count = expected.len();
    let actual_count = actual.len();
    let expected_word = if expected_count == 1 {
        "argument"
    } else {
        "arguments"
    };

    LisetteDiagnostic::error("Wrong argument count")
        .with_infer_code("arg_count_mismatch")
        .with_span_label(
            &span,
            format!("expected `({})`, found `({})`", expected_str, actual_str),
        )
        .with_help(format!(
            "This {} expects {} {} but received {} arguments",
            if is_constructor {
                "constructor"
            } else {
                "function"
            },
            expected_count,
            expected_word,
            actual_count
        ))
}

pub fn generics_arity_mismatch(
    expected_generic_params: &[String],
    actual_type_args: &[Annotation],
    actual_types: &[Type],
    span: Span,
) -> LisetteDiagnostic {
    let expected: Vec<Type> = expected_generic_params
        .iter()
        .map(|param| Type::Parameter(param.as_str().into()))
        .collect();

    let expected_str = expected
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let actual_str = actual_types
        .iter()
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let expected_count = expected.len();
    let actual_count = actual_types.len();
    let expected_word = if expected_count == 1 {
        "type parameter"
    } else {
        "type parameters"
    };

    let generics_span =
        if let (Some(first), Some(last)) = (actual_type_args.first(), actual_type_args.last()) {
            let first_span = first.get_span();
            let last_span = last.get_span();
            Span::new(
                first_span.file_id,
                first_span.byte_offset.saturating_sub(1),
                (last_span.byte_offset + last_span.byte_length + 1)
                    .saturating_sub(first_span.byte_offset.saturating_sub(1)),
            )
        } else {
            span
        };

    LisetteDiagnostic::error("Wrong type argument count")
        .with_infer_code("type_arg_count_mismatch")
        .with_span_label(
            &generics_span,
            format!("expected `<{}>`, found `<{}>`", expected_str, actual_str),
        )
        .with_help(format!(
            "This type expects {} {} but received {} type parameters",
            expected_count, expected_word, actual_count
        ))
}

pub fn tuple_arity_mismatch(
    pattern_arity: usize,
    expected_arity: usize,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Tuple arity mismatch")
        .with_infer_code("tuple_element_count_mismatch")
        .with_span_label(
            &span,
            format!(
                "expected {} elements, found {} elements",
                expected_arity, pattern_arity
            ),
        )
        .with_help("Adjust the pattern to match the number of elements in the tuple.")
}

pub fn struct_not_found(identifier: &str, span: Span) -> LisetteDiagnostic {
    let simple_name = identifier.rsplit('.').next().unwrap_or(identifier);
    let name_span = Span::new(span.file_id, span.byte_offset, simple_name.len() as u32);

    LisetteDiagnostic::error("Struct not found")
        .with_resolve_code("struct_not_found")
        .with_span_label(&name_span, "struct not found in scope")
        .with_help("Define or import this struct")
}

pub fn struct_missing_fields(
    struct_name: &str,
    missing: &[String],
    span: Span,
) -> LisetteDiagnostic {
    let fields_list = missing.join(", ");

    let simple_name = struct_name.rsplit('.').next().unwrap_or(struct_name);
    let name_span = Span::new(span.file_id, span.byte_offset, simple_name.len() as u32);

    LisetteDiagnostic::error(format!("Struct `{}` is missing fields", simple_name))
        .with_infer_code("missing_struct_fields")
        .with_span_label(&name_span, format!("missing fields: {}", fields_list))
        .with_help("Initialize all fields in this struct literal")
}

pub fn pattern_missing_fields(missing: &[String], span: Span) -> LisetteDiagnostic {
    let (noun, fields_fmt) = if missing.len() == 1 {
        ("field", format!("`{}`", missing[0]))
    } else {
        let formatted: Vec<String> = missing.iter().map(|f| format!("`{}`", f)).collect();
        ("fields", formatted.join(", "))
    };

    let pronoun = if missing.len() == 1 { "it" } else { "them" };

    LisetteDiagnostic::error("Missing pattern fields")
        .with_infer_code("pattern_missing_fields")
        .with_span_label(&span, format!("missing {}", fields_fmt))
        .with_help(format!(
            "Include the missing {}, or use `..` to ignore {}",
            noun, pronoun
        ))
}

pub fn private_field_access(field_name: &str, struct_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Private field")
        .with_resolve_code("private_field_spread")
        .with_span_label(&span, "private")
        .with_help(format!(
            "Cannot access private field `{}` of struct `{}`. Mark the field as `pub`.",
            field_name, struct_name
        ))
}

pub fn private_method_access(method_name: &str, type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Private method")
        .with_span_label(&span, "private")
        .with_help(format!(
            "Cannot access private method `{}` of type `{}`. Mark the method as `pub`.",
            method_name, type_name
        ))
}

pub fn private_field_in_spread(
    field_name: &str,
    struct_name: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Private field")
        .with_resolve_code("private_field_spread")
        .with_span_label(&span, "private")
        .with_help(format!(
            "Cannot spread `{}` because field `{}` is private. Mark the field as `pub`.",
            struct_name, field_name
        ))
}

pub fn private_field_in_zero_fill(
    field_name: &str,
    struct_name: &str,
    owning_module: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Private field")
        .with_resolve_code("private_field_zero_fill")
        .with_span_label(&span, "private")
        .with_help(format!(
            "`{}` of `{}` cannot be zero-filled because `{}` is private to module `{}`. \
             Provide an explicit value, or have `{}` expose `{}` as `pub` or offer a \
             constructor.",
            field_name, struct_name, field_name, owning_module, owning_module, field_name
        ))
}

pub fn field_no_zero(
    struct_name: &str,
    field_name: &str,
    field_ty: &Type,
    chain: &[&str],
    private: Option<(&str, &str, &str)>,
    span: Span,
) -> LisetteDiagnostic {
    let main = match private {
        Some((priv_struct, priv_field, priv_module)) => format!(
            "`{}` of `{}` cannot be zero-filled because `{}.{}` is private to module `{}`. \
             Provide an explicit value for `{}`, or have `{}` expose `{}` as `pub`.",
            field_name,
            struct_name,
            priv_struct,
            priv_field,
            priv_module,
            field_name,
            priv_module,
            priv_field
        ),
        None if chain.is_empty() => format!(
            "Field `{}` of type `{}` has no zero value. Provide an explicit value, \
             or wrap the field type in `Option<T>`.",
            field_name, field_ty
        ),
        None => format!(
            "Field `{}.{}` of type `{}` has no zero value. Provide an explicit value for \
             `{}`, or wrap the field type in `Option<T>`.",
            field_name,
            chain.join("."),
            field_ty,
            field_name
        ),
    };
    LisetteDiagnostic::error("Field has no zero value")
        .with_infer_code("field_no_zero")
        .with_span_label(&span, "no zero available")
        .with_help(main)
}

pub fn member_not_found(
    ty: &Type,
    field: &str,
    span: Span,
    available_fields: Option<&[String]>,
) -> LisetteDiagnostic {
    let mut diagnostic = LisetteDiagnostic::error("Member not found")
        .with_infer_code("member_not_found")
        .with_span_label(&span, format!("no member `{}` on type `{}`", field, ty));

    let suggestion = available_fields.and_then(|fields| find_similar_name(field, fields));

    if let Some(suggestion) = suggestion {
        diagnostic = diagnostic.with_help(format!("Did you mean `{}`?", suggestion));
    } else {
        diagnostic = diagnostic.with_help("Ensure the field or method is defined on this type");
    }

    diagnostic
}

pub fn not_numeric(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(&span, format!("expected `int` or `float`, found `{}`", ty))
        .with_help("The negation operator `-` can only be used with `int` or `float`")
}

pub fn not_numeric_for_binary(
    operator: &BinaryOperator,
    ty: &Type,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(&span, format!("expected `int` or `float`, found `{}`", ty))
        .with_help(format!(
            "The `{}` operator can only be used with `int` or `float`",
            operator
        ))
}

pub fn binary_operator_type_mismatch(
    operator: &BinaryOperator,
    left_ty: &Type,
    right_ty: &Type,
    span: Span,
) -> LisetteDiagnostic {
    let label_msg = format!(
        "cannot {} `{}` and `{}`",
        operator_verb(operator),
        left_ty,
        right_ty
    );

    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(&span, label_msg)
        .with_help(format!(
            "The `{}` operator {}",
            operator,
            operator_help(operator)
        ))
}

pub fn not_orderable(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(&span, format!("expected comparable, found `{}`", ty))
        .with_help("Use comparison operators only with numeric, string, or boolean types")
}

pub fn not_comparable(ty: &Type, reason: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(&span, format!("`{}` cannot be compared with `==`", ty))
        .with_help(format!(
            "The `==` and `!=` operators cannot be used on {} because they are not comparable in Go",
            reason
        ))
}

pub fn not_orderable_bound(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Bound not satisfied")
        .with_infer_code("not_orderable_bound")
        .with_span_label(&span, "does not satisfy `cmp.Ordered`")
        .with_help(
            "The type parameter must be `cmp.Ordered` but the argument is not orderable. \
             Relax the bound or pass an argument that satisfies it",
        )
}

pub fn not_comparable_bound(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Bound not satisfied")
        .with_infer_code("not_comparable_bound")
        .with_span_label(&span, "does not satisfy `Comparable`")
        .with_help(
            "The parameter must be `Comparable` but the argument is not comparable. \
             Relax the bound or pass an argument that satisfies it",
        )
}

pub fn bound_only_in_value_position(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("`{}` is a bound, not a value type", name))
        .with_infer_code("bound_only_in_value_position")
        .with_span_label(&span, "not allowed here")
        .with_help(format!(
            "Use `{}` only as a bound to constrain a generic parameter, e.g. `fn f<T: {}>(x: T)`",
            name, name
        ))
}

pub fn missing_bound_on_param(
    param_name: &str,
    required_bound: &str,
    span: Span,
) -> LisetteDiagnostic {
    let short = required_bound.rsplit('.').next().unwrap_or(required_bound);
    LisetteDiagnostic::error("Missing bound on type parameter")
        .with_infer_code("missing_bound_on_param")
        .with_span_label(&span, format!("does not satisfy `{}`", short))
        .with_help(format!(
            "The parameter must be `{}` but the argument is unbounded. \
             Add this bound to the enclosing function: `<{}: {}>`",
            required_bound, param_name, required_bound
        ))
}

pub fn division_by_zero(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Division by zero")
        .with_infer_code("division_by_zero")
        .with_span_label(&span, "cannot divide by zero")
        .with_help("This operation will panic at runtime")
}

pub fn incompatible_named_numeric_types(underlying_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("incompatible_named_numeric_types")
        .with_span_label(&span, "cannot compute")
        .with_help(format!(
            "Cast one to the other's type, or convert both to `{}`",
            underlying_ty
        ))
}

pub fn invalid_division_order(
    operator: &BinaryOperator,
    left_ty: &Type,
    right_ty: &Type,
    span: Span,
) -> LisetteDiagnostic {
    let (op_symbol, help_msg) = match operator {
        BinaryOperator::Division => (
            "/",
            format!(
                "To divide by `{}`, the dividend (left operand) must also be `{}`",
                right_ty, right_ty
            ),
        ),
        BinaryOperator::Remainder => (
            "%",
            format!(
                "To take the remainder by `{}`, the dividend (left operand) must also be `{}`",
                right_ty, right_ty
            ),
        ),
        _ => unreachable!(),
    };

    LisetteDiagnostic::error("Invalid operation")
        .with_infer_code("invalid_division_order")
        .with_span_label(
            &span,
            format!("cannot compute `{}` {} `{}`", left_ty, op_symbol, right_ty),
        )
        .with_help(help_msg)
}

pub fn branch_type_mismatch(
    consequence_ty: &Type,
    consequence_span: Span,
    alternative_ty: &Type,
    alternative_span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(
            &consequence_span,
            format!("this branch returns `{}`", consequence_ty),
        )
        .with_span_label(
            &alternative_span,
            format!("this branch returns `{}`", alternative_ty),
        )
        .with_help("All branches must return the same type")
}

pub fn missing_else_branch(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing `else` branch")
        .with_infer_code("missing_else_branch")
        .with_span_label(&span, "`else` branch required")
        .with_help("Add an `else` branch")
}

pub fn let_else_must_diverge(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `else` block")
        .with_infer_code("let_else_must_diverge")
        .with_span_primary_label(&span, "this branch does not diverge")
        .with_help("Add `return`, `break`, `continue`, or a diverging call in the `else` block")
}

pub fn return_outside_function(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`return` outside function")
        .with_infer_code("return_outside_function")
        .with_span_label(&span, "`return` outside function")
        .with_help("Use `return` only inside a function body")
}

pub fn disallowed_mut_use(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `mut`")
        .with_infer_code("mut_not_allowed")
        .with_span_label(&span, "not allowed here")
        .with_help("`mut` is not allowed with destructuring patterns")
}

pub fn cannot_match_on_functions(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid pattern")
        .with_infer_code("invalid_pattern")
        .with_span_label(&span, "cannot pattern match on functions")
        .with_help("Functions cannot be compared for equality")
}

pub fn cannot_match_on_unknown(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot match on Unknown")
        .with_infer_code("cannot_match_on_unknown")
        .with_span_label(&span, "is type `Unknown`")
        .with_help("Use `assert_type` to narrow this value into a concrete type before matching. Example: `let value = assert_type<MyType>(x)?`")
}

pub fn cannot_match_on_unconstrained_type(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Uninferred type")
        .with_infer_code("cannot_match_on_unconstrained_type")
        .with_span_label(&span, "type cannot be inferred at this point")
        .with_help("Add a type annotation on the value before matching on it")
}

pub fn duplicate_binding_in_pattern(
    name: &str,
    first_span: Span,
    second_span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate binding")
        .with_infer_code("duplicate_binding_in_pattern")
        .with_span_label(&first_span, format!("first use of `{}`", name))
        .with_span_label(&second_span, "used again")
        .with_help("Remove the duplicate binding")
}

pub fn literal_pattern_in_binding(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Pattern might not match")
        .with_infer_code("literal_in_binding")
        .with_span_label(&span, "value might not equal this literal")
        .with_help("Use `match` or `if` to compare values")
}

pub fn as_binding_in_irrefutable_context(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `as` binding")
        .with_infer_code("as_binding_in_irrefutable_context")
        .with_span_label(&span, "`as` is disallowed here")
        .with_help("Use `as` only in `match`, `if let`, and `while let`")
}

pub fn select_some_as_binding_not_supported(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot alias `Some(...)` in select")
        .with_infer_code("select_some_as_not_supported")
        .with_span_label(&span, "`as` cannot be placed around `Some(...)`")
        .with_help(
            "Place `as` inside `Some(...)` to bind the received value: `Some(value as alias)`",
        )
}

pub fn redundant_as_identifier(inner: &str, alias: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Redundant `as` binding")
        .with_infer_code("redundant_as_binding")
        .with_span_label(&span, format!("`{}` already binds this value", inner))
        .with_help(format!(
            "Use `{}` directly, or rename `{}` to `{}`",
            alias, inner, alias
        ))
}

pub fn redundant_as_wildcard(alias: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Redundant `as` binding")
        .with_infer_code("redundant_as_binding")
        .with_span_label(&span, "`_` binds nothing")
        .with_help(format!("Replace `_ as {}` with just `{}`", alias, alias))
}

pub fn redundant_as_literal(literal: &str, alias: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Redundant `as` binding")
        .with_infer_code("redundant_as_binding")
        .with_span_label(&span, format!("`{}` is always `{}`", alias, literal))
        .with_help(format!(
            "Replace `{} as {}` with just `{}`",
            literal, alias, literal
        ))
}

pub fn or_pattern_in_irrefutable_context(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid or-pattern")
        .with_infer_code("or_pattern_in_irrefutable")
        .with_span_label(&span, "or-patterns are not allowed here")
        .with_help("Use a `match` expression instead.")
        .with_note("Or-patterns can only be used in `match`, `if let`, and `while let`.")
}

pub fn or_pattern_binding_mismatch(
    span: Span,
    missing_in_later: &[&str],
    missing_in_first: &[&str],
) -> LisetteDiagnostic {
    let missing = if !missing_in_later.is_empty() {
        missing_in_later.join(", ")
    } else {
        missing_in_first.join(", ")
    };

    LisetteDiagnostic::error("Invalid or-pattern")
        .with_infer_code("or_pattern_binding_mismatch")
        .with_span_label(&span, "only bound here")
        .with_help(format!(
            "Variable {} is not bound in all alternatives. Use a wildcard `_` instead of a binding, or ensure all alternatives bind the same variable",
            missing
        ))
}

pub fn or_pattern_type_mismatch(span: Span, first_ty: &str, alt_ty: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid or-pattern")
        .with_infer_code("or_pattern_type_mismatch")
        .with_span_label(
            &span,
            format!("expected `{}`, found `{}`", first_ty, alt_ty),
        )
        .with_help(
            "Use a wildcard `_` instead of a binding, or use separate match arms for each variant",
        )
}

pub fn unknown_iterable_type(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Uninferrable type")
        .with_infer_code("type_not_inferred")
        .with_span_label(&span, "cannot be inferred")
        .with_help("Add a type annotation to the iterable expression")
}

pub fn not_iterable(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Not iterable")
        .with_infer_code("not_iterable")
        .with_span_label(&span, format!("`{}` is not iterable", ty))
        .with_help("Use `Slice`, `Map`, `Range`, or `string`")
}

pub fn tuple_literal_required_in_loop(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid loop pattern")
        .with_infer_code("invalid_pattern")
        .with_span_label(&span, "tuple literal required here")
        .with_help("Use `(key, value)` destructuring pattern for map or enumerated iteration")
}

pub fn propagate_on_partial(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use `?` on `Partial`")
        .with_infer_code("propagate_on_partial")
        .with_span_label(&span, "`Partial` requires explicit `match`")
        .with_help(
            "The `?` operator is incompatible with `Partial` because it has \
             three variants. Use `match` to handle `Ok`, `Err`, and `Both` \
             explicitly.",
        )
}

pub fn try_requires_result_or_option(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("try_requires_result_or_option")
        .with_span_label(&span, "expects `Result` or `Option`")
        .with_help("Use the `?` operator only on `Result` or `Option`")
}

pub fn try_outside_function(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`?` outside function")
        .with_infer_code("try_outside_function")
        .with_span_label(&span, "`?` outside function")
        .with_help("Use `?` only inside a function that returns `Result` or `Option`")
}

pub fn try_return_type_mismatch(expected: &str, actual_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("try_return_type_mismatch")
        .with_span_label(
            &span,
            format!(
                "expects `{}`, but function returns `{}`",
                expected, actual_ty
            ),
        )
        .with_help(format!(
            "Change the function return type to `{}` or remove the `?` operator",
            expected
        ))
}

pub fn try_block_empty(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Empty `try` block")
        .with_infer_code("try_block_empty")
        .with_span_label(&span, "empty")
        .with_help("Ensure the `try` block contains at least one expression")
}

pub fn try_block_no_question_mark(try_keyword_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Useless `try` block")
        .with_infer_code("try_block_no_question_mark")
        .with_span_label(&try_keyword_span, "no `?` operator found")
        .with_help("A `try` block must contain at least one `?` for propagation")
}

pub fn mixed_carriers_in_try_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Mixed `try` block")
        .with_infer_code("try_block_mixed_carriers")
        .with_span_label(&span, "mixing `Option` and `Result`")
        .with_help(
            "A `try` block must use either all `Option` operations or all `Result` operations",
        )
}

pub fn break_outside_loop(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`break` outside loop")
        .with_infer_code("break_outside_loop")
        .with_span_label(&span, "not inside a loop")
        .with_help("`break` can only be used inside `loop`, `for`, or `while`")
}

pub fn continue_outside_loop(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`continue` outside loop")
        .with_infer_code("continue_outside_loop")
        .with_span_label(&span, "not inside a loop")
        .with_help("`continue` can only be used inside `loop`, `for`, or `while`")
}

pub fn nested_function(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Nested function declaration")
        .with_infer_code("nested_function")
        .with_span_label(&span, "functions can only be declared at top level")
        .with_help("Use a lambda instead: `|x| x + 1` or `|x| { ... }`")
}

pub fn return_in_try_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`return` in `try` block")
        .with_infer_code("try_block_return")
        .with_span_label(&span, "not inside a function")
        .with_help(
            "Use `return` inside a function, or use `Err(...)? ` to exit the `try` block early",
        )
}

pub fn break_in_try_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`break` in `try` block")
        .with_infer_code("try_block_break")
        .with_span_label(&span, "not inside a loop")
        .with_help("Use `break` inside a loop, or use `Err(...)? ` to exit the `try` block early")
}

pub fn continue_in_try_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`continue` in `try` block")
        .with_infer_code("try_block_continue")
        .with_span_label(&span, "not inside a loop")
        .with_help(
            "Use `continue` inside a loop, or use `Err(...)? ` to exit the `try` block early",
        )
}

pub fn recover_block_empty(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Empty `recover` block")
        .with_infer_code("recover_block_empty")
        .with_span_label(&span, "empty")
        .with_help("Ensure the `recover` block contains at least one expression that may panic")
}

pub fn recover_cannot_use_question_mark(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`?` in `recover` block")
        .with_infer_code("recover_cannot_use_question_mark")
        .with_span_label(&span, "cannot propagate to `recover` block")
        .with_help(
            "Use a `try` block inside the `recover` block, or handle the `Result` explicitly",
        )
}

pub fn return_in_recover_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`return` in `recover` block")
        .with_infer_code("recover_block_return")
        .with_span_label(&span, "not allowed inside `recover` block")
        .with_help("Remove the `return`, or move it inside a nested function")
}

pub fn break_in_recover_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`break` in `recover` block")
        .with_infer_code("recover_block_break")
        .with_span_label(&span, "not allowed inside `recover` block")
        .with_help("Remove the `break`, or move it inside a loop within the `recover` block")
}

pub fn continue_in_recover_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`continue` in `recover` block")
        .with_infer_code("recover_block_continue")
        .with_span_label(&span, "not allowed inside `recover` block")
        .with_help("Remove the `continue`, or move it inside a loop within the `recover` block")
}

pub fn expected_channel_receive(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Expected channel receive")
        .with_infer_code("expected_channel_receive")
        .with_span_label(&span, format!("`{}` is not a channel receive", ty))
        .with_help("Use `ch.receive()` to receive from a channel in select")
}

pub fn empty_select(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Empty select")
        .with_infer_code("empty_select")
        .with_span_label(&span, "select has no arms")
        .with_help(
            "Add at least one channel operation arm, e.g. `select { ch.receive() => v { ... } }`",
        )
}

pub fn expected_channel_send(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Expected channel operation")
        .with_infer_code("expected_channel_send")
        .with_span_label(&span, "not a channel operation")
        .with_help("Use `ch.send(value)` or `ch.receive()` in select arms")
}

pub fn bare_identifier_in_select_receive(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select case")
        .with_infer_code("bare_identifier_in_select_receive")
        .with_span_label(span, "expected destructuring")
        .with_help("`ch.receive()` returns an `Option`, so use `let Some(v) = ch.receive()` to bind the value")
}

pub fn none_pattern_in_select_receive(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select case")
        .with_infer_code("none_pattern_in_select_receive")
        .with_span_label(span, "expected match")
        .with_help(
            "To detect channel close, use `match ch.receive() { Some(v) => ..., None => ... }`",
        )
}

pub fn select_match_missing_some_arm(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_missing_some_arm")
        .with_span_label(&span, "missing `Some` arm")
        .with_help("`None` only handles channel close. Add a `Some(v) => ...` arm to handle received values")
}

pub fn select_match_missing_none_arm(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_missing_none_arm")
        .with_span_label(&span, "missing `None` arm")
        .with_help("Matching on `ch.receive()` requires handling channel close. Add a `None => ...` arm to handle channel close, or simplify to `let Some(v) = ch.receive() => ...`")
}

pub fn select_match_duplicate_some_arm(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_duplicate_some_arm")
        .with_span_label(&span, "duplicate")
        .with_help(
            "Remove the duplicate `Some` arm. If you need to, use a `match` inside the arm body",
        )
}

pub fn select_match_duplicate_none_arm(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_duplicate_none_arm")
        .with_span_label(&span, "duplicate")
        .with_help(
            "Remove the duplicate `None` arm. If you need to, use a `match` inside the arm body",
        )
}

pub fn select_match_guard_not_allowed(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_guard_not_allowed")
        .with_span_label(&span, "not supported")
        .with_help("Match arms inside `select` do not support guards. Move the condition inside the arm body: `Some(v) => { if condition { ... } }`")
}

pub fn select_match_invalid_pattern(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select match")
        .with_infer_code("select_match_invalid_pattern")
        .with_span_label(&span, "unsupported pattern")
        .with_help("Select match arms support only `Some(...)` and `None` patterns")
}

pub fn select_receive_refutable_pattern(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Refutable pattern in select receive")
        .with_infer_code("select_receive_refutable_pattern")
        .with_span_label(&span, "may not match all received values")
        .with_help(
            "Select receive requires an irrefutable binding like `Some(v)` or `Some(_)`. \
             Use a regular `match` inside the arm body to filter values",
        )
}

pub fn multiple_select_receives(first_span: Span, second_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select")
        .with_infer_code("multiple_select_receives")
        .with_span_label(&first_span, "first receive arm")
        .with_span_label(&second_span, "second receive arm")
        .with_help("Multiple shorthand receive arms can lead to unexpected behavior when a channel closes. Use `match ch.receive() { Some(v) => ..., None => ... }` to handle closes explicitly")
}

pub fn duplicate_select_default(first_span: Span, second_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid select")
        .with_infer_code("duplicate_select_default")
        .with_span_label(&first_span, "first default arm")
        .with_span_label(&second_span, "duplicate default arm")
        .with_help(
            "A select block can have at most one default arm (`_ => ...`). Remove the duplicate.",
        )
}

pub fn non_exhaustive_select_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Non-exhaustive select expression")
        .with_infer_code("non_exhaustive_select_expression")
        .with_span_label(&span, "may not produce a value")
        .with_help("Add a default arm `_ => ...` to handle closed channels")
}

pub fn type_must_be_known(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Uninferrable type")
        .with_infer_code("type_not_inferred")
        .with_span_label(&span, "cannot be inferred")
        .with_help("Add a type annotation to help the compiler infer the type")
}

pub fn uninferred_binding(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Uninferrable type")
        .with_infer_code("type_not_inferred")
        .with_span_label(&span, "cannot be inferred")
        .with_help(format!(
            "Add a type annotation. For example: `let {}: Slice<int> = ...`",
            name
        ))
}

pub fn unconstrained_type_param(param_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unconstrained type parameter")
        .with_infer_code("unconstrained_type_param")
        .with_span_label(
            &span,
            format!(
                "`{}` is not constrained by parameters or return type",
                param_name
            ),
        )
        .with_help(format!(
            "Use `{}` in a parameter or return type, or provide an explicit type argument: `func<SomeType>(...)`",
            param_name
        ))
}

pub fn slice_index_type_mismatch(index_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("slice_index_type_mismatch")
        .with_span_label(&span, format!("expected `int`, found `{}`", index_ty))
        .with_help(
            "Use an integer to index into a `Slice`. For key-value lookup, use a `Map<K, V>`",
        )
}

pub fn only_slices_and_maps_indexable(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Not indexable")
        .with_infer_code("not_indexable")
        .with_span_label(&span, format!("expected `Slice` or `Map`, found `{}`", ty))
        .with_help("Only `Slice` and `Map` can be indexed into")
}

pub fn string_not_indexable(span: Span, receiver: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot index into `string`")
        .with_infer_code("string_not_indexable")
        .with_span_label(&span, "not indexable")
        .with_help(format!(
            "Use `{receiver}.rune_at(i)` to get a `rune`, or `{receiver}.byte_at(i)` to get a `byte`"
        ))
}

pub fn not_callable(
    ty: &Type,
    callee_name: Option<&str>,
    arg_name: Option<&str>,
    span: Span,
) -> LisetteDiagnostic {
    let type_name = ty.get_name();
    let is_type_call = matches!((callee_name, type_name), (Some(c), Some(t)) if c == t);
    let is_cast_target = ty.get_underlying().is_some()
        || type_name.is_some_and(|n| SimpleKind::from_name(n).is_some());

    let help = if is_type_call && is_cast_target {
        let subject = arg_name.unwrap_or("value");
        format!(
            "Use `{} as {}` to cast between types",
            subject,
            type_name.unwrap()
        )
    } else {
        "Only functions can be called with `()`".to_string()
    };

    LisetteDiagnostic::error("Not callable")
        .with_infer_code("not_callable")
        .with_span_label(&span, format!("expected function, found `{}`", ty))
        .with_help(help)
}

pub fn type_conversion_arity(
    type_name: &str,
    actual_count: usize,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Wrong argument count")
        .with_infer_code("type_conversion_arity")
        .with_span_label(
            &span,
            format!("expected 1 argument, found {}", actual_count),
        )
        .with_help(format!(
            "Type conversion `{}(value)` takes exactly one argument — the value to convert",
            type_name
        ))
}

#[derive(Debug, Clone)]
pub struct InterfaceViolation {
    pub interface_name: String,
    pub parent_of: Option<String>,
    pub missing: Vec<(String, Type)>,
    pub incompatible: Vec<(String, Type, Type)>,
}

pub fn interface_not_implemented(
    interface_name: &str,
    type_name: &str,
    violations: &[InterfaceViolation],
    span: Span,
) -> LisetteDiagnostic {
    let mut help_lines = Vec::new();

    let mut missing_sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut incompatible_sections: Vec<(String, Vec<String>)> = Vec::new();

    for violation in violations {
        let header = if let Some(ref parent) = violation.parent_of {
            format!(
                "From `{}` (required by `{}`)",
                violation.interface_name, parent
            )
        } else {
            format!("From `{}`", violation.interface_name)
        };

        if !violation.missing.is_empty() {
            let methods: Vec<String> = violation
                .missing
                .iter()
                .map(|(name, sig)| format!("  - {}: {}", name, sig))
                .collect();
            missing_sections.push((header.clone(), methods));
        }

        if !violation.incompatible.is_empty() {
            let methods: Vec<String> = violation
                .incompatible
                .iter()
                .map(|(name, expected, actual)| {
                    format!("  - {}: found `{}`, expected `{}`", name, actual, expected)
                })
                .collect();
            incompatible_sections.push((header, methods));
        }
    }

    if !missing_sections.is_empty() {
        help_lines.push("Missing methods:".to_string());
        for (header, methods) in &missing_sections {
            help_lines.push(format!("  {}", header));
            for method in methods {
                help_lines.push(format!("  {}", method));
            }
        }
    }

    if !incompatible_sections.is_empty() {
        help_lines.push("Incompatible methods:".to_string());
        for (header, methods) in &incompatible_sections {
            help_lines.push(format!("  {}", header));
            for method in methods {
                help_lines.push(format!("  {}", method));
            }
        }
    }

    LisetteDiagnostic::error("Interface not implemented")
        .with_infer_code("interface_not_implemented")
        .with_span_label(
            &span,
            format!("`{}` does not implement `{}`", type_name, interface_name),
        )
        .with_help(help_lines.join("\n"))
}

pub fn pointer_receiver_interface_mismatch(
    interface_name: &str,
    type_name: &str,
    methods: &[String],
    span: Span,
) -> LisetteDiagnostic {
    let methods_str = methods
        .iter()
        .map(|m| format!("`{}.{}`", type_name, m))
        .collect::<Vec<_>>()
        .join(", ");
    let takes = if methods.len() == 1 {
        format!("{} takes `self: Ref<{}>`", methods_str, type_name)
    } else {
        format!("{} take `self: Ref<{}>`", methods_str, type_name)
    };
    LisetteDiagnostic::error("Interface not implemented")
        .with_infer_code("interface_not_implemented")
        .with_span_label(
            &span,
            format!("`{}` does not implement `{}`", type_name, interface_name),
        )
        .with_help(format!("{}, so pass a `Ref<{}>`.", takes, type_name))
}

pub fn unknown_outside_typedef(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `Unknown` type")
        .with_infer_code("unknown_outside_typedef")
        .with_span_label(&span, "not allowed in `.lis` files")
        .with_help("Remove the `Unknown` annotation.")
        .with_note("`Unknown` can only be used in `.d.lis` files. This type exists only to represent Go's `any` in typedefs.")
}

pub fn opaque_type_outside_typedef(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Undefined type")
        .with_infer_code("undefined_type_outside_typedef")
        .with_span_label(&span, "needs a definition")
        .with_help("Use `type Point = ...` to define the type.")
        .with_note("Opaque declarations are only allowed in `.d.lis` files.")
}

pub fn bodyless_function_outside_typedef(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing function body")
        .with_infer_code("bodyless_function_outside_typedef")
        .with_span_label(&span, "needs a body")
        .with_help("Add a body: `fn greet() { ... }`.")
        .with_note("Bodyless declarations are only allowed in `.d.lis` files.")
}

pub fn valueless_const_outside_typedef(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing const value")
        .with_infer_code("valueless_const_outside_typedef")
        .with_span_label(&span, "needs a value")
        .with_help("Ensure the constant has a value: `const MAX_SIZE: int = 100`.")
        .with_note("Valueless const declarations are only allowed in `.d.lis` files.")
}

pub fn valueless_const_missing_annotation(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing const annotation")
        .with_infer_code("valueless_const_missing_annotation")
        .with_span_label(&span, "needs a type annotation")
        .with_help("Value-less const declarations require a type annotation: `const MAX_SIZE: int`")
}

pub fn variable_declaration_outside_typedef(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid variable declaration")
        .with_infer_code("variable_declaration_outside_typedef")
        .with_span_label(&span, "`var` is not allowed here")
        .with_help("Use `let` to declare a variable: `let x: int = 0`.")
        .with_note("`var` declarations are only allowed in `.d.lis` files.")
}

pub fn range_full_not_valid_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid expression")
        .with_infer_code("range_full_not_expression")
        .with_span_label(&span, "`..` can only be used in slice indexing")
        .with_help("Use `arr[..]` to get a full slice, or provide bounds like `0..10`")
}

pub fn range_not_iterable(range_type: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Not iterable")
        .with_infer_code("range_not_iterable")
        .with_span_label(&span, format!("`{}` has no start bound", range_type))
        .with_help("Use a range with a start bound, e.g. `0..10` instead of `..10`")
}

pub fn taking_value_of_ufcs_method(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid method value")
        .with_infer_code("taking_value_of_ufcs_method")
        .with_span_label(&span, "taking value not allowed")
        .with_help(
            "This method cannot be taken as a value. Call the method directly: `obj.method(...)`",
        )
}

pub fn duplicate_definition(kind: &str, name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!("Duplicate {}", kind))
        .with_infer_code("duplicate_definition")
        .with_span_label(&span, "already defined")
        .with_help(format!(
            "`{}` is already defined in this module. Rename or remove this definition.",
            name
        ))
}

pub fn duplicate_impl_item(item_name: &str, type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate name in impl")
        .with_infer_code("duplicate_impl_item")
        .with_span_label(&span, "method name already taken")
        .with_help(format!(
            "Method `{}` is already defined for type `{}`. Rename one of the methods.",
            item_name, type_name
        ))
}

pub fn duplicate_method_across_specialized_impls(
    method_name: &str,
    type_name: &str,
    generics: &[String],
    span: Span,
) -> LisetteDiagnostic {
    let params = generics.join(", ");
    LisetteDiagnostic::error("Duplicate method across specialized `impl` blocks")
        .with_infer_code("duplicate_method_across_specialized_impls")
        .with_span_label(&span, "already defined in another specialization")
        .with_help(format!(
            "Specialized `impl` blocks for `{type_name}` share a method namespace. \
             Use different method names, or move `{method_name}` to a generic `impl<{params}> {type_name}<{params}> {{}}` block."
        ))
}

pub fn method_shadows_field(type_name: &str, field_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Method shadows struct field")
        .with_infer_code("method_shadows_field")
        .with_span_label(&span, "same as field")
        .with_help(format!(
            "`{}` has a field `{}` and a method `{}`. Rename either the field or the method",
            type_name, field_name, field_name
        ))
}

pub fn non_int_range_not_iterable(element_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Not iterable")
        .with_infer_code("non_int_range_not_iterable")
        .with_span_label(
            &span,
            format!("cannot iterate over `Range<{}>`", element_ty),
        )
        .with_help("Range iteration requires integer bounds")
}

pub fn only_slices_indexable_by_range(ty: &Type, span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("range_index_not_slice")
        .with_span_label(
            span,
            format!("expected `Slice` or `string`, found `{}`", ty),
        )
        .with_help("Range indexing only works on `Slice` and `string`")
}

pub fn empty_body_return_mismatch(expected_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type mismatch")
        .with_infer_code("type_mismatch")
        .with_span_label(
            &span,
            format!("promises `{}`, but returns `()`", expected_ty),
        )
        .with_help("Return a value or change the return type annotation to `()`.")
        .with_note("An empty function body implicitly returns `()`.")
}

fn operator_verb(operator: &BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Addition => "add",
        BinaryOperator::Subtraction => "subtract",
        BinaryOperator::Multiplication => "multiply",
        BinaryOperator::Division => "divide",
        BinaryOperator::Remainder => "get remainder of",
        BinaryOperator::Equal | BinaryOperator::NotEqual => "compare",
        BinaryOperator::LessThan
        | BinaryOperator::LessThanOrEqual
        | BinaryOperator::GreaterThan
        | BinaryOperator::GreaterThanOrEqual => "compare",
        BinaryOperator::And | BinaryOperator::Or => "apply logical operator to",
        BinaryOperator::Pipeline => "pipe",
    }
}

fn operator_help(op: &BinaryOperator) -> &'static str {
    match op {
        BinaryOperator::Addition => "requires both operands to have the same type",
        BinaryOperator::Subtraction
        | BinaryOperator::Multiplication
        | BinaryOperator::Division
        | BinaryOperator::Remainder => "requires both operands to have the same numeric type",
        BinaryOperator::Equal | BinaryOperator::NotEqual => {
            "requires both operands to have the same type"
        }
        BinaryOperator::LessThan
        | BinaryOperator::LessThanOrEqual
        | BinaryOperator::GreaterThan
        | BinaryOperator::GreaterThanOrEqual => "requires both operands to have the same type",
        BinaryOperator::And | BinaryOperator::Or => "requires both operands to be bool",
        BinaryOperator::Pipeline => "should have been desugared",
    }
}

pub fn task_in_expression_position(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `task`")
        .with_infer_code("task_in_expression_position")
        .with_span_label(&span, "produces no value")
        .with_help("Move `task` to its own statement")
}

pub fn defer_in_expression_position(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `defer`")
        .with_infer_code("defer_in_expression_position")
        .with_span_label(&span, "produces no value")
        .with_help("Move `defer` to its own statement")
}

pub fn non_addressable_expression(expression_kind: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Non-addressable expression")
        .with_infer_code("non_addressable_expression")
        .with_span_label(&span, format!("cannot take address of {}", expression_kind))
        .with_help("Assign the value to a variable first, then take its address")
}

pub fn non_addressable_const(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot take address of `const`")
        .with_infer_code("non_addressable_const")
        .with_span_label(&span, "not addressable")
        .with_help(
            "`const` bindings are not addressable. Copy the value into a local `let` first if you need a reference",
        )
}

pub fn non_addressable_assignment(expression_kind: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot assign to non-addressable expression")
        .with_infer_code("non_addressable_assignment")
        .with_span_label(&span, format!("cannot assign to {}", expression_kind))
        .with_help("Assign the value to a variable first, then modify it")
}

pub fn newtype_field_assignment(type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot assign to newtype field")
        .with_infer_code("newtype_field_assignment")
        .with_span_label(&span, "newtype fields are read-only")
        .with_help(format!(
            "Reconstruct the newtype: `variable = {type_name}(new_value)`"
        ))
}

pub fn complex_select_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Complex expression in `select` arm")
        .with_infer_code("complex_select_expression")
        .with_span_label(&span, "expected simple expression")
        .with_help("Hoist to a `let` binding before the `select`")
}

pub fn ref_slice_append(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot call append/extend on `Ref<Slice>`")
        .with_infer_code("ref_slice_append")
        .with_span_label(&span, "dereference the ref first")
        .with_help("Use `r.*.append(x)` to deref, then append")
}

pub fn map_field_chain_assignment(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot assign to field of map entry")
        .with_infer_code("map_field_chain_assignment")
        .with_span_label(&span, "assignment not allowed here")
        .with_help(
            "Extract, modify, and reinsert: `let mut entry = m[key]; entry.field = value; m[key] = entry`",
        )
}

pub fn enum_field_type_conflict(
    loc_a: &str,
    type_a: &str,
    loc_b: &str,
    type_b: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Conflicting field types across enum variants")
        .with_infer_code("enum_field_type_conflict")
        .with_span_label(&span, "field type mismatch")
        .with_help(format!(
            "`{loc_a}` is `{type_a}` but `{loc_b}` is `{type_b}`. Rename one of the fields or align their types",
        ))
}

pub fn cannot_auto_address_receiver(
    receiver_kind: &str,
    method_name: &str,
    expected_ty: &Type,
    actual_ty: &Type,
    span: Span,
) -> LisetteDiagnostic {
    let readable_kind = match receiver_kind {
        "map index expression" => "map lookup",
        "function call" => "function result",
        "literal" => "literal",
        "binary expression" => "expression result",
        "conditional expression" => "conditional result",
        "match expression" => "match result",
        "block expression" => "block result",
        "lambda" => "lambda",
        "tuple" => "tuple",
        "range expression" => "range expression",
        _ => "expression",
    };

    LisetteDiagnostic::error("Expression not modifiable")
        .with_infer_code("cannot_auto_address_receiver")
        .with_span_label(&span, "modifies its receiver")
        .with_help(format!(
            "Assign the {} to a variable first, then call the method. The receiver of `{}` is `{}`, not `{}`",
            readable_kind, method_name, expected_ty, actual_ty
        ))
}

pub fn break_value_in_non_loop(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`break` with value in non-`loop` loop")
        .with_infer_code("break_value_in_non_loop")
        .with_span_label(&span, "`break` with value only allowed in `loop`")
        .with_help("`break` with a value is only meaningful in `loop` expressions, which can return the value. In `for` and `while` loops, use `break` without a value.")
}

pub fn defer_in_loop(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`defer` inside loop")
        .with_infer_code("defer_in_loop")
        .with_span_label(&span, "not allowed inside loop")
        .with_help("Wrap the loop body in a helper function, e.g. `fn process(file: File) { defer file.close(); ... }` and call it in the loop: `for f in files { process(f); }`")
}

pub fn propagate_in_condition(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`?` cannot be used inside a condition")
        .with_infer_code("propagate_in_condition")
        .with_span_label(&span, "`?` inside condition")
        .with_help("Bind the result first: `let val = expression?; if val { ... }`")
}

pub fn propagate_in_defer(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid `?` in `defer`")
        .with_infer_code("propagate_in_defer")
        .with_span_label(&span, "`?` not allowed here")
        .with_help("`defer` in combination with `?` is not allowed due to confusing semantics. Handle the error inside a `defer` block: `defer { if let Err(e) = file.close() { log(e); } }`")
}

pub fn return_in_defer_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`return` in `defer` block")
        .with_infer_code("return_in_defer_block")
        .with_span_label(&span, "not allowed inside `defer` block")
        .with_help("Remove the `return` as it only exits the `defer` block")
}

pub fn break_in_defer_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`break` in `defer` block")
        .with_infer_code("break_in_defer_block")
        .with_span_label(&span, "not allowed inside `defer` block")
        .with_help("Remove the `break`, or move it inside a loop within the `defer` block")
}

pub fn continue_in_defer_block(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`continue` in `defer` block")
        .with_infer_code("continue_in_defer_block")
        .with_span_label(&span, "not allowed inside `defer` block")
        .with_help("Remove the `continue`, or move it inside a loop within the `defer` block")
}

pub fn invalid_cast(source_ty: &Type, target_ty: &Type, span: Span) -> LisetteDiagnostic {
    let same_constructor_with_unresolved = source_ty
        .get_qualified_id()
        .zip(target_ty.get_qualified_id())
        .is_some_and(|(s, t)| s == t)
        && source_ty.has_unbound_variables();

    let help = if same_constructor_with_unresolved {
        format!(
            "Use a type annotation instead: `let x: {} = ...`",
            target_ty,
        )
    } else if source_ty.is_string() {
        "Strings cannot be cast to numbers and require explicit conversion. Use `strconv.Atoi()` to parse.".into()
    } else if source_ty.is_complex() || target_ty.is_complex() {
        "Complex numbers cannot be cast directly. Use `real(c)` or `imaginary(c)` to extract components.".into()
    } else {
        "Casts are supported between numeric types, strings and byte/rune slices, concrete types and interfaces, and values and their type aliases.".into()
    };

    LisetteDiagnostic::error("Invalid cast")
        .with_infer_code("invalid_cast")
        .with_span_label(
            &span,
            format!("cannot cast `{}` to `{}`", source_ty, target_ty),
        )
        .with_help(help)
}

pub fn chained_cast(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid cast")
        .with_infer_code("chained_cast")
        .with_span_label(&span, "chained cast not allowed")
        .with_help("Use an intermediate variable if you need to cast through multiple types")
}

pub fn redundant_cast(ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Redundant cast")
        .with_infer_code("redundant_cast")
        .with_span_label(&span, format!("casting `{}` to itself has no effect", ty))
        .with_help("Remove the unnecessary cast")
}

pub fn integer_literal_overflow(
    target_ty: &str,
    min: i128,
    max: i128,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Integer literal overflow")
        .with_infer_code("integer_literal_overflow")
        .with_span_label(&span, format!("overflows `{}`", target_ty))
        .with_help(format!(
            "`{}` must be in range `{}` to `{}`",
            target_ty, min, max
        ))
}

pub fn float_literal_overflow(target_ty: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Float literal overflow")
        .with_infer_code("float_literal_overflow")
        .with_span_label(&span, format!("value overflows `{}`", target_ty))
        .with_help(format!(
            "Use `float64` for larger values, or ensure the value fits in `{}`",
            target_ty
        ))
}

pub fn cannot_negate_unsigned(target_ty: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot negate unsigned type")
        .with_infer_code("cannot_negate_unsigned")
        .with_span_label(&span, format!("cannot negate `{}`", target_ty))
        .with_help("Unsigned types cannot represent negative values")
}

fn go_builtin_hint(name: &str) -> Option<&'static str> {
    match name {
        "len" => Some(
            "Lisette has no `len` builtin. Use the `.length()` method instead, e.g. `items.length()`.",
        ),
        "cap" => Some(
            "Lisette has no `cap` builtin. Use the `.capacity()` method instead, e.g. `items.capacity()`.",
        ),
        "make" => Some(
            "Lisette has no `make` builtin. Use constructor methods instead, e.g. `Channel.new<int>()`, `Slice.new<int>()`, `Map.new<K, V>()`.",
        ),
        "append" => Some(
            "Lisette has no `append` builtin. Use the `.append()` method instead, e.g. `items.append(1)`.",
        ),
        "close" => Some(
            "Lisette has no `close` builtin. Use the `.close()` method instead, e.g. `ch.close()`.",
        ),
        "copy" => Some(
            "Lisette has no `copy` builtin. Use the `.copy_from()` method instead, e.g. `dst.copy_from(src)`.",
        ),
        "delete" => Some(
            "Lisette has no `delete` builtin. Use the `.delete()` method instead, e.g. `map.delete(key)`.",
        ),
        "new" => Some(
            "Lisette has no `new` builtin. Use constructor methods instead, e.g. `MyStruct { field: value }` or `MyType.new()`.",
        ),
        "print" | "println" | "printf" => Some(
            "Lisette has no `print` builtin. Use `fmt.Println`, `fmt.Printf`, etc. after `import \"go:fmt\"`.",
        ),
        _ => None,
    }
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let b_len = b.len();

    if a.is_empty() {
        return b_len;
    }
    if b_len == 0 {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, a_char) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Uses Levenshtein distance (threshold <= 2) and prefix matching
/// to catch abbreviations like `len` → `length`.
pub fn find_similar_name(name: &str, candidates: &[String]) -> Option<String> {
    let best_distance = candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein_distance(name, c);
            (d <= 2).then_some((c, d))
        })
        .min_by_key(|(_, d)| *d);

    let by_prefix = if name.len() >= 2 {
        candidates
            .iter()
            .filter(|c| c.starts_with(name) || name.starts_with(c.as_str()))
            .min_by_key(|c| c.len().abs_diff(name.len()))
    } else {
        None
    };

    match (best_distance, by_prefix) {
        (Some((d, dist)), Some(p)) => {
            // Prefer Levenshtein only if it's a very close match (distance 1),
            // otherwise prefer prefix which better handles abbreviations
            if dist <= 1 {
                Some(d.clone())
            } else {
                Some(p.clone())
            }
        }
        (Some((d, _)), None) => Some(d.clone()),
        (None, Some(p)) => Some(p.clone()),
        (None, None) => None,
    }
}

pub fn cannot_infer_type_argument(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing type argument")
        .with_infer_code("missing_type_argument")
        .with_span_label(&span, "expected type argument")
        .with_help("Supply a type argument for the call, e.g. `Channel.new<int>()`")
}

fn format_list<T, F>(items: &[T], fmt: F) -> String
where
    F: Fn(&T) -> String,
{
    match items.len() {
        0 => String::new(),
        1 => fmt(&items[0]),
        2 => format!("{} and {}", fmt(&items[0]), fmt(&items[1])),
        _ => {
            let mut result = String::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    result.push_str(", ");
                }
                if i == items.len() - 1 {
                    result.push_str("and ");
                }
                result.push_str(&fmt(item));
            }
            result
        }
    }
}

pub fn recursive_generic_instantiation(type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Recursive generic instantiation")
        .with_infer_code("recursive_instantiation")
        .with_span_label(&span, format!("`{}` is nested within itself", type_name))
        .with_help(format!(
            "Go does not allow recursive type instantiation (e.g., `{0}<{0}<T>>`). \
             Use a wrapper type or a different design.",
            type_name
        ))
}

pub fn non_comparable_map_key(key_ty: &Type, reason: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid map key type")
        .with_infer_code("non_comparable_map_key")
        .with_span_label(&span, format!("`{}` is not comparable", key_ty))
        .with_help(format!(
            "Map keys must be comparable in Go. {} cannot be used as map keys.",
            reason
        ))
}

pub fn ref_of_interface_type(inner_ty: &Type, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid use of `Ref` with interface")
        .with_infer_code("ref_of_interface")
        .with_span_label(&span, "not allowed")
        .with_help(format!(
            "Use `{}` instead of `Ref<{}>`. Interfaces are already reference types in Go.",
            inner_ty, inner_ty
        ))
}

pub fn float_modulo_not_supported(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid operation")
        .with_infer_code("float_modulo")
        .with_span_label(&span, "`%` is not supported on floating-point types")
        .with_help("Use `math.Mod(x, y)` for floating-point modulo")
}

pub fn recursive_type(type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Recursive type has infinite size")
        .with_infer_code("recursive_type")
        .with_span_label(
            &span,
            format!("`{}` contains itself without indirection", type_name),
        )
        .with_help(format!(
            "Use `Ref<{}>` for indirection. For example: `next: Option<Ref<{}>>`",
            type_name, type_name
        ))
}

pub fn interface_self_embedding(interface_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Recursive interface embedding")
        .with_infer_code("interface_cycle")
        .with_span_label(&span, format!("`{}` embeds itself", interface_name))
        .with_help("An interface cannot embed itself. Remove the self-referencing `impl`.")
}

pub fn interface_embedding_cycle(cycle: &[String], span: Span) -> LisetteDiagnostic {
    let cycle_str = cycle.join(" → ");
    LisetteDiagnostic::error("Recursive interface embedding")
        .with_infer_code("interface_cycle")
        .with_span_label(&span, "creates a cycle")
        .with_help(format!(
            "Interface embedding cycle detected: {}. Break the cycle by removing one of the embeddings.",
            cycle_str
        ))
}

pub fn interface_method_conflict(
    interface_name: &str,
    method_name: &str,
    parent1: &str,
    parent2: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Conflicting method signatures")
        .with_infer_code("interface_method_conflict")
        .with_span_label(&span, format!("duplicate method `{}`", method_name))
        .with_help(format!(
            "Interface `{}` inherits conflicting definitions of `{}` from `{}` and `{}`. \
             Rename one of the methods or remove one of the embeddings.",
            interface_name, method_name, parent1, parent2
        ))
}

pub fn impl_on_foreign_type(type_name: &str, module_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot implement methods on foreign type")
        .with_infer_code("impl_on_foreign_type")
        .with_span_label(
            &span,
            format!("`{}` is defined in module `{}`", type_name, module_name),
        )
        .with_help(format!(
            "Methods can only be defined on types in the same module. \
             Use a standalone function instead: `fn my_method(w: {}) {{ ... }}`",
            type_name
        ))
}

pub fn impl_on_type_alias(_type_name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot implement methods on type alias")
        .with_infer_code("impl_on_type_alias")
        .with_span_label(&span, "type alias")
        .with_help(
            "A type alias cannot carry its own methods. Either add methods to the underlying \
             type directly or define a tuple struct instead",
        )
}

pub fn prelude_type_shadowed(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot shadow prelude type")
        .with_infer_code("prelude_type_shadowed")
        .with_span_label(&span, format!("`{}` is a prelude type", name))
        .with_help(format!(
            "Choose a different name — `{}` is defined in the prelude and cannot be redefined",
            name
        ))
}

pub fn prelude_function_shadowed(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot shadow prelude function")
        .with_infer_code("prelude_function_shadowed")
        .with_span_label(&span, format!("`{}` is a prelude function", name))
        .with_help(format!(
            "Choose a different name — `{}` is defined in the prelude and cannot be redefined",
            name
        ))
}

pub fn non_pub_interface_with_pub_impl(
    interface_name: &str,
    struct_name: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Visibility mismatch in interface implementation")
        .with_infer_code("non_pub_interface_pub_impl")
        .with_span_label(
            &span,
            "has public methods, but interface is private",
        )
        .with_help(format!(
            "`{}` implements public methods for the private interface `{}`. Either make the interface `pub`, or remove `pub` from the struct methods",
            struct_name, interface_name
        ))
}

pub fn missing_constraint_on_generic_return_type(
    fn_name: &str,
    param_name: &str,
    constraint: &Type,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Missing constraint on generic return type")
        .with_infer_code("missing_constraint_on_return_type")
        .with_span_label(
            &span,
            format!("expected `{}` to be constrained", param_name),
        )
        .with_help(
            format!(
                "Constrain the generic: `{}<{}: {}>()`",
                fn_name, param_name, constraint
            ) + ". The function returns a type whose methods depend on the constraint",
        )
}

pub fn panic_in_expression_position(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`panic()` used as a value")
        .with_infer_code("panic_in_expression_position")
        .with_span_label(&span, "disallowed")
        .with_help("`panic()` can only be used in statement position, not assigned to a variable or passed as an argument")
}

pub fn specialized_impl_cannot_satisfy_interface(
    struct_name: &str,
    interface_name: &str,
    method_name: &str,
    generics: &[String],
    span: Span,
) -> LisetteDiagnostic {
    let params = generics.join(", ");
    LisetteDiagnostic::error("Specialized impl cannot satisfy interface")
        .with_infer_code("specialized_impl_cannot_satisfy_interface")
        .with_span_label(
            &span,
            format!(
                "`{}` on `{}` cannot satisfy `{}`",
                method_name, struct_name, interface_name
            ),
        )
        .with_help(format!(
            "Methods in specialized `impl` blocks cannot satisfy interfaces. \
             Move `{}` to a generic `impl` block: `impl<{params}> {}<{params}> {{}}`",
            method_name, struct_name
        ))
}

pub fn native_method_value(method: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use native method as a value")
        .with_infer_code("native_method_value")
        .with_span_label(&span, "native methods must be called directly")
        .with_help(format!(
            "Use a closure instead: `|args| receiver.{}(args)`",
            method
        ))
}

pub fn native_constructor_value(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use native constructor as a value")
        .with_infer_code("native_constructor_value")
        .with_span_label(&span, "native constructors must be called directly")
        .with_help(format!("Use a closure instead: `|args| {name}(args)`"))
}

pub fn enum_variant_constructor_value(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use enum variant as value")
        .with_infer_code("enum_variant_constructor_value")
        .with_span_label(&span, "used as value")
        .with_help(format!(
            "Instantiate the variant: `{name} {{ field: value, ... }}`"
        ))
}

pub fn record_struct_value(name: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use struct type as a value")
        .with_infer_code("record_struct_value")
        .with_span_label(&span, "struct types cannot be used as expressions")
        .with_help(format!(
            "Use a struct literal instead: `{name} {{ field: value, ... }}`"
        ))
}

pub fn private_method_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use private method as a value")
        .with_infer_code("private_method_expression")
        .with_span_label(&span, "private methods must be called directly")
        .with_help("Use a closure instead: `|self_, args| self_.method(args)`")
}

pub fn float_literal_int_cast(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot cast float literal to integer directly")
        .with_infer_code("float_literal_int_cast")
        .with_span_label(&span, "unsupported cast")
        .with_help("Bind to a variable first: `let f = 1.0; f as int`")
}

pub fn const_requires_simple_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`const` requires a simple expression")
        .with_infer_code("const_requires_simple_expression")
        .with_span_label(&span, "expected literal or simple expression")
        .with_help("Use `let` for computed values")
}

pub fn complex_sub_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Complex expression used as sub-expression")
        .with_infer_code("complex_sub_expression")
        .with_span_label(&span, "expected simple expression")
        .with_help("Hoist to a `let` binding")
}

pub fn reference_through_newtype(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot take reference through newtype boundary")
        .with_infer_code("reference_through_newtype")
        .with_span_label(&span, "newtype `.0` inside `&`")
        .with_help("Bind the inner value first: `let inner = val.0; &inner`")
}

pub fn immutable_argument_to_mut_param(
    var_name: &str,
    span: Span,
    is_external: bool,
) -> LisetteDiagnostic {
    let help = if is_external {
        format!(
            "Bindings in Lisette are immutable by default. Use `let mut {} = ...` to allow mutation",
            var_name
        )
    } else {
        format!(
            "Bindings in Lisette are immutable by default. Use `let mut {} = ...` to allow mutation, \
             or make the parameter immutable by removing `mut` from the function signature",
            var_name
        )
    };
    LisetteDiagnostic::error("Immutable argument passed to `mut` parameter")
        .with_infer_code("immutable_arg_to_mut_param")
        .with_span_label(&span, "expected mutable, found immutable")
        .with_help(help)
}

pub fn failure_propagation_in_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Failure propagation in expression position")
        .with_infer_code("failure_propagation_in_expression")
        .with_span_label(
            &span,
            "`Err(..)?` and `None?` always early-return and never produce a value",
        )
        .with_help("Use `return Err(..)` or `return None` instead")
}

pub fn never_call_in_expression(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Never-returning call in expression position")
        .with_infer_code("never_call_in_expression")
        .with_span_label(&span, "`panic` never returns and cannot produce a value")
        .with_help("Use `panic(...)` as a statement instead")
}

pub fn invalid_main_signature(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid main signature")
        .with_infer_code("invalid_main_signature")
        .with_span_label(&span, "`main` must have no parameters and no return type")
        .with_help(
            "Use `fn main() { ... }`. To handle errors, use `match` or `if let` \
             inside main instead of returning `Result`.",
        )
}

pub fn parenthesized_qualifier(path: &str, member: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unnecessary parentheses around qualifier")
        .with_infer_code("parenthesized_qualifier")
        .with_span_label(&span, "parenthesized qualifier")
        .with_help(format!("Remove the parentheses: `{}.{}`", path, member))
}

pub fn type_alias_as_qualifier(
    alias: &str,
    underlying: &str,
    member: &str,
    span: Span,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot use generic type alias as qualifier")
        .with_infer_code("type_alias_as_qualifier")
        .with_span_label(
            &span,
            format!("`{}` aliases `{}`", alias, underlying),
        )
        .with_help(format!(
            "Aliases for types with generic parameters are not supported as qualifiers. Use the original type directly: `{}.{}`",
            underlying, member
        ))
}

pub fn control_flow_in_expression(keyword: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!(
        "`{}` cannot be used in expression position",
        keyword
    ))
    .with_infer_code("control_flow_in_expression")
    .with_span_label(
        &span,
        format!("`{}` is a statement and cannot produce a value", keyword),
    )
    .with_help(format!(
        "Use `{}` as a standalone statement instead",
        keyword
    ))
}

pub fn spread_on_non_variadic(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Invalid spread argument")
        .with_infer_code("spread_on_non_variadic")
        .with_span_label(&span, "this function does not accept variadic arguments")
        .with_help("Only functions with a `VarArgs<T>` parameter accept a spread argument")
}

pub fn reference_aliases_sibling(ref_span: Span, var_name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Reference aliases sibling expression")
        .with_infer_code("reference_aliases_sibling")
        .with_span_label(
            &ref_span,
            format!(
                "`&{}` could mutate `{}` used by a sibling",
                var_name, var_name
            ),
        )
        .with_help(format!(
            "Bind `{}` to a `let` before this expression to make evaluation order explicit",
            var_name
        ))
}
