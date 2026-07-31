use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unbroken,
    Broken,
    ForcedBroken,
    ForcedUnbroken,
}

/// For ASCII the byte length is already the grapheme count.
fn grapheme_count(text: &str) -> isize {
    if text.is_ascii() {
        text.len() as isize
    } else {
        text.graphemes(true).count() as isize
    }
}

fn fits<'a, 'doc>(
    limit: isize,
    mut current_width: isize,
    docs: &mut Vec<(isize, Mode, &'doc Document<'a>)>,
) -> bool {
    loop {
        if current_width > limit {
            return false;
        }

        let (indent, mode, document) = match docs.pop() {
            Some(x) => x,
            None => return true,
        };

        match document {
            Document::ForceBroken(doc) => match mode {
                Mode::ForcedBroken => docs.push((indent, mode, doc)),
                _ => return false,
            },

            Document::Newline => return true,

            Document::Nest(i, doc) => {
                docs.push((indent + i, mode, doc));
            }

            Document::NestIfBroken(_, doc) => {
                docs.push((indent, mode, doc));
            }

            Document::Group(doc) => match mode {
                Mode::Broken => docs.push((indent, Mode::Unbroken, doc)),
                _ => docs.push((indent, mode, doc)),
            },

            Document::MeasureFlat(doc) => {
                docs.push((indent, Mode::ForcedUnbroken, doc));
            }

            Document::Text(s) => {
                current_width += grapheme_count(s);
            }

            Document::VerbatimText(s) => {
                if s.contains('\n') {
                    return false;
                }
                current_width += grapheme_count(s);
            }

            Document::StrictBreak { unbroken, .. } | Document::FlexBreak { unbroken, .. } => {
                match mode {
                    Mode::Broken | Mode::ForcedBroken => return true,
                    Mode::Unbroken | Mode::ForcedUnbroken => {
                        current_width += unbroken.len() as isize;
                    }
                }
            }

            Document::NextBreakFits(doc) => match mode {
                Mode::ForcedUnbroken => docs.push((indent, mode, doc)),
                _ => docs.push((indent, Mode::ForcedBroken, doc)),
            },

            Document::NextBreakDoesNotFit(doc) => match mode {
                Mode::ForcedBroken => docs.push((indent, mode, doc)),
                _ => docs.push((indent, Mode::ForcedUnbroken, doc)),
            },

            Document::Sequence(vec) => {
                for doc in vec.iter().rev() {
                    docs.push((indent, mode, doc));
                }
            }
        }
    }
}

fn write_indent(output: &mut String, indent: isize) {
    for _ in 0..indent {
        output.push(' ');
    }
}

fn write_pending_indent(output: &mut String, pending_indent: &mut Option<isize>) {
    if let Some(indent) = pending_indent.take() {
        write_indent(output, indent);
    }
}

