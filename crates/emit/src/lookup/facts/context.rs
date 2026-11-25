use crate::lookup::facts::catalog::GoAbiCatalog;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::ir::go_expression::Position;
use crate::target::types::enum_layout::EnumLayout;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use syntax::ast::Span;
use syntax::program::{Definition, DefinitionBody, GoMethodPolicy, is_prelude_symbol};
use syntax::types::Symbol;

#[derive(Default)]
pub(crate) struct GlobalEmitData {
    pub(crate) go_abi_catalog: GoAbiCatalog,
    pub(crate) method_policy: GoMethodPolicy,
    pub(crate) tagged_method_names: FxHashSet<String>,
    pub(crate) enum_layouts: FxHashMap<Symbol, EnumLayout>,
}

impl GlobalEmitData {
    pub(crate) fn compute(definitions: &FxHashMap<Symbol, Definition>) -> Self {
        let policy = GoMethodPolicy::compute(definitions.iter(), |id| definitions.get(id));
        let mut globals = GlobalEmitData {
            go_abi_catalog: GoAbiCatalog::from_definitions(definitions),
            tagged_method_names: policy.tagged_method_names.iter().cloned().collect(),
            method_policy: policy,
            enum_layouts: FxHashMap::default(),
        };

        for (key, definition) in definitions.iter() {
            globals.register_enum_layout(key, definition, definitions);
        }

        globals
    }

    fn register_enum_layout(
        &mut self,
        key: &Symbol,
        definition: &Definition,
        definitions: &FxHashMap<Symbol, Definition>,
    ) {
        let DefinitionBody::Enum {
            generics,
            variants,
            default_variant,
            ..
        } = &definition.body
        else {
            return;
        };

        if is_prelude_symbol(key) {
            return;
        }
        let layout = EnumLayout::new(key, generics, variants, *default_variant, |id| {
            definitions.get(id)
        });
        self.enum_layouts.insert(key.clone(), layout);
    }
}

pub(crate) struct SharedEmitContext<'a> {
    pub(crate) go_module: &'a str,
    pub(crate) entry_package_name: &'a str,
    pub(crate) emit_tests: bool,
    pub(crate) line_indexes: Option<FxHashMap<u32, LineIndex>>,
    pub(crate) globals: GlobalEmitData,
}

impl EmitFacts<'_> {
    pub(crate) fn position_at(&self, span: &Span, byte_offset: u32) -> Option<Position> {
        if span.is_dummy() {
            return None;
        }
        let source = self.line_index(span.file_id)?;
        Some(Position {
            path: source.path.clone(),
            line: source.line_for_offset(byte_offset),
            column: source.col_for_offset(byte_offset),
        })
    }

    pub(crate) fn source_position(&self, span: &Span) -> Option<Position> {
        self.position_at(span, span.byte_offset)
    }

    pub(crate) fn end_position(&self, span: &Span) -> Option<Position> {
        self.position_at(span, span.byte_offset + span.byte_length)
    }
}

#[derive(Clone)]
pub(crate) struct LineIndex {
    pub(crate) path: Arc<str>,
    line_offsets: Vec<u32>,
}

impl LineIndex {
    pub(crate) fn from_source(path: String, source: &str) -> Self {
        let mut line_offsets = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_offsets.push((i + 1) as u32);
            }
        }
        Self {
            path: path.into(),
            line_offsets,
        }
    }

    pub(crate) fn line_for_offset(&self, byte_offset: u32) -> usize {
        match self.line_offsets.binary_search(&byte_offset) {
            Ok(line) => line + 1,
            Err(line) => line,
        }
    }

    pub(crate) fn col_for_offset(&self, byte_offset: u32) -> usize {
        let line = self.line_for_offset(byte_offset);
        let line_start = self.line_offsets[line - 1];
        (byte_offset - line_start + 1) as usize
    }
}
