use rustc_hash::FxHashMap as HashMap;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use super::imports::format_import;

#[derive(Clone, Debug)]
pub struct OutputFile {
    pub name: String,
    pub source: String,
    pub imports: HashMap<String, String>,
    pub package_name: String,
}

impl OutputFile {
    pub fn to_go(&self) -> String {
        let unformatted = self.render_unformatted();
        self.gofmt(&unformatted).unwrap_or(unformatted)
    }

    fn render_unformatted(&self) -> String {
        let mut output = OutputCollector::new();

        output.collect(format!("package {}", self.package_name));

        match self.imports.len() {
            0 => {}
            1 => {
                let (path, alias) = self
                    .imports
                    .iter()
                    .next()
                    .expect("single-element collection must have first element");
                output.collect(format!("import {}", format_import(path, alias)));
            }
            _ => {
                output.collect("import (");
                for (path, alias) in &self.imports {
                    output.collect(format_import(path, alias));
                }
                output.collect(")");
            }
        }

        output.collect(&self.source);
        output.render()
    }

    fn gofmt(&self, code: &str) -> Result<String, io::Error> {
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
