use crate::{assert_emit_snapshot, assert_emit_snapshot_with_go_typedefs};

#[test]
fn import_single() {
    let input = r#"
import "go:io"
import "go:fmt"

fn test() {
  fmt.Print("Using imports")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_multiple() {
    let input = r#"
import "go:io"
import "go:os"
import "go:fs"
import "go:fmt"

fn test() {
  fmt.Print("Multiple imports")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_nested_path() {
    let input = r#"
import "internal/api"
import "internal/handlers"
import "go:fmt"

fn test() {
  fmt.Print("Nested path imports")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_deep_nested() {
    let input = r#"
import "internal/services/auth"
import "go:fmt"

fn test() {
  fmt.Print("Deep nested import")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_with_usage() {
    let input = r#"
import "go:io"
import "go:fmt"

fn test() {
  let x = "hello";
  fmt.Print(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_order_preserved() {
    let input = r#"
import "services/billing"
import "services/auth"
import "services/notifications"
import "go:fmt"

fn test() {
  fmt.Print("Import order test")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_named_alias() {
    let input = r#"
import router "go:github.com/gorilla/mux"
import "go:fmt"

fn test() {
  fmt.Print("Named alias import")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_blank() {
    let input = r#"
import _ "go:os"
import "go:fmt"

fn test() {
  let _ = fmt.Print("Blank import");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_mixed_aliases() {
    let input = r#"
import mystrings "go:strings"
import _ "go:os"
import "go:fmt"

fn test() {
  let _ = fmt.Print("Mixed alias imports");
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn import_local_with_alias() {
    let input = r#"
import h "utils/helpers"
import "go:fmt"

fn test() {
  fmt.Print("Local module with alias")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn aliased_go_import_preserved_for_unused_type() {
    let input = r#"
import s "go:sync"

struct Wrapper {
  mu: s.Mutex,
}

fn main() {}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn go_opaque_type_struct_literal() {
    let input = r#"
import "go:sync"

fn test() {
  let mut wg = sync.WaitGroup{}
  wg.Add(1)
  wg.Wait()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn third_party_go_import_path_emitted_in_full() {
    let input = r#"
import "go:github.com/bwmarrin/discordgo"

fn test() {
  let s = discordgo.Session{}
  let _ = s
}
"#;
    let typedef = r#"
pub struct Session {}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:github.com/bwmarrin/discordgo", typedef)]);
}

#[test]
fn third_party_go_type_uses_short_package_qualifier() {
    let input = r#"
import "go:github.com/bwmarrin/discordgo"

fn make() -> Ref<discordgo.Session> {
  &discordgo.Session{}
}
"#;
    let typedef = r#"
pub struct Session {}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:github.com/bwmarrin/discordgo", typedef)]);
}

#[test]
fn versioned_go_module_uses_package_directive_as_alias() {
    let input = r#"
import "go:example.com/bubbletea/v2"

fn make() -> Ref<tea.Program> {
  &tea.Program{}
}
"#;
    let typedef = r#"// Package: tea

pub struct Program {}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/bubbletea/v2", typedef)]);
}

#[test]
fn two_versioned_modules_with_distinct_package_names_coexist() {
    let input = r#"
import "go:example.com/bubbletea/v2"
import "go:example.com/lipgloss/v2"

fn make() -> Ref<tea.Program> {
  let _ = lipgloss.Style{}
  &tea.Program{}
}
"#;
    let tea_typedef = r#"// Package: tea

pub struct Program {}
"#;
    let lipgloss_typedef = r#"// Package: lipgloss

pub struct Style {}
"#;
    assert_emit_snapshot_with_go_typedefs!(
        input,
        &[
            ("go:example.com/bubbletea/v2", tea_typedef),
            ("go:example.com/lipgloss/v2", lipgloss_typedef),
        ]
    );
}

#[test]
fn go_type_uses_declared_package_name_not_path_segment() {
    let input = r#"
import "go:example.com/ultraviolet"

fn handle(_msg: uv.Event) {
}
"#;
    let typedef = r#"// Package: uv

pub struct Event {}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/ultraviolet", typedef)]);
}

#[test]
fn transitive_go_import_uses_declared_package_name() {
    let input = r#"
import "go:example.com/bubbletea"

struct Model {}

impl Model {
  fn Init(self: Model) -> tea.Cmd {
    || ()
  }
}
"#;
    let tea_typedef = r#"// Package: tea

import "go:example.com/ultraviolet"

pub type Cmd = fn() -> uv.Event
"#;
    let uv_typedef = r#"// Package: uv

pub interface Event {}
"#;
    assert_emit_snapshot_with_go_typedefs!(
        input,
        &[
            ("go:example.com/ultraviolet", uv_typedef),
            ("go:example.com/bubbletea", tea_typedef),
        ]
    );
}

#[test]
fn package_local_option_alias_does_not_collide_with_prelude_option() {
    // Regression: a Go module that declares its own `type Option = ...`
    // (e.g. the functional-options pattern) would trip `Type::is_option`
    // because it compared unqualified tails, causing an ICE in the emit
    // phase when the package-local Option was treated as prelude.Option.
    let input = r#"
import "go:example.com/validator"

fn test() {
  let _ = validator.WithOption()
}
"#;
    let typedef = r#"
pub struct Validate {}

pub type Option = fn(Ref<Validate>) -> ()

pub fn WithOption() -> Option
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/validator", typedef)]);
}
