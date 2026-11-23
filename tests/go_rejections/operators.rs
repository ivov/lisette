//! Each operator on each kind of operand type, against the emitted Go of a skeleton that only prints the operands.

use std::collections::HashMap;

use crate::{Case, Compiled, Family, GoFiles, Intended, compile_case};

pub const FAMILY: Family = Family {
    name: "operators",
    intended: &[
        Intended {
            code: "infer.type_mismatch",
            except: &["uintptr/", "T:Ordered/", "Ref<T:Ordered>/"],
        },
        Intended {
            code: "infer.cannot_negate_unsigned",
            except: &[],
        },
    ],
};

const DECLARATIONS: &str = "struct N(int)
struct NS(string)
struct NB(bool)
struct NF(float64)
struct NC(complex128)
type AI = int
type AS = string
type AR = Ref<int>
interface I {
  fn m() -> int
}
struct P { x: int }
struct Q { xs: Slice<int> }
enum E { A, B }
";

/// (Lisette type, generic parameters, label).
const TYPES: &[(&str, &str, &str)] = &[
    ("int", "", "int"),
    ("int8", "", "int8"),
    ("uint8", "", "uint8"),
    ("uint64", "", "uint64"),
    ("uintptr", "", "uintptr"),
    ("float32", "", "float32"),
    ("float64", "", "float64"),
    ("complex128", "", "complex128"),
    ("string", "", "string"),
    ("bool", "", "bool"),
    ("rune", "", "rune"),
    ("byte", "", "byte"),
    ("Ref<int>", "", "Ref<int>"),
    ("Ref<string>", "", "Ref<string>"),
    ("Ref<bool>", "", "Ref<bool>"),
    ("Ref<N>", "", "Ref<N>"),
    ("N", "", "N"),
    ("NS", "", "NS"),
    ("NB", "", "NB"),
    ("NF", "", "NF"),
    ("NC", "", "NC"),
    ("AI", "", "AI"),
    ("AS", "", "AS"),
    ("AR", "", "AR"),
    ("I", "", "I"),
    ("P", "", "P"),
    ("Q", "", "Q"),
    ("Slice<int>", "", "Slice<int>"),
    ("Map<string, int>", "", "Map<string,int>"),
    ("Array<int, 2>", "", "Array<int,2>"),
    ("Option<int>", "", "Option<int>"),
    ("(int, int)", "", "(int,int)"),
    ("fn() -> int", "", "fn()->int"),
    ("E", "", "E"),
    ("Ref<Ref<int>>", "", "Ref<Ref<int>>"),
    ("Ref<AI>", "", "Ref<AI>"),
    ("T", "<T>", "T"),
    ("T", "<T: Comparable>", "T:Comparable"),
    ("T", "<T: Ordered>", "T:Ordered"),
    ("Ref<T>", "<T: Ordered>", "Ref<T:Ordered>"),
];
const BINARY: &[&str] = &[
    "+", "-", "*", "/", "%", "&", "|", "^", "&^", "<<", ">>", "==", "!=", "<", "<=", ">", ">=",
    "&&", "||",
];
const COMPOUND: &[&str] = &[
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "&^=", "<<=", ">>=",
];
const UNARY: &[&str] = &["-", "!", "^"];
const LITERALS: &[&str] = &["1", "1.5", "\"x\"", "true"];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Parameters {
    TwoOfType,
    TypeAndUint,
    One,
}

impl Parameters {
    fn render(self, ty: &str) -> String {
        match self {
            Self::TwoOfType => format!("a: {ty}, b: {ty}"),
            Self::TypeAndUint => format!("a: {ty}, b: uint"),
            Self::One => format!("a: {ty}"),
        }
    }

    fn print_line(self) -> &'static str {
        match self {
            Self::TwoOfType | Self::TypeAndUint => "  fmt.Println(a, b)\n",
            Self::One => "  fmt.Println(a)\n",
        }
    }
}

fn program(generics: &str, parameters: &str, body: &str) -> String {
    format!(
        "import \"go:fmt\"\n\n{DECLARATIONS}\nfn f{generics}({parameters}) {{\n{body}}}\n\nfn main() {{\n  fmt.Println(1)\n}}\n"
    )
}

