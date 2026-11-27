use std::fs;
use std::process::Command;

use rayon::prelude::*;

use crate::_harness::go::{
    compile_go_test, go_available, prelude_dir, read_go_toolchain, repo_root, write_go_mod,
};

const PRELUDE: &str = r#"
import "go:fmt"

interface Shape { fn area() -> int }
struct Sq { side: int }
struct Ci { r: int }
struct Pt(int, int)
struct Nt(int)
enum Kind { Big(int), Small, Wide { w: int } }
impl Sq { fn area(self) -> int { self.side } }
impl Ci { fn area(self) -> int { self.r } }
impl Pt { fn area(self) -> int { self.0 * self.1 } }
impl Nt { fn area(self) -> int { self.0 } }
impl Kind { fn area(self) -> int { 7 } }

fn inner(x: Shape) -> string {
  match x {
    Sq { side } => fmt.Sprint("sq", side),
    Ci { r } => fmt.Sprint("ci", r),
    _ => "shape",
  }
}
"#;

struct Case {
    name: &'static str,
    concrete: &'static str,
    widened: &'static str,
    body: &'static str,
    widened_extra_arm: &'static str,
    values: &'static [&'static str],
}

const KINDS: &[&str] = &[
    "Kind.Big(9)",
    "Kind.Big(2)",
    "Kind.Small",
    "Kind.Wide { w: 6 }",
];
const SQUARES: &[&str] = &["Sq { side: 1 }", "Sq { side: 2 }"];

const CASES: &[Case] = &[
    Case {
        name: "enum_variants",
        concrete: "Kind",
        widened: "Shape",
        body: "match v { Kind.Big(n) => fmt.Sprint(n), Kind.Small => \"s\", Kind.Wide { w } => fmt.Sprint(w)$EXTRA }",
        widened_extra_arm: ", _ => \"other\"",
        values: KINDS,
    },
    Case {
        name: "enum_guard_then_catchall",
        concrete: "Kind",
        widened: "Shape",
        body: "match v { Kind.Big(n) if n > 5 => \"big\", other => inner(other) }",
        widened_extra_arm: "",
        values: KINDS,
    },
    Case {
        name: "enum_as_binding",
        concrete: "Kind",
        widened: "Shape",
        body: "match v { Kind.Wide { .. } as k => inner(k), Kind.Big(n) => fmt.Sprint(n), _ => \"rest\" }",
        widened_extra_arm: "",
        values: KINDS,
    },
    Case {
        name: "struct_field_then_catchall",
        concrete: "Sq",
        widened: "Shape",
        body: "match v { Sq { side: 1 } => \"one\", other => inner(other) }",
        widened_extra_arm: "",
        values: SQUARES,
    },
    Case {
        name: "struct_guard_reads_subject",
        concrete: "Sq",
        widened: "Shape",
        body: "match v { Sq { .. } if v.area() == 1 => \"one\", other => inner(other) }",
        widened_extra_arm: "",
        values: SQUARES,
    },
    Case {
        name: "struct_as_binding",
        concrete: "Sq",
        widened: "Shape",
        body: "match v { Sq { side: 2 } as c => fmt.Sprint(\"two\", c.side), other => inner(other) }",
        widened_extra_arm: "",
        values: SQUARES,
    },
    Case {
        name: "tuple_struct_literal",
        concrete: "Pt",
        widened: "Shape",
        body: "match v { Pt(1, b) => fmt.Sprint(b), other => inner(other) }",
        widened_extra_arm: "",
        values: &["Pt(1, 5)", "Pt(2, 5)"],
    },
    Case {
        name: "newtype_literal",
        concrete: "Nt",
        widened: "Shape",
        body: "match v { Nt(4) => \"four\", Nt(n) => fmt.Sprint(n)$EXTRA }",
        widened_extra_arm: ", _ => \"other\"",
        values: &["Nt(4)", "Nt(5)"],
    },
    Case {
        name: "option_payload",
        concrete: "Option<Kind>",
        widened: "Option<Shape>",
        body: "match v { Some(Kind.Big(n)) => fmt.Sprint(n), Some(other) => inner(other), None => \"none\" }",
        widened_extra_arm: "",
        values: &["Some(Kind.Big(9))", "Some(Kind.Small)", "None"],
    },
    Case {
        name: "result_payload_as",
        concrete: "Result<Kind, string>",
        widened: "Result<Shape, string>",
        body: "match v { Ok(Kind.Wide { .. } as k) => inner(k), Ok(other) => inner(other), Err(e) => e }",
        widened_extra_arm: "",
        values: &["Ok(Kind.Wide { w: 6 })", "Ok(Kind.Small)", "Err(\"e\")"],
    },
    Case {
        name: "tuple_element",
        concrete: "(Kind, int)",
        widened: "(Shape, int)",
        body: "match v { (Kind.Small, 1) => \"s1\", (k, _) => inner(k) }",
        widened_extra_arm: "",
        values: &["(Kind.Small, 1)", "(Kind.Small, 2)", "(Kind.Big(3), 1)"],
    },
    Case {
        name: "slice_element_as",
        concrete: "Slice<Sq>",
        widened: "Slice<Shape>",
        body: "match v { [Sq { side: 1 } as c] => fmt.Sprint(c.side), [first, ..] => inner(first), [] => \"empty\" }",
        widened_extra_arm: "",
        values: &["[Sq { side: 1 }]", "[Sq { side: 2 }]", "[]"],
    },
    Case {
        name: "if_let_variant",
        concrete: "Kind",
        widened: "Shape",
        body: "if let Kind.Big(n) = v { fmt.Sprint(n) } else { \"no\" }",
        widened_extra_arm: "",
        values: KINDS,
    },
    Case {
        name: "let_else_variant",
        concrete: "Kind",
        widened: "Shape",
        body: "{ let Kind.Wide { w } = v else { return \"no\" }; fmt.Sprint(w) }",
        widened_extra_arm: "",
        values: KINDS,
    },
];