fn format<'a, 'doc>(
    output: &mut String,
    limit: isize,
    mut width: isize,
    mut docs: Vec<(isize, Mode, &'doc Document<'a>)>,
) {
    let mut pending_indent = None;
    let mut fits_buffer = Vec::new();

    while let Some((indent, mode, document)) = docs.pop() {
        match document {
            Document::Newline => {
                output.push('\n');
                pending_indent = Some(indent);
                width = indent;
            }

            Document::FlexBreak { broken, unbroken } => {
                let unbroken_width = width + unbroken.len() as isize;
                let unbroken_fits = mode == Mode::Unbroken || {
                    fits_buffer.clear();
                    fits_buffer.extend_from_slice(&docs);
                    fits(limit, unbroken_width, &mut fits_buffer)
                };
                if unbroken_fits {
                    write_pending_indent(output, &mut pending_indent);
                    output.push_str(unbroken);
                    width = unbroken_width;
                } else {
                    write_pending_indent(output, &mut pending_indent);
                    output.push_str(broken);
                    output.push('\n');
                    pending_indent = Some(indent);
                    width = indent;
                }
            }

            Document::StrictBreak { broken, unbroken } => match mode {
                Mode::Broken | Mode::ForcedBroken => {
                    write_pending_indent(output, &mut pending_indent);
                    output.push_str(broken);
                    output.push('\n');
                    pending_indent = Some(indent);
                    width = indent;
                }
                Mode::Unbroken | Mode::ForcedUnbroken => {
                    write_pending_indent(output, &mut pending_indent);
                    output.push_str(unbroken);
                    width += unbroken.len() as isize;
                }
            },

            Document::Text(s) => {
                write_pending_indent(output, &mut pending_indent);
                width += grapheme_count(s);
                output.push_str(s);
            }

            Document::VerbatimText(s) => {
                write_pending_indent(output, &mut pending_indent);
                let mut segments = s.split('\n');
                if let Some(first) = segments.next() {
                    output.push_str(first);
                    width += grapheme_count(first);
                }
                for segment in segments {
                    output.push('\n');
                    output.push_str(segment);
                    width = grapheme_count(segment);
                }
            }

            Document::Sequence(vec) => {
                for doc in vec.iter().rev() {
                    docs.push((indent, mode, doc));
                }
            }

            Document::Nest(i, doc) => {
                docs.push((indent + i, mode, doc));
            }

            Document::NestIfBroken(i, doc) => {
                if mode == Mode::Broken {
                    docs.push((indent + i, mode, doc));
                } else {
                    docs.push((indent, mode, doc));
                }
            }

            Document::Group(doc) => {
                fits_buffer.clear();
                fits_buffer.push((indent, Mode::Unbroken, doc.as_ref()));
                if fits(limit, width, &mut fits_buffer) {
                    docs.push((indent, Mode::Unbroken, doc));
                } else {
                    docs.push((indent, Mode::Broken, doc));
                }
            }

            Document::ForceBroken(document)
            | Document::NextBreakFits(document)
            | Document::NextBreakDoesNotFit(document)
            | Document::MeasureFlat(document) => {
                docs.push((indent, mode, document));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document<'a> {
    Newline,
    ForceBroken(Box<Self>),
    NextBreakFits(Box<Self>),
    NextBreakDoesNotFit(Box<Self>),
    MeasureFlat(Box<Self>),
    StrictBreak { broken: &'a str, unbroken: &'a str },
    FlexBreak { broken: &'a str, unbroken: &'a str },
    Sequence(Vec<Self>),
    Nest(isize, Box<Self>),
    NestIfBroken(isize, Box<Self>),
    Group(Box<Self>),
    Text(Cow<'a, str>),
    VerbatimText(Cow<'a, str>),
}

impl<'a> Document<'a> {
    pub fn str(string: &'a str) -> Self {
        Document::Text(Cow::Borrowed(string))
    }

    pub fn string(string: String) -> Self {
        Document::Text(Cow::Owned(string))
    }

    pub fn verbatim(string: String) -> Self {
        Document::VerbatimText(Cow::Owned(string))
    }

    pub fn group(self) -> Self {
        Self::Group(Box::new(self))
    }

    pub fn nest(self, indent: isize) -> Self {
        Self::Nest(indent, Box::new(self))
    }

    pub fn nest_if_broken(self, indent: isize) -> Self {
        Self::NestIfBroken(indent, Box::new(self))
    }

    pub fn force_break(self) -> Self {
        Self::ForceBroken(Box::new(self))
    }

    pub fn next_break_fits(self) -> Self {
        Self::NextBreakFits(Box::new(self))
    }

    pub fn next_break_does_not_fit(self) -> Self {
        Self::NextBreakDoesNotFit(Box::new(self))
    }

    pub fn measure_flat(self) -> Self {
        Self::MeasureFlat(Box::new(self))
    }

    pub fn append(self, second: impl Documentable<'a>) -> Self {
        match self {
            Self::Sequence(mut vec) => {
                vec.push(second.to_doc());
                Self::Sequence(vec)
            }
            first => Self::Sequence(vec![first, second.to_doc()]),
        }
    }

    pub fn to_pretty_string(&self, limit: isize) -> String {
        let mut buffer = String::new();
        self.pretty_print(limit, &mut buffer);
        buffer
    }

    pub fn pretty_print(&self, limit: isize, writer: &mut String) {
        format(writer, limit, 0, vec![(0, Mode::Unbroken, self)]);
    }
}

pub trait Documentable<'a> {
    fn to_doc(self) -> Document<'a>;
}

impl<'a> Documentable<'a> for &'a str {
    fn to_doc(self) -> Document<'a> {
        Document::str(self)
    }
}

impl<'a> Documentable<'a> for Document<'a> {
    fn to_doc(self) -> Document<'a> {
        self
    }
}

pub fn concat<'a>(docs: impl IntoIterator<Item = Document<'a>>) -> Document<'a> {
    Document::Sequence(docs.into_iter().collect())
}

pub fn join<'a>(
    docs: impl IntoIterator<Item = Document<'a>>,
    separator: Document<'a>,
) -> Document<'a> {
    let mut result = Vec::new();
    let mut first = true;
    for doc in docs {
        if first {
            first = false;
        } else {
            result.push(separator.clone());
        }
        result.push(doc);
    }
    Document::Sequence(result)
}

pub fn strict_break<'a>(broken: &'a str, unbroken: &'a str) -> Document<'a> {
    Document::StrictBreak { broken, unbroken }
}

pub fn flex_break<'a>(broken: &'a str, unbroken: &'a str) -> Document<'a> {
    Document::FlexBreak { broken, unbroken }
}
