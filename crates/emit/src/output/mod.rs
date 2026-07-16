pub mod imports;

use std::io::{self, Write};
use std::process::{Command, Stdio};

use diagnostics::LisetteDiagnostic;

use self::imports::format_import;
use crate::expressions::top_items::emit_doc;

#[derive(Clone, Debug)]
pub struct OutputFile {
    pub name: String,
    pub source: String,
    /// `(path, alias)` pairs; a path may appear twice when a generated
    /// import coexists with a source alias of the same package.
    pub imports: Vec<(String, String)>,
    pub package_name: String,
    pub file_comment: Option<String>,
    pub diagnostics: Vec<LisetteDiagnostic>,
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

        let header = emit_doc(&self.file_comment);
        if !header.is_empty() {
            output.collect(header.trim_end_matches('\n'));
            output.collect("");
        }

        output.collect(format!("package {}", self.package_name));

        match self.imports.as_slice() {
            [] => {}
            [(path, alias)] => {
                output.collect(format!("import {}", format_import(path, alias)));
            }
            entries => {
                output.collect("import (");
                for (path, alias) in entries {
                    output.collect(format_import(path, alias));
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

    pub(crate) fn collect(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
    }

    pub(crate) fn collect_with_blank(&mut self, line: impl Into<String>) {
        self.output.push(line.into());
        self.output.push(String::new());
    }
}
