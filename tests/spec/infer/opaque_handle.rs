use crate::spec::infer::*;

const TYPEDEF: &str = r#"
#[go(unexported)]
pub type chest

pub var SmallChest: chest
pub var LargeChest: chest

pub fn NewMenu(name: string, c: chest) -> int
pub fn Use(c: chest)
"#;

#[test]
fn hold_then_pass() {
    let input = r#"
import "go:example.com/inv"

fn run() {
  let c = inv.SmallChest
  inv.Use(c)
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]).assert_no_errors();
}

#[test]
fn pass_directly() {
    let input = r#"
import "go:example.com/inv"

fn run() -> int {
  inv.NewMenu("duel", inv.SmallChest)
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]).assert_no_errors();
}

#[test]
fn cannot_be_named() {
    let input = r#"
import "go:example.com/inv"

fn run() {
  let _c = inv.chest {}
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)])
        .assert_resolve_code("not_found_in_module");
}

#[test]
fn cannot_be_compared() {
    let input = r#"
import "go:example.com/inv"

fn run() -> bool {
  let a = inv.SmallChest
  let b = inv.LargeChest
  a == b
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)])
        .assert_error_contains("compared");
}
