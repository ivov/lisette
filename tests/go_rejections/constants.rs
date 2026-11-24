//! Constant expressions: literals, typed and untyped constants, binary operators and shifts.

use crate::{Case, Family, Intended};

/// The checker types the untyped left operand of a shift from its count, which Go does not.
const SHIFTS: &[&str] = &["shift_"];

pub const FAMILY: Family = Family {
    name: "constants",
    intended: &[
        Intended {
            code: "infer.type_mismatch",
            except: SHIFTS,
        },
        Intended {
            code: "infer.invalid_conversion",
            except: SHIFTS,
        },
        Intended {
            code: "infer.oversized_shift",
            except: &[],
        },
        Intended {
            code: "infer.division_by_zero",
            except: &[],
        },
        Intended {
            code: "infer.float_literal_int_conversion",
            except: &[],
        },
        Intended {
            code: "infer.cannot_negate_unsigned",
            except: &[],
        },
    ],
};

const TYPES: &[&str] = &[
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "float32",
    "float64",
    "complex64",
    "complex128",
    "byte",
    "rune",
    "string",
    "N8",
    "NF",
    "AU",
];
const NEWTYPES: &[&str] = &["N8", "NF"];
const LITERALS: &[&str] = &[
    "0",
    "1",
    "127",
    "128",
    "255",
    "256",
    "-1",
    "-128",
    "-129",
    "32767",
    "32768",
    "65536",
    "2147483647",
    "2147483648",
    "4294967296",
    "9223372036854775807",
    "9223372036854775808",
    "18446744073709551615",
    "18446744073709551616",
    "0xFF",
    "0x8000_0000",
    "0o777",
    "0b1111_1111",
    "2.0",
    "2.5",
    "-0.5",
    "1e10",
    "1e39",
    "1e300",
    "'a'",
    r"'\u{10FFFF}'",
    "\"s\"",
    "1i",
    "0i",
    "true",
];
const BINARY_TYPES: &[&str] = &[
    "int",
    "int8",
    "uint8",
    "uint64",
    "float32",
    "float64",
    "complex128",
    "string",
    "rune",
    "N8",
    "NF",
    "AU",
];
const LEFT_LITERALS: &[&str] = &["1", "127", "-1", "2.5", "'a'", "\"s\"", "1i"];
const RIGHT_LITERALS: &[&str] = &["0", "1", "8", "64", "-1", "0.5", "'a'", "\"t\"", "0i"];
const OPERATORS: &[&str] = &["+", "-", "*", "/", "%", "&", "|", "^", "&^", "<<", ">>"];
const SHIFTED_LITERALS: &[&str] = &["1", "1.0", "2.5", "'a'", "255"];
const SHIFT_COUNTS: &[(&str, &str)] = &[("uint", "3"), ("int", "3"), ("int", "70")];

const LISETTE_HEAD: &str =
    "import \"go:fmt\"\n\nstruct N8(int8)\nstruct NF(float32)\ntype AU = uint8\n\n";
const GO_HEAD: &str =
    "package main\n\nimport \"fmt\"\n\ntype N8 int8\ntype NF float32\ntype AU = uint8\n\n";

fn go_literal(literal: &str) -> String {
    literal.replace(r"'\u{10FFFF}'", r"'\U0010FFFF'")
}

/// The initializer of a variable of type `ty` from `literal`.
fn initializer(ty: &str, literal: &str) -> String {
    if NEWTYPES.contains(&ty) {
        format!("{ty}({literal})")
    } else {
        literal.to_string()
    }
}

struct Builder {
    cases: Vec<Case>,
}

impl Builder {
    fn add(&mut self, id: String, lisette: (&str, &str), go: (&str, &str)) {
        let (lisette_declarations, lisette_body) = lisette;
        let (go_declarations, go_body) = go;
        let main = format!(
            "{LISETTE_HEAD}{lisette_declarations}fn main() {{\n{lisette_body}  fmt.Println(v)\n}}\n"
        );
        let reference =
            format!("{GO_HEAD}{go_declarations}func main() {{\n{go_body}\tfmt.Println(v)\n}}\n");
        self.cases.push(Case {
            id,
            files: vec![("main.lis".to_string(), main)],
            reference: Some(vec![("main.go".to_string(), reference)]),
        });
    }
}

