use crate::_harness::emit::emit_diagnostics_with_go_typedefs;
use crate::_harness::emit_with_go_typedefs;
use crate::_harness::wrap::wrap;

const TYPEDEF: &str = r#"
#[go(unexported)]
pub type chest

pub var SmallChest: chest
pub var LargeChest: chest

pub fn NewMenu(name: string, c: chest) -> int
pub fn Use(c: chest)
"#;

#[test]
fn handle_flows_by_inference_without_spelling_the_type() {
    let input = r#"
import "go:example.com/inv"

fn run() -> int {
  let c = inv.SmallChest
  inv.Use(c)
  inv.NewMenu("duel", c)
}
"#;
    let go = emit_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]).go_code();

    assert!(
        go.contains("inv.SmallChest"),
        "expected the producer var reference, got:\n{go}"
    );
    assert!(
        go.contains("inv.NewMenu("),
        "expected the consumer call, got:\n{go}"
    );
    assert!(
        go.contains("inv.Use("),
        "expected the consumer call, got:\n{go}"
    );
    assert!(
        !go.contains("inv.chest"),
        "the unexported handle type must never be spelled, got:\n{go}"
    );
}

#[test]
fn handle_passed_directly_without_a_binding() {
    let input = r#"
import "go:example.com/inv"

fn run() -> int {
  inv.NewMenu("duel", inv.SmallChest)
}
"#;
    let go = emit_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]).go_code();

    assert!(go.contains("inv.NewMenu(") && go.contains("inv.SmallChest"));
    assert!(!go.contains("inv.chest"), "got:\n{go}");
}

fn rejection_codes(input: &str) -> Vec<String> {
    emit_diagnostics_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)])
        .iter()
        .filter_map(|diagnostic| diagnostic.code_str().map(str::to_string))
        .collect()
}

#[test]
fn handle_in_collection_literal_is_rejected() {
    let input = r#"
import "go:example.com/inv"

fn run() {
  let _xs = [inv.SmallChest, inv.LargeChest]
}
"#;
    assert_eq!(rejection_codes(input), ["emit.unnamable_go_type"]);
}

#[test]
fn handle_in_spelled_positions_gives_one_error_per_statement() {
    let input = r#"
import "go:example.com/inv"

fn run(flag: bool) {
  let c = if flag { inv.SmallChest } else { inv.LargeChest }
  inv.Use(c)
  let _t = (inv.SmallChest, 1)
  let d = loop { break { inv.LargeChest } }
  inv.Use(d)
}
"#;
    assert_eq!(
        rejection_codes(input),
        [
            "emit.unnamable_go_type",
            "emit.unnamable_go_type",
            "emit.unnamable_go_type"
        ]
    );
}

#[test]
fn handle_into_unknown_is_not_rejected() {
    let input = r#"
import "go:example.com/inv"

fn run() -> Unknown {
  let value: Unknown = { inv.SmallChest }
  value
}
"#;
    assert_eq!(rejection_codes(input), Vec::<String>::new());
}

#[test]
fn rejection_points_at_the_statement_after_a_nested_one() {
    let input = r#"
import "go:example.com/inv"

fn run() {
  let _t = ({ let z = 1; z }, inv.SmallChest)
}
"#;
    let diagnostics = emit_diagnostics_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]);
    assert_eq!(diagnostics.len(), 1);
    let source = wrap(input);
    let at = &source[diagnostics[0].primary_offset()..];
    assert!(at.starts_with("let _t"), "the error points at: {at}");
}