/// Puts `go_body` in place of the print line that opens `func f`.
fn splice(go: &str, go_body: &str) -> Option<String> {
    let function = go.find("\nfunc f")? + 1;
    let line_start = function + go[function..].find('\n')? + 1;
    let line_end = line_start + go[line_start..].find('\n')? + 1;
    let line = &go[line_start..line_end];
    if !line.starts_with('\t') || !line.contains("fmt.Println(") {
        return None;
    }
    Some(format!("{}{go_body}{}", &go[..line_start], &go[line_end..]))
}

struct Builder {
    skeletons: HashMap<(&'static str, Parameters), Option<String>>,
    cases: Vec<Case>,
}

impl Builder {
    fn add(
        &mut self,
        ty: &(&'static str, &'static str, &'static str),
        parameters: Parameters,
        id: String,
        lisette_body: String,
        go_body: String,
    ) {
        let (lisette_type, generics, label) = *ty;
        let skeleton = self
            .skeletons
            .entry((label, parameters))
            .or_insert_with(|| {
                let source = program(
                    generics,
                    &parameters.render(lisette_type),
                    parameters.print_line(),
                );
                match compile_case(&[("main.lis".to_string(), source)]) {
                    Compiled::Accepted(files) => files
                        .into_iter()
                        .find(|(name, _)| name.ends_with(".go"))
                        .map(|(_, go)| go),
                    Compiled::Rejected(_) | Compiled::Failed(_) => None,
                }
            });
        let reference: Option<GoFiles> = skeleton
            .as_deref()
            .and_then(|go| splice(go, &go_body))
            .map(|go| vec![("main.go".to_string(), go)]);
        self.cases.push(Case {
            id,
            files: vec![(
                "main.lis".to_string(),
                program(generics, &parameters.render(lisette_type), &lisette_body),
            )],
            reference,
        });
    }
}

pub fn cases() -> Vec<Case> {
    let mut builder = Builder {
        skeletons: HashMap::new(),
        cases: Vec::new(),
    };
    for ty in TYPES {
        let label = ty.2;
        for operator in BINARY {
            builder.add(
                ty,
                Parameters::TwoOfType,
                format!("{label}/{operator}/same"),
                format!("  let v = a {operator} b\n  fmt.Println(v)\n"),
                format!("\tv := a {operator} b\n\tfmt.Println(v)\n"),
            );
            for literal in LITERALS {
                builder.add(
                    ty,
                    Parameters::One,
                    format!("{label}/{operator}/right={literal}"),
                    format!("  let v = a {operator} {literal}\n  fmt.Println(v)\n"),
                    format!("\tv := a {operator} {literal}\n\tfmt.Println(v)\n"),
                );
                builder.add(
                    ty,
                    Parameters::One,
                    format!("{label}/{operator}/left={literal}"),
                    format!("  let v = {literal} {operator} a\n  fmt.Println(v)\n"),
                    format!("\tv := {literal} {operator} a\n\tfmt.Println(v)\n"),
                );
            }
            if *operator == "<<" || *operator == ">>" {
                builder.add(
                    ty,
                    Parameters::TypeAndUint,
                    format!("{label}/{operator}/uint"),
                    format!("  let v = a {operator} b\n  fmt.Println(v)\n"),
                    format!("\tv := a {operator} b\n\tfmt.Println(v)\n"),
                );
            }
        }
        for operator in COMPOUND {
            builder.add(
                ty,
                Parameters::TwoOfType,
                format!("{label}/{operator}/same"),
                format!("  let mut c = a\n  c {operator} b\n  fmt.Println(c)\n"),
                format!("\tc := a\n\tc {operator} b\n\tfmt.Println(c)\n"),
            );
        }
        for operator in UNARY {
            builder.add(
                ty,
                Parameters::One,
                format!("{label}/unary{operator}"),
                format!("  let v = {operator}a\n  fmt.Println(v)\n"),
                format!("\tv := {operator}a\n\tfmt.Println(v)\n"),
            );
        }
    }
    builder.cases
}
