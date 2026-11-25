use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::lookup::state::imports::OutputImport;
use crate::target::names::go_name;

#[derive(Clone, Debug)]
pub struct OutputFile {
    pub name: String,
    pub(crate) source: String,
    pub imports: Vec<OutputImport>,
    pub package_name: String,
    pub(crate) file_comment: Option<String>,
}

impl OutputFile {
    pub fn to_go(&self) -> String {
        let unformatted = self.render_unformatted();
        gofmt(&unformatted).unwrap_or(unformatted)
    }

    pub fn to_go_unformatted(&self) -> String {
        self.render_unformatted()
    }

    fn render_unformatted(&self) -> String {
        let mut output = OutputCollector::new();

        let header = doc_text(&self.file_comment);
        if !header.is_empty() {
            output.collect(header.trim_end_matches('\n'));
            output.collect("");
        }

        output.collect(format!("package {}", self.package_name));

        match self.imports.as_slice() {
            [] => {}
            [entry] => {
                output.collect(format!(
                    "import {}",
                    format_import(&entry.path, entry.qualifier.as_deref())
                ));
            }
            entries => {
                output.collect("import (");
                let groups = entries.chunk_by(|left, right| left.group == right.group);
                for (index, group) in groups.enumerate() {
                    if index > 0 {
                        output.collect("");
                    }
                    for entry in group {
                        output.collect(format_import(&entry.path, entry.qualifier.as_deref()));
                    }
                }
                output.collect(")");
            }
        }

        output.collect(&self.source);
        output.render()
    }
}

fn gofmt(code: &str) -> Result<String, io::Error> {
    let mut child = Command::new("gofmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(io::Error::other("Failed to open stdin"));
    };

    stdin.write_all(code.as_bytes())?;
    drop(stdin);

    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(io::Error::other("gofmt failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Default)]
pub(crate) struct OutputCollector {
    output: Vec<String>,
}

impl OutputCollector {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn render(&self) -> String {
        self.output.join("\n")
    }

    fn collect(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
    }

    pub(crate) fn collect_with_blank(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
        self.output.push(String::new());
    }
}

pub(crate) fn doc_text(doc: &Option<String>) -> String {
    doc.as_deref().map_or_else(String::new, doc_lines)
}

pub(crate) fn doc_lines(text: &str) -> String {
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                "//".to_string()
            } else {
                format!("// {}", line)
            }
        })
        .collect();
    format!("{}\n", lines.join("\n"))
}

pub(crate) fn format_import(path: &str, qualifier: Option<&str>) -> String {
    match qualifier {
        None => format!("_ \"{path}\""),
        Some(qualifier) if qualifier == go_name::go_package_name(path) => format!("\"{path}\""),
        Some(qualifier) => format!("{qualifier} \"{path}\""),
    }
}
