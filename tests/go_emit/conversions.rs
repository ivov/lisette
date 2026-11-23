use crate::_harness::go::go_available;
use crate::_harness::lists::FailingList;

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::process::Command;

use rayon::prelude::*;

use crate::_harness::go::{
    CompileError, compile_go_test, prelude_dir, read_go_toolchain, repo_root, run_go_build,
    write_go_mod, write_subpackage,
};

struct Conversion {
    name: &'static str,
    source: &'static str,
    slot: &'static str,
    value: &'static str,
    zero: &'static str,
    check: &'static str,
}

const CONVERSIONS: &[Conversion] = &[
    Conversion {
        name: "interface",
        source: "Boom",
        slot: "error",
        value: "Boom {}",
        zero: "Boom {}",
        check: "if v.Error() != \"boom\" { panic(\"wrong value\") }",
    },
    Conversion {
        name: "array",
        source: "Array<Boom, 1>",
        slot: "Array<error, 1>",
        value: "[Boom {}]",
        zero: "[Boom {}]",
        check: "if v[0].Error() != \"boom\" { panic(\"wrong value\") }",
    },
    Conversion {
        name: "tuple",
        source: "(Boom, int)",
        slot: "(error, int)",
        value: "(Boom {}, 1)",
        zero: "(Boom {}, 0)",
        check: "if v.0.Error() != \"boom\" { panic(\"wrong value\") }",
    },
    Conversion {
        name: "function",
        source: "fn() -> ()",
        slot: "Unknown",
        value: "unit",
        zero: "unit",
        check: "if fmt.Sprintf(\"%T\", v) != \"func()\" { panic(fmt.Sprintf(\"wrong Go type %T\", v)) }",
    },
    Conversion {
        name: "implicit_some",
        source: "io.Reader",
        slot: "Option<io.Reader>",
        value: "strings.NewReader(\"x\")",
        zero: "None",
        check: "match v { Some(_) => {}, None => panic(\"lost the value\") }",
    },
];

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Value,
    Pair,
}

#[derive(Clone, Copy, PartialEq)]
enum Body {
    Unit,
    Option,
    Result,
}

struct Producer {
    name: &'static str,
    body: Body,
    write: fn(&str) -> String,
}

const PRODUCERS: &[Producer] = &[
    Producer {
        name: "plain",
        body: Body::Unit,
        write: |source| format!("{source}()"),
    },
    Producer {
        name: "paren",
        body: Body::Unit,
        write: |source| format!("({source}())"),
    },
    Producer {
        name: "block",
        body: Body::Unit,
        write: |source| format!("{{ {source}() }}"),
    },
    Producer {
        name: "if",
        body: Body::Unit,
        write: |source| format!("if flag() {{ {source}() }} else {{ {source}() }}"),
    },
    Producer {
        name: "match",
        body: Body::Unit,
        write: |source| format!("match flag() {{ true => {source}(), false => {source}() }}"),
    },
    Producer {
        name: "break",
        body: Body::Unit,
        write: |source| format!("loop {{ break {source}() }}"),
    },
    Producer {
        name: "propagate",
        body: Body::Option,
        write: |source| format!("opt_{source}()?"),
    },
    Producer {
        name: "ok_or",
        body: Body::Result,
        write: |source| format!("opt_{source}().ok_or(\"none\")?"),
    },
    Producer {
        name: "map_err",
        body: Body::Result,
        write: |source| format!("res_{source}().map_err(|e| e)?"),
    },
];

struct Position {
    name: &'static str,
    source: Source,
    returns_slot: bool,
    write: fn(&str, &str) -> String,
}

