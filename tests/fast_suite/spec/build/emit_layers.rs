use std::collections::BTreeMap;

use super::package_namespace::source_files;

const EMIT_SOURCE: &str = "crates/emit/src/";

const RANKS: [(&str, usize); 11] = [
    ("target/names", 0),
    ("target/types", 1),
    ("target/ir", 2),
    ("lookup/facts", 3),
    ("lookup/state", 4),
    ("output/finish", 5),
    ("output/render", 5),
    ("lowering/plan", 6),
    ("lowering/planner", 7),
    ("lowering", 8),
    ("lowering/driver", 9),
];

fn rank(module: &str) -> Option<usize> {
    RANKS
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, rank)| *rank)
}

fn module_of(segments: &[String]) -> Option<String> {
    match segments {
        [first, ..] if first == "lowering" => Some(first.clone()),
        [first, second, ..] if ["target", "lookup", "output"].contains(&first.as_str()) => {
            Some(format!("{first}/{second}"))
        }
        _ => None,
    }
}

fn layer_of(segments: &[String]) -> Option<String> {
    match segments {
        [first, second, ..]
            if first == "lowering" && ["plan", "planner", "driver"].contains(&second.as_str()) =>
        {
            Some(format!("{first}/{second}"))
        }
        _ => module_of(segments),
    }
}

fn file_segments(path: &str) -> Vec<String> {
    let relative = path.strip_prefix(EMIT_SOURCE).unwrap_or(path);
    let mut segments: Vec<String> = relative
        .trim_end_matches(".rs")
        .split('/')
        .map(str::to_string)
        .collect();
    if segments.last().is_some_and(|last| last == "mod") {
        segments.pop();
    }
    segments
}

fn identifier(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    text.split_at(end)
}

fn skip_whitespace(text: &str, position: usize) -> usize {
    position + (text[position..].len() - text[position..].trim_start().len())
}

fn skip_rename(text: &str, position: usize) -> usize {
    let start = skip_whitespace(text, position);
    let Some(rest) = text[start..].strip_prefix("as ") else {
        return position;
    };
    let alias_start = skip_whitespace(rest, 0);
    let (alias, _) = identifier(&rest[alias_start..]);
    start + "as ".len() + alias_start + alias.len()
}

fn use_tree(text: &str, prefix: &[String], out: &mut Vec<Vec<String>>) -> usize {
    let mut path = prefix.to_vec();
    let mut position = 0;
    loop {
        position = skip_whitespace(text, position);
        if text[position..].starts_with('{') {
            position += 1;
            loop {
                position = skip_whitespace(text, position);
                let rest = &text[position..];
                if rest.is_empty() {
                    return position;
                }
                if rest.starts_with('}') {
                    return position + 1;
                }
                let used = use_tree(rest, &path, out);
                position = skip_whitespace(text, position + used);
                if text[position..].starts_with(',') || used == 0 {
                    position += 1;
                }
            }
        }
        let (name, _) = identifier(&text[position..]);
        if name.is_empty() {
            out.push(path);
            return position;
        }
        path.push(name.to_string());
        position += name.len();
        if text[position..].starts_with("::") {
            position += 2;
        } else {
            out.push(path);
            return skip_rename(text, position);
        }
    }
}