fn program(case: &Case, widened: bool) -> String {
    let (subject_ty, extra) = if widened {
        (case.widened, case.widened_extra_arm)
    } else {
        (case.concrete, "")
    };
    let body = case.body.replace("$EXTRA", extra);
    let calls = case
        .values
        .iter()
        .map(|value| format!("  fmt.Println(f({value}))\n"))
        .collect::<String>();
    format!(
        "{PRELUDE}\nfn f(v: {subject_ty}) -> string {{\n  {body}\n}}\n\nfn main() {{\n  let _ = inner(Ci {{ r: 0 }})\n{calls}}}\n"
    )
}

fn run(name: &str, source: &str) -> Result<String, String> {
    let dir = repo_root().join("target/go_widened").join(name);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::write(dir.join("main.lis"), source).map_err(|error| error.to_string())?;
    let emitted = compile_go_test(source, "main").map_err(|error| error.to_string())?;
    fs::write(dir.join("main.go"), emitted.go_code).map_err(|error| error.to_string())?;
    let output = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[test]
fn widened_match_subject_gives_the_same_values() {
    if !go_available() {
        eprintln!("skipping widened_match_subject_gives_the_same_values: `go` not found");
        return;
    }
    let root = repo_root().join("target/go_widened");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create target/go_widened");
    let go_toolchain = read_go_toolchain().expect("read go-toolchain");
    write_go_mod(&root, &prelude_dir(), &go_toolchain).expect("write go.mod");

    let failures: Vec<String> = CASES
        .par_iter()
        .filter_map(|case| {
            let concrete = run(&format!("{}_concrete", case.name), &program(case, false));
            let widened = run(&format!("{}_widened", case.name), &program(case, true));
            match (concrete, widened) {
                (Ok(concrete), Ok(widened)) if concrete == widened => None,
                (concrete, widened) => Some(format!(
                    "{}:\n  concrete: {concrete:?}\n  widened:  {widened:?}",
                    case.name
                )),
            }
        })
        .collect();

    assert!(
        failures.is_empty(),
        "widening the subject changed the output:\n{}",
        failures.join("\n")
    );
}
