use crate::target::types::go_type::{ChannelDirection, GoType, GoTypeShape};

pub(in crate::output) fn go_type_text(go_type: &GoType) -> String {
    let mut text = String::new();
    write_go_type(&mut text, go_type);
    text
}

pub(in crate::output) fn needs_conversion_parentheses(go_type: &GoType) -> bool {
    matches!(
        printed(go_type).shape.as_ref(),
        GoTypeShape::Pointer(_) | GoTypeShape::Channel(ChannelDirection::Receive, _)
    ) || ends_in_function_without_result(go_type)
}

fn ends_in_function_without_result(go_type: &GoType) -> bool {
    match printed(go_type).shape.as_ref() {
        GoTypeShape::Function(_, None) => true,
        GoTypeShape::Function(_, Some(inner))
        | GoTypeShape::Slice(inner)
        | GoTypeShape::Array(_, inner)
        | GoTypeShape::Map(_, inner)
        | GoTypeShape::Channel(_, inner)
        | GoTypeShape::Pointer(inner) => ends_in_function_without_result(inner),
        GoTypeShape::Name(_)
        | GoTypeShape::Qualified { .. }
        | GoTypeShape::Instantiated { .. }
        | GoTypeShape::Arguments(_)
        | GoTypeShape::Results(_)
        | GoTypeShape::Variadic(_)
        | GoTypeShape::Struct(_)
        | GoTypeShape::EmptyInterface => false,
    }
}

fn printed(go_type: &GoType) -> &GoType {
    match &go_type.alias {
        Some(alias) => printed(alias),
        None => go_type,
    }
}

fn write_go_type(text: &mut String, go_type: &GoType) {
    match printed(go_type).shape.as_ref() {
        GoTypeShape::Name(name) => text.push_str(name),
        GoTypeShape::Qualified { package, member } => {
            text.push_str(package);
            text.push('.');
            text.push_str(member);
        }
        GoTypeShape::Instantiated { base, arguments } => {
            write_go_type(text, base);
            write_list(text, '[', arguments, ']');
        }
        GoTypeShape::Arguments(arguments) => write_list(text, '[', arguments, ']'),
        GoTypeShape::Results(results) => write_list(text, '(', results, ')'),
        GoTypeShape::Slice(element) => {
            text.push_str("[]");
            write_go_type(text, element);
        }
        GoTypeShape::Map(key, value) => {
            text.push_str("map[");
            write_go_type(text, key);
            text.push(']');
            write_go_type(text, value);
        }
        GoTypeShape::Pointer(pointee) => {
            text.push('*');
            write_go_type(text, pointee);
        }
        GoTypeShape::Array(length, element) => {
            text.push('[');
            text.push_str(length);
            text.push(']');
            write_go_type(text, element);
        }
        GoTypeShape::Channel(direction, element) => {
            text.push_str(match direction {
                ChannelDirection::Send => "chan<- ",
                ChannelDirection::Receive => "<-chan ",
                ChannelDirection::Bidirectional => "chan ",
            });
            write_go_type(text, element);
        }
        GoTypeShape::Variadic(element) => {
            text.push_str("...");
            write_go_type(text, element);
        }
        GoTypeShape::Function(parameters, result) => {
            write_function_type(text, parameters, result.as_deref())
        }
        GoTypeShape::Struct(fields) if fields.is_empty() => text.push_str("struct{}"),
        GoTypeShape::Struct(fields) => write_struct_type(text, fields),
        GoTypeShape::EmptyInterface => text.push_str("any"),
    }
}

fn write_function_type(text: &mut String, parameters: &[GoType], result: Option<&GoType>) {
    text.push_str("func");
    write_list(text, '(', parameters, ')');
    if let Some(result) = result {
        text.push(' ');
        write_go_type(text, result);
    }
}

fn write_struct_type(text: &mut String, fields: &[(String, GoType)]) {
    text.push_str("struct { ");
    for (index, (name, ty)) in fields.iter().enumerate() {
        if index > 0 {
            text.push_str("; ");
        }
        text.push_str(name);
        text.push(' ');
        write_go_type(text, ty);
    }
    text.push_str(" }");
}

fn write_list(text: &mut String, open: char, types: &[GoType], close: char) {
    text.push(open);
    for (index, ty) in types.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        write_go_type(text, ty);
    }
    text.push(close);
}