const POSITIONS: &[Position] = &[
    Position {
        name: "let",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| format!("let x: {slot} = {value}\n  check(x)"),
    },
    Position {
        name: "destructuring",
        source: Source::Pair,
        returns_slot: false,
        write: |slot, value| {
            format!("let (x, n): ({slot}, int) = {value}\n  check(x)\n  fmt.Println(n)")
        },
    },
    Position {
        name: "assignment",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| format!("let mut x: {slot} = initial()\n  x = {value}\n  check(x)"),
    },
    Position {
        name: "field_assignment",
        source: Source::Value,
        returns_slot: false,
        write: |_, value| {
            format!("let mut h = Holder {{ f: initial() }}\n  h.f = {value}\n  check(h.f)")
        },
    },
    Position {
        name: "return",
        source: Source::Value,
        returns_slot: true,
        write: |_, value| format!("if flag() {{\n    return {value}\n  }}\n  initial()"),
    },
    Position {
        name: "tail",
        source: Source::Value,
        returns_slot: true,
        write: |_, value| value.to_string(),
    },
    Position {
        name: "closure_tail",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| format!("let c = || -> {slot} {{ {value} }}\n  check(c())"),
    },
    Position {
        name: "argument",
        source: Source::Value,
        returns_slot: false,
        write: |_, value| format!("check({value})"),
    },
    Position {
        name: "method_argument",
        source: Source::Value,
        returns_slot: false,
        write: |_, value| format!("Taker {{}}.take({value})"),
    },
    Position {
        name: "append",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| {
            format!("let mut s: Slice<{slot}> = []\n  s = s.append({value})\n  check(s[0])")
        },
    },
    Position {
        name: "send",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| {
            format!(
                "let ch = Channel.buffered<{slot}>(1)\n  ch.send({value})\n  match ch.receive() {{ Some(v) => check(v), None => panic(\"empty\") }}"
            )
        },
    },
    Position {
        name: "struct_field",
        source: Source::Value,
        returns_slot: false,
        write: |_, value| format!("let h = Holder {{ f: {value} }}\n  check(h.f)"),
    },
    Position {
        name: "slice_element",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| format!("let s: Slice<{slot}> = [{value}]\n  check(s[0])"),
    },
    Position {
        name: "map_from",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| {
            format!(
                "let m: Map<string, {slot}> = Map.from([(\"a\", {value})])\n  match m.get(\"a\") {{ Some(v) => check(v), None => panic(\"missing\") }}"
            )
        },
    },
    Position {
        name: "some",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| {
            format!(
                "let o: Option<{slot}> = Some({value})\n  match o {{ Some(v) => check(v), None => panic(\"missing\") }}"
            )
        },
    },
    Position {
        name: "tuple_element",
        source: Source::Value,
        returns_slot: false,
        write: |slot, value| format!("let t: ({slot}, int) = ({value}, 1)\n  check(t.0)"),
    },
];

struct Shape {
    name: String,
    source: String,
}

enum Compiled {
    Written,
    Rejected,
    Failed(String),
}

#[test]
fn conversions_give_valid_go() {
    if !go_available() {
        eprintln!("skipping conversions_give_valid_go: `go` not found");
        return;
    }

    let target = repo_root().join("target/go_emit_conversions");
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).expect("create target/go_emit_conversions");
    let go_toolchain = read_go_toolchain().expect("read go-toolchain");
    write_go_mod(&target, &prelude_dir(), &go_toolchain).expect("write go.mod");

    let shapes = shapes();
    let compiled: Vec<(&Shape, Compiled)> = shapes
        .par_iter()
        .map(|shape| (shape, compile(&target, shape)))
        .collect();

    let mut go_errors = HashMap::new();
    if let Err(output) = run_go_build(&target) {
        assert!(
            record_first_errors(&output, &mut go_errors),
            "go build failed outside any package:\n{output}"
        );
    }
    let runnable: Vec<String> = compiled
        .iter()
        .filter(|(shape, compiled)| {
            matches!(compiled, Compiled::Written)
                && !go_errors.contains_key(&package_name(&shape.name))
        })
        .map(|(shape, _)| format!("./{}", package_name(&shape.name)))
        .collect();
    if let Err(output) = run_go_test(&target, &runnable) {
        record_first_errors(&output, &mut go_errors);
    }

    let mut rejected = 0;
    let mut ran = BTreeSet::new();
    let mut failures = BTreeMap::new();
    for (shape, compiled) in &compiled {
        ran.insert(shape.name.clone());
        let error = match compiled {
            Compiled::Rejected => {
                rejected += 1;
                continue;
            }
            Compiled::Failed(message) => format!("compiler: {message}"),
            Compiled::Written => match go_errors.get(&package_name(&shape.name)) {
                Some(line) => format!("go: {line}"),
                None => continue,
            },
        };
        let repro = target.join(package_name(&shape.name)).join("test.lis");
        failures.insert(
            shape.name.clone(),
            format!("{error} (repro: {})", repro.display()),
        );
    }

    eprintln!("{} shapes, {rejected} rejected by Lisette", shapes.len());
    FailingList::read("go_emit/failing_conversions.txt").assert_matches(&ran, &failures, true);
}

fn shapes() -> Vec<Shape> {
    let mut shapes = Vec::new();
    for conversion in CONVERSIONS {
        for producer in PRODUCERS {
            for position in POSITIONS {
                if position.returns_slot && producer.body != Body::Unit {
                    continue;
                }
                if position.name == "closure_tail" && producer.body != Body::Unit {
                    continue;
                }
                shapes.push(Shape {
                    name: format!("{}__{}__{}", conversion.name, producer.name, position.name),
                    source: program(conversion, producer, position),
                });
            }
        }
    }
    shapes
}

