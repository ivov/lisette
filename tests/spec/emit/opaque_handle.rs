use crate::_harness::emit_with_go_typedefs;

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

#[test]
#[should_panic(expected = "opaque Go handle")]
fn handle_in_collection_literal_trips_emit_guard() {
    let input = r#"
import "go:example.com/inv"

fn run() {
  let _xs = [inv.SmallChest, inv.LargeChest]
}
"#;
    let _ = emit_with_go_typedefs(input, &[("go:example.com/inv", TYPEDEF)]).go_code();
}