fn referenced_paths(
    path: &str,
    text: &str,
    roots: &BTreeMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    let own = file_segments(path);
    let mut found = Vec::new();
    for (index, _) in text.match_indices("crate::") {
        if text[..index].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let mut paths = Vec::new();
        use_tree(&text[index + "crate::".len()..], &[], &mut paths);
        for segments in paths {
            match segments.first().and_then(|first| roots.get(first)) {
                Some(real) => found.push(real.clone()),
                None => found.push(segments),
            }
        }
    }
    for (index, _) in text.match_indices("super::") {
        if text[..index].ends_with("super::")
            || text[..index].ends_with(|c: char| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let mut rest = &text[index..];
        let mut base = own.clone();
        while let Some(after) = rest.strip_prefix("super::") {
            base.pop();
            rest = after;
        }
        let mut paths = Vec::new();
        use_tree(rest, &base, &mut paths);
        found.extend(paths);
    }
    found
}

fn root_reexports(lib: &str) -> BTreeMap<String, Vec<String>> {
    let mut roots = BTreeMap::new();
    for line in lib.lines() {
        let Some(tree) = line.strip_prefix("pub use ") else {
            continue;
        };
        let mut paths = Vec::new();
        use_tree(tree, &[], &mut paths);
        for segments in paths {
            if let Some(name) = segments.last() {
                roots.insert(name.clone(), segments.clone());
            }
        }
    }
    roots
}

#[test]
fn emit_layers_use_only_lower_layers() {
    let files = source_files("crates/emit/src");
    let lib = files
        .iter()
        .find(|(path, _)| path == "crates/emit/src/lib.rs")
        .map(|(_, text)| text.as_str())
        .expect("emit has a lib.rs");
    let roots = root_reexports(lib);
    let mut failures = Vec::new();
    for (path, text) in &files {
        let Some(module) = layer_of(&file_segments(path)) else {
            continue;
        };
        let Some(own_rank) = rank(&module) else {
            failures.push(format!("{path}: module `{module}` has no layer in `RANKS`"));
            continue;
        };
        for segments in referenced_paths(path, text, &roots) {
            if segments == ["lowering"] {
                continue;
            }
            let Some(target) = layer_of(&segments) else {
                continue;
            };
            if target == module {
                continue;
            }
            match rank(&target) {
                Some(target_rank) if target_rank < own_rank => {}
                _ => failures.push(format!(
                    "{path}: `{module}` uses `{target}` ({})",
                    segments.join("::")
                )),
            }
        }
    }
    failures.sort();
    failures.dedup();
    assert!(
        failures.is_empty(),
        "each emit module may use only modules in lower layers:\n{}",
        failures.join("\n")
    );
}

fn strip_visibility(line: &str) -> (&str, Option<&str>) {
    let Some(rest) = line.strip_prefix("pub") else {
        return (line, None);
    };
    if let Some(inner) = rest.strip_prefix('(') {
        let end = inner.find(')').unwrap_or(inner.len());
        let after = inner.get(end + 1..).unwrap_or("").trim_start();
        return (after, Some(&inner[..end]));
    }
    (rest.trim_start(), Some("crate"))
}

fn last_type_segment(text: &str) -> String {
    let text = text.trim_start_matches(['&', ' ']);
    let text = text.strip_prefix("mut ").unwrap_or(text);
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
        .unwrap_or(text.len());
    text[..end].rsplit("::").next().unwrap_or("").to_string()
}

fn defined_types(files: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut types: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, text) in files {
        let Some(module) = module_of(&file_segments(path)) else {
            continue;
        };
        for line in text.lines() {
            let (item, _) = strip_visibility(line.trim_start());
            for keyword in ["struct ", "enum ", "type ", "trait "] {
                if let Some(rest) = item.strip_prefix(keyword) {
                    let (name, _) = identifier(rest);
                    types
                        .entry(name.to_string())
                        .or_default()
                        .push(module.clone());
                }
            }
        }
    }
    types
}

fn impl_target(header: &str) -> (bool, String) {
    let mut rest = header.trim_start().trim_start_matches("impl");
    if rest.starts_with('<') {
        let mut depth = 0;
        for (index, c) in rest.char_indices() {
            depth += match c {
                '<' => 1,
                '>' => -1,
                _ => 0,
            };
            if depth == 0 {
                rest = &rest[index + 1..];
                break;
            }
        }
    }
    let rest = rest.split(" where").next().unwrap_or(rest);
    let rest = rest.trim_end_matches(['{', ' ']);
    match rest.split_once(" for ") {
        Some((_, target)) => (true, last_type_segment(target)),
        None => (false, last_type_segment(rest)),
    }
}

fn visible_outside(path: &str, module: &str, restriction: &str) -> bool {
    let scope = match restriction {
        "self" => return false,
        "crate" => return true,
        "super" => {
            let mut segments = file_segments(path);
            segments.pop();
            segments
        }
        other => match other.strip_prefix("in crate::") {
            Some(scope) => scope.split("::").map(str::to_string).collect(),
            None => return true,
        },
    };
    module_of(&scope).is_none_or(|scope_module| scope_module != module)
}

#[test]
fn emit_types_get_callable_methods_only_from_their_own_module() {
    let files = source_files("crates/emit/src");
    let types = defined_types(&files);
    let mut failures = Vec::new();
    for (path, text) in &files {
        let Some(module) = module_of(&file_segments(path)) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("impl") {
                continue;
            }
            let indent = &line[..line.len() - line.trim_start().len()];
            let header_end = (index..lines.len())
                .find(|&end| lines[end].trim_end().ends_with('{'))
                .unwrap_or(index);
            let header = lines[index..=header_end].join(" ");
            let (is_trait, target) = impl_target(&header);
            let Some(homes) = types.get(&target) else {
                continue;
            };
            if homes.contains(&module) {
                continue;
            }
            let closing = format!("{indent}}}");
            let body = lines[header_end + 1..]
                .iter()
                .take_while(|body_line| **body_line != closing);
            let exposed = is_trait
                || body.into_iter().any(|body_line| {
                    let (item, restriction) = strip_visibility(body_line.trim_start());
                    item.starts_with("fn ")
                        && restriction.is_some_and(|scope| visible_outside(path, &module, scope))
                });
            if exposed {
                failures.push(format!(
                    "{path}:{}: `{module}` adds callable methods to `{target}` from `{}`",
                    index + 1,
                    homes.join("`, `")
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "add a method to a type in the module that defines the type, or keep it private to its own module:\n{}",
        failures.join("\n")
    );
}

#[test]
fn emit_exempts_only_lookup_facts_from_disallowed_methods() {
    let files = source_files("crates/emit/src");
    let mut failures = Vec::new();
    for (path, text) in &files {
        if module_of(&file_segments(path)).as_deref() == Some("lookup/facts") {
            continue;
        }
        for (index, line) in text.lines().enumerate() {
            if line.contains("clippy::disallowed_methods") {
                failures.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "reduce types through an `EmitFacts` query in `lookup/facts` instead of exempting code from `disallowed_methods`:\n{}",
        failures.join("\n")
    );
}
