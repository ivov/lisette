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