fn program(conversion: &Conversion, producer: &Producer, position: &Position) -> String {
    let Conversion {
        source,
        slot,
        value,
        zero,
        check,
        ..
    } = conversion;
    let helper = match position.source {
        Source::Value => "value",
        Source::Pair => "pair",
    };
    let written = (producer.write)(helper);
    let statements = (position.write)(slot, &written);
    let (body, entry) = if position.returns_slot {
        (
            format!("fn produce() -> {slot} {{\n  {statements}\n}}"),
            "check(produce())".to_string(),
        )
    } else {
        match producer.body {
            Body::Unit => (
                format!("fn produce() {{\n  {statements}\n}}"),
                "produce()".to_string(),
            ),
            Body::Option => (
                format!("fn produce() -> Option<()> {{\n  {statements}\n  Some(())\n}}"),
                "match produce() { Some(_) => {}, None => panic(\"no value\") }".to_string(),
            ),
            Body::Result => (
                format!("fn produce() -> Result<(), string> {{\n  {statements}\n  Ok(())\n}}"),
                "match produce() { Ok(_) => {}, Err(e) => panic(e) }".to_string(),
            ),
        }
    };
    format!(
        r#"import "go:fmt"
import "go:io"
import "go:strings"

struct Boom {{}}

impl Boom {{
  fn Error(self) -> string {{ "boom" }}
}}

struct Holder {{
  f: {slot},
}}

struct Taker {{}}

impl Taker {{
  fn take(self, v: {slot}) {{ check(v) }}
}}

fn unit() {{}}

fn flag() -> bool {{ true }}

fn reader() -> io.Reader {{ strings.NewReader("x") }}

fn value() -> {source} {{ {value} }}

fn pair() -> ({source}, int) {{ (value(), 1) }}

fn opt_value() -> Option<{source}> {{ Some(value()) }}

fn opt_pair() -> Option<({source}, int)> {{ Some(pair()) }}

fn res_value() -> Result<{source}, string> {{ Ok(value()) }}

fn res_pair() -> Result<({source}, int), string> {{ Ok(pair()) }}

fn initial() -> {slot} {{ {zero} }}

fn check(v: {slot}) {{
  {check}
}}

{body}

fn main() {{
  {entry}
  fmt.Println("ok")
}}
"#
    )
}

fn package_name(shape: &str) -> String {
    format!("test_{shape}")
}

fn compile(target: &Path, shape: &Shape) -> Compiled {
    let package_dir = target.join(package_name(&shape.name));
    fs::create_dir_all(&package_dir).expect("create package dir");
    fs::write(package_dir.join("test.lis"), &shape.source).expect("write test.lis");

    let result = catch_unwind(AssertUnwindSafe(|| {
        compile_go_test(&shape.source, &package_name(&shape.name))
    }));
    match result {
        Ok(Ok(emitted)) => {
            write_subpackage(target, &shape.name, &emitted.go_code, emitted.entry)
                .expect("write subpackage");
            Compiled::Written
        }
        Ok(Err(CompileError::Inference(_))) => Compiled::Rejected,
        Ok(Err(CompileError::Emit(message))) => Compiled::Failed(message),
        Err(payload) => Compiled::Failed(panic_message(payload)),
    }
}

fn run_go_test(target: &Path, packages: &[String]) -> Result<String, String> {
    if packages.is_empty() {
        return Ok(String::new());
    }
    let output = Command::new("go")
        .arg("test")
        .args(["-timeout", "60s"])
        .args(packages)
        .current_dir(target)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn go test");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => format!("panic: {message}"),
        Err(payload) => match payload.downcast::<&str>() {
            Ok(message) => format!("panic: {message}"),
            Err(_) => "panic".to_string(),
        },
    }
}

fn record_first_errors(output: &str, errors: &mut HashMap<String, String>) -> bool {
    let mut found = false;
    let mut current_failure: Option<String> = None;
    for line in output.lines() {
        let path = line.strip_prefix("./").unwrap_or(line);
        if let Some(rest) = line.strip_prefix("--- FAIL: ") {
            current_failure = Some(rest.to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("FAIL\t") {
            let package = rest
                .split_whitespace()
                .next()
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or_default();
            if package.starts_with("test_") {
                found = true;
                let message = current_failure
                    .take()
                    .unwrap_or_else(|| "go test failed".to_string());
                errors.entry(package.to_string()).or_insert(message);
            }
            continue;
        }
        if let Some(failure) = current_failure.as_mut()
            && line.contains("panic")
        {
            failure.push_str(" | ");
            failure.push_str(line.trim());
        }
        let Some((package, _)) = path.split_once('/') else {
            continue;
        };
        if package.starts_with("test_") {
            found = true;
            errors
                .entry(package.to_string())
                .or_insert_with(|| line.to_string());
        }
    }
    found
}
