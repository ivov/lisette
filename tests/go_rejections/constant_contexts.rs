//! Expressions that Go folds to a constant in a checked position: divisors, indexes, slice bounds, `min`, `max`, shifts.

use crate::{Case, Family, Intended};

pub const FAMILY: Family = Family {
    name: "constant_contexts",
    intended: &[
        Intended {
            code: "infer.type_mismatch",
            except: &[],
        },
        Intended {
            code: "infer.division_by_zero",
            except: &[],
        },
        Intended {
            code: "infer.range_index_not_slice",
            except: &[],
        },
        Intended {
            code: "infer.string_not_indexable",
            except: &[],
        },
    ],
};

const DECLARATIONS: &str =
    "const K0 = 0\nconst K3 = 3\nconst Z = \"\"\nconst S3 = \"abc\"\nconst F0 = 0.0\n\n";

/// Integer zero forms as (Lisette, Go). Go folds most of them to a constant.
const ZEROS: &[(&str, &str)] = &[
    ("0", "0"),
    ("1 - 1", "1 - 1"),
    ("K0", "K0"),
    ("\"\".length()", "len(\"\")"),
    ("Z.length()", "len(Z)"),
    ("a.length() - 3", "len(a) - 3"),
    ("(0 as int)", "int(0)"),
    ("K3 - 3", "K3 - 3"),
    ("2 * 0", "2 * 0"),
    ("S3.length() - 3", "len(S3) - 3"),
    ("min(0, 1)", "min(0, 1)"),
    ("max(-1, 0)", "max(-1, 0)"),
];
const FLOAT_ZEROS: &[(&str, &str)] = &[
    ("0.0", "0.0"),
    ("F0", "F0"),
    ("0.0 * 2.0", "0.0 * 2.0"),
    ("0i", "0i"),
    ("(0 as float64)", "float64(0)"),
];
const INDEXES: &[(&str, &str)] = &[
    ("3", "3"),
    ("1 + 2", "1 + 2"),
    ("K3", "K3"),
    ("S3.length()", "len(S3)"),
    ("a.length()", "len(a)"),
    ("-1", "-1"),
    ("0 - 1", "0 - 1"),
    ("K0 - 1", "K0 - 1"),
    ("max(3, 1)", "max(3, 1)"),
    ("2", "2"),
    ("(3 as int)", "int(3)"),
    ("(1 << 2)", "(1 << 2)"),
];
const MIN_MAX: &[(&str, &str, &str)] = &[
    ("min", "1, 2.5", "1, 2.5"),
    ("min", "x, 2.5", "x, 2.5"),
    ("max", "x, 2", "x, 2"),
    ("min", "f, 2", "f, 2"),
    ("max", "\"a\", \"b\"", "\"a\", \"b\""),
    ("min", "x, f", "x, f"),
    ("max", "1 << 70, 1", "1 << 70, 1"),
    ("min", "S3, \"b\"", "S3, \"b\""),
    ("max", "true, false", "true, false"),
    ("min", "c, c", "c, c"),
    ("min", "a.length(), 9", "len(a), 9"),
    ("max", "'a', 1", "'a', 1"),
    ("min", "x as uint8, 300", "uint8(x), 300"),
    ("max", "x as int8, -129", "int8(x), -129"),
];
const OTHERS: &[(&str, &str)] = &[
    ("real(c) / 0.0", "real(c) / 0.0"),
    ("complex(1.0, 2.0) / 0i", "complex(1.0, 2.0) / 0i"),
    ("imaginary(1i)", "imag(1i)"),
    ("(1 << 3) as float64", "float64(1 << 3)"),
    ("(1.0 + 2.0i) / (0i)", "(1.0 + 2.0i) / (0i)"),
    ("x << 64", "x << 64"),
    ("x >> -1", "x >> -1"),
    ("x << K0 - 1", "x << (K0 - 1)"),
    ("1 << 64", "1 << 64"),
    ("(1 as uint64) << 64", "uint64(1) << 64"),
    ("a.length() << 62", "len(a) << 62"),
    (
        "S3.length() * 9223372036854775807",
        "len(S3) * 9223372036854775807",
    ),
];

struct Builder {
    cases: Vec<Case>,
}

