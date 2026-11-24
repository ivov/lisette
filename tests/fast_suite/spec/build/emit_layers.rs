use std::collections::BTreeMap;

use super::package_namespace::source_files;

const EMIT_SOURCE: &str = "crates/emit/src/";

const RANKS: [(&str, usize); 8] = [
    ("target/names", 0),
    ("target/types", 1),
    ("target/ir", 2),
    ("lookup/facts", 3),
    ("lookup/state", 4),
    ("output/finish", 5),
    ("output/render", 5),
    ("lowering", 6),
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
        let Some(module) = module_of(&file_segments(path)) else {
            continue;
        };
        let Some(own_rank) = rank(&module) else {
            failures.push(format!("{path}: module `{module}` has no layer in `RANKS`"));
            continue;
        };
        if module == "lowering" {
            continue;
        }
        for segments in referenced_paths(path, text, &roots) {
            let Some(target) = module_of(&segments) else {
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
