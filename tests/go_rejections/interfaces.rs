//! Interface satisfaction in two-package programs, against the emitted Go of a skeleton that only prints the value.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::{Case, Compiled, Family, GoFiles, compile_case};

pub const FAMILY: Family = Family {
    name: "interfaces",
    intended: &[],
};

const TYPE_KINDS: &[&str] = &[
    "struct",
    "newtype",
    "enum",
    "generic",
    "generic_specialized",
    "alias",
    "embedded",
];
const RECEIVERS: &[&str] = &["value", "ref", "mut_ref"];
const USE_VALUES: &[&str] = &["value", "ref"];
const METHOD_VISIBILITIES: &[&str] = &["pub", "private"];
const INTERFACE_SHAPES: &[&str] = &["match", "return_mismatch", "missing", "generic_interface"];
const SITES: &[&str] = &["let", "argument", "bound", "slice"];
const LOCATIONS: &[&str] = &["same", "interface_other", "type_other", "both_other"];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Shape {
    type_kind: &'static str,
    receiver: &'static str,
    use_value: &'static str,
    method_visibility: &'static str,
    interface: &'static str,
    location: &'static str,
}

fn type_source(shape: &Shape, public: bool) -> String {
    let visibility = if public { "pub " } else { "" };
    let method_visibility = if shape.method_visibility == "pub" {
        "pub "
    } else {
        ""
    };
    let target = match shape.type_kind {
        "generic" => "S<T>",
        "generic_specialized" => "S<int>",
        "embedded" => "Inner",
        _ => "S",
    };
    let head = match shape.type_kind {
        "generic" => "impl<T> S<T>",
        "generic_specialized" => "impl S<int>",
        "embedded" => "impl Inner",
        _ => "impl S",
    };
    let receiver = match shape.receiver {
        "value" => "self".to_string(),
        "ref" => format!("self: Ref<{target}>"),
        _ => format!("self: mut Ref<{target}>"),
    };
    let (declaration, make) = match shape.type_kind {
        "struct" => (format!("{visibility}struct S {{ x: int }}\n"), "S { x: 1 }"),
        "newtype" => (format!("{visibility}struct S(int)\n"), "S(1)"),
        "enum" => (format!("{visibility}enum S {{ A, B }}\n"), "S.A"),
        "generic" | "generic_specialized" => (
            format!("{visibility}struct S<T> {{ x: T }}\n"),
            "S { x: 1 }",
        ),
        "alias" => (
            format!("{visibility}struct S0 {{ x: int }}\n{visibility}type S = S0\n"),
            "S0 { x: 1 }",
        ),
        _ => (
            format!(
                "{visibility}struct Inner {{ x: int }}\n{visibility}struct S {{ embed Inner }}\n"
            ),
            "S { Inner: Inner { x: 1 } }",
        ),
    };
    let return_type = if shape.type_kind.starts_with("generic") {
        "S<int>"
    } else {
        "S"
    };
    format!(
        "{declaration}\n{head} {{\n  {method_visibility}fn m({receiver}) -> int {{\n    1\n  }}\n}}\n\n{visibility}fn make() -> {return_type} {{\n  {make}\n}}\n"
    )
}

fn interface_source(shape: &str, public: bool) -> String {
    let visibility = if public { "pub " } else { "" };
    match shape {
        "match" => format!("{visibility}interface I {{\n  fn m() -> int\n}}\n"),
        "return_mismatch" => format!("{visibility}interface I {{\n  fn m() -> int64\n}}\n"),
        "missing" => {
            format!("{visibility}interface I {{\n  fn m() -> int\n  fn n() -> int\n}}\n")
        }
        _ => format!("{visibility}interface I<T> {{\n  fn m() -> T\n}}\n"),
    }
}

/// The program files, where `site` is `None` for the skeleton, whose use site prints the value.
fn program(shape: &Shape, site: Option<&str>) -> Vec<(String, String)> {
    let type_in_lib = matches!(shape.location, "type_other" | "both_other");
    let interface_in_lib = matches!(shape.location, "interface_other" | "both_other");
    let type_arguments = if shape.interface == "generic_interface" {
        "<int>"
    } else {
        ""
    };
    let interface = format!(
        "{}{type_arguments}",
        if interface_in_lib { "lib.I" } else { "I" }
    );
    let make = if type_in_lib { "lib.make()" } else { "make()" };
    let mut main = String::from("import \"go:fmt\"\nimport \"lib\"\n\n");
    let mut lib = String::new();
    if type_in_lib {
        lib.push_str(&type_source(shape, true));
        lib.push('\n');
    } else {
        main.push_str(&type_source(shape, false));
        main.push('\n');
    }
    if interface_in_lib {
        lib.push_str(&interface_source(shape.interface, true));
        lib.push('\n');
    } else {
        main.push_str(&interface_source(shape.interface, false));
        main.push('\n');
    }
    if lib.is_empty() {
        lib.push_str("pub fn unused() -> int {\n  1\n}\n");
        main.push_str("fn keep() -> int {\n  lib.unused()\n}\n\n");
    }
    let value = if shape.use_value == "value" {
        "s"
    } else {
        "&s"
    };
    main.push_str(&format!(
        "fn use_interface(x: {interface}) {{\n  fmt.Println(x)\n}}\n\nfn use_bound<T: {interface}>(x: T) {{\n  fmt.Println(x)\n}}\n\n"
    ));
    let use_site = match site {
        None => format!("  fmt.Println({value})\n"),
        Some("let") => format!("  let x: {interface} = {value}\n  fmt.Println(x)\n"),
        Some("argument") => format!("  use_interface({value})\n"),
        Some("bound") => format!("  use_bound({value})\n"),
        Some(_) => format!("  let xs: Slice<{interface}> = [{value}]\n  fmt.Println(xs)\n"),
    };
    main.push_str(&format!(
        "fn main() {{\n  let mut s = {make}\n{use_site}}}\n"
    ));
    vec![
        ("main.lis".to_string(), main),
        ("lib/lib.lis".to_string(), lib),
    ]
}

