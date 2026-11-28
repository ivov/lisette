use std::fs;

use rustc_hash::FxHashMap;
use syntax::ast::{Annotation, Expression};
use syntax::go_names::snake_to_camel;

use crate::_harness::go::prelude_dir;

const CALLED_WITHOUT_TYPE_ARGUMENTS: &[&str] = &["MapClone"];

fn type_parameter_names(list: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (index, character) in list.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth == 0 => {
                names.push(list[start..index].to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    names.push(list[start..].to_string());
    names
        .iter()
        .filter_map(|parameter| parameter.split_whitespace().next().map(str::to_string))
        .collect()
}

fn go_generic_functions() -> FxHashMap<String, Vec<String>> {
    let mut functions = FxHashMap::default();
    for entry in fs::read_dir(prelude_dir()).expect("read prelude dir") {
        let path = entry.expect("dir entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !name.ends_with(".go") || name.ends_with("_test.go") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read prelude Go file");
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("func ") else {
                continue;
            };
            let Some(open) = rest.find('[') else {
                continue;
            };
            if rest[..open].contains('(') {
                continue;
            }
            let mut depth = 0;
            let close = rest[open..]
                .char_indices()
                .find_map(|(index, character)| {
                    match character {
                        '[' => depth += 1,
                        ']' => depth -= 1,
                        _ => {}
                    }
                    (depth == 0).then_some(open + index)
                })
                .expect("type parameter list closes");
            functions.insert(
                rest[..open].to_string(),
                type_parameter_names(&rest[open + 1..close]),
            );
        }
    }
    functions
}

#[test]
fn prelude_go_type_parameters_follow_impl_then_method_generics() {
    let functions = go_generic_functions();
    let result = syntax::build_ast(stdlib::LIS_PRELUDE_SOURCE, 0);
    assert!(result.errors.is_empty(), "prelude parses");
    let mut checked = 0;
    let mut offenders = Vec::new();
    for item in &result.ast {
        let Expression::ImplBlock {
            annotation: Annotation::Constructor { name, .. },
            generics,
            methods,
            ..
        } = item
        else {
            continue;
        };
        for method in methods {
            let Expression::Function {
                name: method_name,
                generics: method_generics,
                ..
            } = method
            else {
                continue;
            };
            let go_name = format!("{}{}", snake_to_camel(name), snake_to_camel(method_name));
            if CALLED_WITHOUT_TYPE_ARGUMENTS.contains(&go_name.as_str()) {
                continue;
            }
            let Some(go_parameters) = functions.get(&go_name) else {
                continue;
            };
            checked += 1;
            let expected: Vec<String> = generics
                .iter()
                .chain(method_generics)
                .map(|generic| generic.name.to_string())
                .collect();
            if *go_parameters != expected {
                offenders.push(format!(
                    "{go_name}: Go has [{}], Lisette has [{}]",
                    go_parameters.join(", "),
                    expected.join(", ")
                ));
            }
        }
    }
    assert!(checked > 0, "the test finds prelude Go functions");
    assert!(
        offenders.is_empty(),
        "the Go type parameters of a prelude method are the impl generics, then the method generics:\n{}",
        offenders.join("\n")
    );
}