pub fn cases() -> Vec<Case> {
    let mut builder = Builder { cases: Vec::new() };
    for ty in TYPES {
        for literal in LITERALS {
            let go = go_literal(literal);
            let id = |context: &str| format!("{context}/{ty}/{literal}");
            builder.add(
                id("let"),
                ("", &format!("  let v: {ty} = {literal}\n")),
                ("", &format!("\tvar v {ty} = {go}\n")),
            );
            if NEWTYPES.contains(ty) {
                builder.add(
                    id("constructor"),
                    ("", &format!("  let v = {ty}({literal})\n")),
                    ("", &format!("\tv := {ty}({go})\n")),
                );
            } else {
                builder.add(
                    id("cast"),
                    ("", &format!("  let v = ({literal}) as {ty}\n")),
                    ("", &format!("\tv := {ty}({go})\n")),
                );
                builder.add(
                    id("constant_cast"),
                    (
                        &format!("const C = {literal}\n\n"),
                        &format!("  let v = C as {ty}\n"),
                    ),
                    (&format!("const C = {go}\n\n"), &format!("\tv := {ty}(C)\n")),
                );
                builder.add(
                    id("variable_cast"),
                    ("", &format!("  let x = {literal}\n  let v = x as {ty}\n")),
                    ("", &format!("\tx := {go}\n\tv := {ty}(x)\n")),
                );
                builder.add(
                    id("typed_constant"),
                    (&format!("const C: {ty} = {literal}\n\n"), "  let v = C\n"),
                    (&format!("const C {ty} = {go}\n\n"), "\tv := C\n"),
                );
            }
            builder.add(
                id("constant_let"),
                (
                    &format!("const C = {literal}\n\n"),
                    &format!("  let v: {ty} = C\n"),
                ),
                (
                    &format!("const C = {go}\n\n"),
                    &format!("\tvar v {ty} = C\n"),
                ),
            );
            builder.add(
                id("negated_let"),
                ("", &format!("  let v: {ty} = -({literal})\n")),
                ("", &format!("\tvar v {ty} = -({go})\n")),
            );
            builder.add(
                id("complement_let"),
                ("", &format!("  let v: {ty} = ^({literal})\n")),
                ("", &format!("\tvar v {ty} = ^({go})\n")),
            );
        }
    }
    for (type_index, ty) in BINARY_TYPES.iter().enumerate() {
        for (operator_index, operator) in OPERATORS.iter().enumerate() {
            for (right_index, right) in RIGHT_LITERALS.iter().enumerate() {
                let left = LEFT_LITERALS
                    [(type_index + operator_index + right_index) % LEFT_LITERALS.len()];
                let (go_left, go_right) = (go_literal(left), go_literal(right));
                let id = |context: &str| format!("{context}/{ty}/{left}/{operator}/{right}");
                builder.add(
                    id("binary_untyped"),
                    ("", &format!("  let v: {ty} = {left} {operator} {right}\n")),
                    (
                        "",
                        &format!("\tvar v {ty} = {go_left} {operator} {go_right}\n"),
                    ),
                );
                builder.add(
                    id("binary_variable"),
                    (
                        "",
                        &format!(
                            "  let a: {ty} = {}\n  let v = a {operator} {right}\n",
                            initializer(ty, left)
                        ),
                    ),
                    (
                        "",
                        &format!(
                            "\tvar a {ty} = {}\n\tv := a {operator} {go_right}\n",
                            initializer(ty, &go_left)
                        ),
                    ),
                );
                if !NEWTYPES.contains(ty) {
                    builder.add(
                        id("binary_typed_constant"),
                        (
                            &format!("const K: {ty} = {left}\n\n"),
                            &format!("  let v = K {operator} {right}\n"),
                        ),
                        (
                            &format!("const K {ty} = {go_left}\n\n"),
                            &format!("\tv := K {operator} {go_right}\n"),
                        ),
                    );
                }
            }
        }
    }
    for ty in BINARY_TYPES {
        for left in SHIFTED_LITERALS {
            for (count_type, count) in SHIFT_COUNTS {
                let go_left = go_literal(left);
                let id = |context: &str| format!("{context}/{ty}/{left}/{count_type}={count}");
                let count_line = format!("  let n: {count_type} = {count}\n");
                let go_count_line = format!("\tvar n {count_type} = {count}\n");
                builder.add(
                    id("shift_typed"),
                    ("", &format!("{count_line}  let v: {ty} = {left} << n\n")),
                    (
                        "",
                        &format!("{go_count_line}\tvar v {ty} = {go_left} << n\n"),
                    ),
                );
                builder.add(
                    id("shift_untyped"),
                    ("", &format!("{count_line}  let v = {left} << n\n")),
                    ("", &format!("{go_count_line}\tv := {go_left} << n\n")),
                );
                builder.add(
                    id("shift_cast"),
                    (
                        "",
                        &format!("{count_line}  let v = ({left} << n) as {ty}\n"),
                    ),
                    ("", &format!("{go_count_line}\tv := {ty}({go_left} << n)\n")),
                );
            }
        }
    }
    builder.cases
}
