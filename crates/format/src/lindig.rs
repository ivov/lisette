use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unbroken,
    Broken,
    ForcedBroken,
    ForcedUnbroken,
}

fn fits(
    limit: isize,
    mut current_width: isize,
    mut docs: Vec<(isize, Mode, &Document<'_>)>,
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

            Document::Text(s) => {
                current_width += s.graphemes(true).count() as isize;
            }

            Document::StrictBreak { unbroken, .. } | Document::FlexBreak { unbroken, .. } => {
                match mode {
                    Mode::Broken | Mode::ForcedBroken => return true,
                    Mode::Unbroken | Mode::ForcedUnbroken => {
                        current_width += unbroken.len() as isize;
                    }
                }
            }

            Document::NextBreakFits(doc, enabled) => {
                if *enabled {
                    match mode {
                        Mode::ForcedUnbroken => docs.push((indent, mode, doc)),
                        _ => docs.push((indent, Mode::ForcedBroken, doc)),
                    }
                } else {
                    docs.push((indent, Mode::ForcedUnbroken, doc));
                }
            }

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

fn format(
    output: &mut String,
    limit: isize,
    mut width: isize,
    mut docs: Vec<(isize, Mode, &Document<'_>)>,
) {
    let mut pending_indent: isize = -1;

    while let Some((indent, mode, document)) = docs.pop() {
        match document {
            Document::Newline => {
                output.push('\n');
                pending_indent = indent;
                width = indent;
            }

            Document::FlexBreak { broken, unbroken } => {
                let unbroken_width = width + unbroken.len() as isize;
                if mode == Mode::Unbroken || fits(limit, unbroken_width, docs.clone()) {
                    if pending_indent >= 0 {
                        write_indent(output, pending_indent);
                        pending_indent = -1;
                    }
                    output.push_str(unbroken);
                    width = unbroken_width;
                } else {
                    if pending_indent >= 0 {
                        write_indent(output, pending_indent);
                    }
                    output.push_str(broken);
                    output.push('\n');
                    pending_indent = indent;
                    width = indent;
                }
            }

            Document::StrictBreak { broken, unbroken } => match mode {
                Mode::Broken | Mode::ForcedBroken => {
                    if pending_indent >= 0 {
                        write_indent(output, pending_indent);
                    }
                    output.push_str(broken);
                    output.push('\n');
                    pending_indent = indent;
                    width = indent;
                }
                Mode::Unbroken | Mode::ForcedUnbroken => {
                    if pending_indent >= 0 {
                        write_indent(output, pending_indent);
                        pending_indent = -1;
                    }
                    output.push_str(unbroken);
                    width += unbroken.len() as isize;
                }
            },

            Document::Text(s) => {
                if pending_indent >= 0 {
                    write_indent(output, pending_indent);
                    pending_indent = -1;
                }
                width += s.graphemes(true).count() as isize;
                output.push_str(s);
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
                let group_docs = vec![(indent, Mode::Unbroken, doc.as_ref())];
                if fits(limit, width, group_docs) {
                    docs.push((indent, Mode::Unbroken, doc));
                } else {
                    docs.push((indent, Mode::Broken, doc));
                }
            }

            Document::ForceBroken(document) | Document::NextBreakFits(document, _) => {
                docs.push((indent, mode, document));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Document<'a> {
    Newline,
    ForceBroken(Box<Self>),
    NextBreakFits(Box<Self>, bool),
    StrictBreak { broken: &'a str, unbroken: &'a str },
    FlexBreak { broken: &'a str, unbroken: &'a str },
    Sequence(Vec<Self>),
    Nest(isize, Box<Self>),
    NestIfBroken(isize, Box<Self>),
    Group(Box<Self>),
    Text(Cow<'a, str>),
}

impl<'a> Document<'a> {
    pub fn str(string: &'a str) -> Self {
        Document::Text(Cow::Borrowed(string))
    }

    pub fn string(string: String) -> Self {
        Document::Text(Cow::Owned(string))
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

    pub fn next_break_fits(self, enabled: bool) -> Self {
        Self::NextBreakFits(Box::new(self), enabled)
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