impl Builder {
    fn add(&mut self, context: &str, expression: &str, lisette_body: &str, go_body: &str) {
        let id = format!("{context}/{}", expression.replace(' ', ""));
        let main = format!(
            "import \"go:fmt\"\n\n{DECLARATIONS}fn main() {{\n  let a = Array.new<int, 3>()\n  let s = [1, 2, 3]\n  let x: int = 7\n  let f: float64 = 7.0\n  let c: complex128 = 1i\n{lisette_body}  fmt.Println(v, a, s, x, f, c)\n}}\n"
        );
        let reference = format!(
            "package main\n\nimport \"fmt\"\n\n{DECLARATIONS}func main() {{\n\ta := [3]int{{}}\n\ts := []int{{1, 2, 3}}\n\tvar x int = 7\n\tvar f float64 = 7.0\n\tvar c complex128 = 1i\n{go_body}\tfmt.Println(v, a, s, x, f, c)\n}}\n"
        );
        self.cases.push(Case {
            id,
            files: vec![("main.lis".to_string(), main)],
            reference: Some(vec![("main.go".to_string(), reference)]),
        });
    }
}

pub fn cases() -> Vec<Case> {
    let mut builder = Builder { cases: Vec::new() };
    for (lisette, go) in ZEROS {
        for operator in ["/", "%"] {
            let name = if operator == "/" {
                "division"
            } else {
                "remainder"
            };
            builder.add(
                &format!("integer_{name}"),
                lisette,
                &format!("  let v = x {operator} ({lisette})\n"),
                &format!("\tv := x {operator} ({go})\n"),
            );
            builder.add(
                &format!("integer_{name}_assign"),
                lisette,
                &format!("  let mut v = x\n  v {operator}= {lisette}\n"),
                &format!("\tv := x\n\tv {operator}= {go}\n"),
            );
            builder.add(
                &format!("constant_{name}"),
                lisette,
                &format!("  let v = 7 {operator} ({lisette})\n"),
                &format!("\tv := 7 {operator} ({go})\n"),
            );
        }
    }
    for (lisette, go) in FLOAT_ZEROS {
        builder.add(
            "float_division",
            lisette,
            &format!("  let v = f / ({lisette})\n"),
            &format!("\tv := f / ({go})\n"),
        );
        builder.add(
            "complex_division",
            lisette,
            &format!("  let v = c / ({lisette})\n"),
            &format!("\tv := c / ({go})\n"),
        );
        builder.add(
            "constant_float_division",
            lisette,
            &format!("  let v = 7.0 / ({lisette})\n"),
            &format!("\tv := 7.0 / ({go})\n"),
        );
    }
    for (lisette, go) in INDEXES {
        builder.add(
            "array_index",
            lisette,
            &format!("  let v = a[{lisette}]\n"),
            &format!("\tv := a[{go}]\n"),
        );
        builder.add(
            "slice_index",
            lisette,
            &format!("  let v = s[{lisette}]\n"),
            &format!("\tv := s[{go}]\n"),
        );
        builder.add(
            "constant_string_index",
            lisette,
            &format!("  let v = S3[{lisette}]\n"),
            &format!("\tv := S3[{go}]\n"),
        );
        builder.add(
            "string_literal_index",
            lisette,
            &format!("  let v = \"abc\"[{lisette}]\n"),
            &format!("\tv := \"abc\"[{go}]\n"),
        );
        builder.add(
            "array_slice_high",
            lisette,
            &format!("  let v = a[0..{lisette} + 1]\n"),
            &format!("\tv := a[0 : {go} + 1]\n"),
        );
        builder.add(
            "slice_low_above_high",
            lisette,
            &format!("  let v = s[{lisette}..1]\n"),
            &format!("\tv := s[{go}:1]\n"),
        );
    }
    for (function, lisette_arguments, go_arguments) in MIN_MAX {
        builder.add(
            "min_max",
            &format!("{function}({lisette_arguments})"),
            &format!("  let v = {function}({lisette_arguments})\n"),
            &format!("\tv := {function}({go_arguments})\n"),
        );
    }
    for (lisette, go) in OTHERS {
        builder.add(
            "other",
            lisette,
            &format!("  let v = {lisette}\n"),
            &format!("\tv := {go}\n"),
        );
    }
    builder.cases
}