/// Puts the Go use site in place of the last print line of `func main`.
fn splice(go_main: &str, site: &str) -> Option<String> {
    let start = go_main.find("func main() {\n")? + "func main() {\n".len();
    let end = start + go_main[start..].find("\n}")?;
    let mut lines: Vec<String> = go_main[start..end]
        .split('\n')
        .map(str::to_string)
        .collect();
    let index = lines
        .iter()
        .rposition(|line| line.contains("fmt.Println("))?;
    let value = lines[index]
        .trim()
        .strip_prefix("fmt.Println(")?
        .strip_suffix(')')?
        .to_string();
    let parameter_start = go_main.find("func useInterface(x ")? + "func useInterface(x ".len();
    let interface =
        &go_main[parameter_start..parameter_start + go_main[parameter_start..].find(')')?];
    lines[index] = match site {
        "let" => format!("\tvar x {interface} = {value}\n\tfmt.Println(x)"),
        "argument" => format!("\tuseInterface({value})"),
        "bound" => format!("\tuseBound({value})"),
        _ => format!("\txs := []{interface}{{{value}}}\n\tfmt.Println(xs)"),
    };
    Some(format!(
        "{}{}{}",
        &go_main[..start],
        lines.join("\n"),
        &go_main[end..]
    ))
}

fn reference(skeleton: &GoFiles, site: &str) -> Option<GoFiles> {
    let mut files = skeleton.clone();
    let (_, main) = files.iter_mut().find(|(name, _)| name == "main.go")?;
    *main = splice(main, site)?;
    Some(files)
}

fn shapes() -> Vec<Shape> {
    let mut shapes = vec![Shape {
        type_kind: "",
        receiver: "",
        use_value: "",
        method_visibility: "",
        interface: "",
        location: "",
    }];
    shapes = expand(shapes, TYPE_KINDS, |shape, value| shape.type_kind = value);
    shapes = expand(shapes, RECEIVERS, |shape, value| shape.receiver = value);
    shapes = expand(shapes, USE_VALUES, |shape, value| shape.use_value = value);
    shapes = expand(shapes, METHOD_VISIBILITIES, |shape, value| {
        shape.method_visibility = value;
    });
    shapes = expand(shapes, INTERFACE_SHAPES, |shape, value| {
        shape.interface = value
    });
    expand(shapes, LOCATIONS, |shape, value| shape.location = value)
}

fn expand(
    shapes: Vec<Shape>,
    values: &[&'static str],
    set: fn(&mut Shape, &'static str),
) -> Vec<Shape> {
    shapes
        .into_iter()
        .flat_map(|shape| {
            values.iter().map(move |value| {
                let mut shape = shape;
                set(&mut shape, value);
                shape
            })
        })
        .collect()
}

pub fn cases() -> Vec<Case> {
    let selected: Vec<(Shape, &str, String)> = shapes()
        .into_iter()
        .flat_map(|shape| SITES.iter().map(move |site| (shape, *site)))
        .map(|(shape, site)| {
            let id = format!(
                "{}/{}/{}/{}/{}/{site}/{}",
                shape.type_kind,
                shape.receiver,
                shape.use_value,
                shape.method_visibility,
                shape.interface,
                shape.location
            );
            (shape, site, id)
        })
        .collect();
    let mut unique: Vec<Shape> = selected.iter().map(|(shape, ..)| *shape).collect();
    unique.sort_by_key(shape_key);
    unique.dedup();
    let skeletons: HashMap<Shape, Option<GoFiles>> = unique
        .into_par_iter()
        .map(|shape| {
            let skeleton = match compile_case(&program(&shape, None)) {
                Compiled::Accepted(files) => Some(files),
                Compiled::Rejected(_) | Compiled::Failed(_) => None,
            };
            (shape, skeleton)
        })
        .collect();
    selected
        .into_iter()
        .map(|(shape, site, id)| Case {
            id,
            files: program(&shape, Some(site)),
            reference: skeletons[&shape]
                .as_ref()
                .and_then(|skeleton| reference(skeleton, site)),
        })
        .collect()
}

fn shape_key(shape: &Shape) -> [&'static str; 6] {
    [
        shape.type_kind,
        shape.receiver,
        shape.use_value,
        shape.method_visibility,
        shape.interface,
        shape.location,
    ]
}
