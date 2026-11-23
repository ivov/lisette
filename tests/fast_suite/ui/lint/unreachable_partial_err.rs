use crate::_harness::build::{compile_check, compile_script_entry};
use crate::_harness::filesystem::MockFileSystem;
use crate::_harness::infer::checker_errors;
use crate::_harness::lint::lint_with_go_typedefs;
use crate::assert_lint_snapshot;
use diagnostics::LisetteDiagnostic;
use semantics::store::ENTRY_PACKAGE_ID;

const CODE: &str = "lint.unreachable_partial_err";

fn count(diagnostics: &[LisetteDiagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code_str() == Some(CODE))
        .count()
}

fn check(source: &str) -> passes::Analysis {
    let mut fs = MockFileSystem::new();
    fs.add_file(ENTRY_PACKAGE_ID, "main.lis", source);
    compile_check(fs)
}

#[test]
fn unreachable_partial_err_diagnostic() {
    assert_lint_snapshot!(
        r#"
import "go:os"
fn main() {
  if let Partial.Err(err) = os.Stdout.Write([1]) {
    panic(err.Error())
  }
}
"#
    );
}

#[test]
fn unreachable_partial_err_direct_shapes() {
    for (declarations, parameter, expression) in [
        ("", "io.Writer", "w.Write([1])"),
        ("", "io.Writer", "(w.Write([1]))"),
        ("type Writer = io.Writer", "Writer", "w.Write([1])"),
        (
            "struct Handle(io.Writer)",
            "io.Writer",
            "Handle(w).0.Write([1])",
        ),
    ] {
        let source = format!(
            r#"
import io "go:io"
{declarations}
fn use_writer<T>(w: {parameter}, _tag: T) {{
  let run = || {{
    let w = w
    if let Partial.Err(_) = {expression} {{ panic("unreachable") }} else {{ () }}
  }}
  run()
}}
fn main() {{}}
"#
        );
        let analysis = check(&source);
        assert!(
            analysis.errors().is_empty(),
            "{expression}: {:?}",
            analysis.errors()
        );
        assert_eq!(
            count(analysis.lints()),
            1,
            "{expression}: {:?}",
            analysis.lints()
        );
    }
}

#[test]
fn unreachable_partial_err_uses_go_nilability() {
    let typedefs = [(
        "go:example.com/values",
        r#"
pub struct Value { pub n: int }
pub type Count = int
pub type Bytes = Slice<byte>
pub fn Scalar() -> Partial<Count, error>
pub fn Record() -> Partial<Value, error>
pub fn ArrayValue() -> Partial<Array<byte, 4>, error>
pub fn SliceValue() -> Partial<Bytes, error>
pub fn Pointer() -> Partial<Ref<Value>, error>
pub fn Callback() -> Partial<fn() -> int, error>
pub fn Mapping() -> Partial<Map<string, int>, error>
pub interface Object { fn Name() -> string }
pub fn InterfaceValue() -> Partial<Object, error>
pub fn Generic<T>() -> Partial<T, error>
"#,
    )];
    for (call, expected) in [
        ("Scalar()", 1),
        ("Record()", 1),
        ("ArrayValue()", 1),
        ("SliceValue()", 0),
        ("Pointer()", 0),
        ("Callback()", 0),
        ("Mapping()", 0),
        ("InterfaceValue()", 0),
        ("Generic<int>()", 1),
        ("Generic<Ref<values.Value>>()", 0),
        ("Generic<T>()", 0),
    ] {
        let source = format!(
            r#"
import "go:example.com/values"
fn test<T>() {{
  if let Partial.Err(_) = values.{call} {{ panic("unreachable") }}
}}
"#
        );
        let errors = checker_errors(&source, &typedefs);
        assert!(!errors.iter().any(|d| d.is_error()), "{call}: {errors:?}");
        assert_eq!(
            count(&lint_with_go_typedefs(&source, &typedefs)),
            expected,
            "{call}"
        );
    }
}

#[test]
fn unreachable_partial_err_preserves_required_and_reachable_patterns() {
    let analysis = check(
        r#"
import "go:io"
fn native() -> Partial<int, string> { Partial.Err("expected") }
fn test(w: io.Writer, reader: io.Reader) {
  if let Partial.Err(_) = native() { () }
  if let Partial.Err(_) = io.ReadAll(reader) { () }
  let result = w.Write([1])
  if let Partial.Err(_) = result { () }
  let write = w.Write
  if let Partial.Err(_) = write([1]) { () }
  match w.Write([1]) {
    Partial.Ok(_) => (),
    Partial.Err(_) => (),
    Partial.Both(_, _) => (),
  }
}
fn main() {}
"#,
    );
    assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
    assert_eq!(count(analysis.lints()), 0);
}

#[test]
fn unreachable_partial_err_allow_is_specific() {
    let analysis = check(
        r#"
import "go:os"
#[allow(unreachable_partial_err)]
fn main() {
  if let Partial.Err(_) = os.Stdout.Write([1]) { panic("unreachable") }
  let unused = 1
}

"#,
    );
    assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
    assert_eq!(count(analysis.lints()), 0);
    assert!(
        analysis
            .lints()
            .iter()
            .any(|d| d.code_str() == Some("lint.unused_variable"))
    );
}

#[test]
fn unreachable_partial_err_on_rejected_code() {
    for (invalid, expected) in [("w.Write(42)", 1), ("42.Write([1])", 0)] {
        let source = format!(
            r#"
import "go:io"
fn test(w: io.Writer) {{
  if let Partial.Err(_) = {invalid} {{ panic("invalid call") }}
}}
fn main() {{}}
"#
        );
        let analysis = check(&source);
        assert!(
            !analysis.errors().is_empty(),
            "expected rejection: {invalid}"
        );
        assert_eq!(count(analysis.lints()), expected, "{invalid}");
    }
}

#[test]
fn unreachable_partial_err_through_go_interface() {
    let analysis = check(
        r#"
import "go:io"
struct Writer {}
impl Writer {
  fn Write(self, _data: Slice<byte>) -> Partial<int, error> { Partial.Err(io.EOF) }
}
fn write(w: io.Writer) {
  if let Partial.Err(_) = w.Write([1]) { panic("unreachable after Go wrapping") }
}
fn main() { write(Writer {}) }
"#,
    );
    assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
    assert_eq!(count(analysis.lints()), 1, "{:?}", analysis.lints());
}

#[test]
fn unreachable_partial_err_in_test_files() {
    for (attribute, expected) in [("", 1), ("#[allow(unreachable_partial_err)]", 0)] {
        let mut fs = MockFileSystem::new();
        fs.add_file(
            ENTRY_PACKAGE_ID,
            "contract.test.lis",
            &format!(
                r#"
import "go:os"
#[test]
{attribute}
fn contract() {{
  if let Partial.Err(_) = os.Stdout.Write([1]) {{ panic("unreachable") }}
}}
"#
            ),
        );
        let analysis = compile_script_entry(fs, "contract.test.lis", semantics::CompilePhase::Test);
        assert!(analysis.errors().is_empty(), "{:?}", analysis.errors());
        assert_eq!(count(analysis.lints()), expected, "{:?}", analysis.lints());
    }
}
