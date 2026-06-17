use crate::passes::walk::NodeCtx;
use syntax::ast::{BinaryOperator, Expression, Literal};
use syntax::types::Type;

/// Go's `internal/syslist` known operating systems. Deliberately broad (it
/// keeps obsolete and experimental targets such as `nacl` and `hurd`) so the
/// lint fires only on values that are never a valid `GOOS`.
const KNOWN_OS: &[&str] = &[
    "aix",
    "android",
    "darwin",
    "dragonfly",
    "freebsd",
    "hurd",
    "illumos",
    "ios",
    "js",
    "linux",
    "nacl",
    "netbsd",
    "openbsd",
    "plan9",
    "solaris",
    "wasip1",
    "windows",
    "zos",
];

/// Go's `internal/syslist` known architectures, broad for the same reason as
/// [`KNOWN_OS`].
const KNOWN_ARCH: &[&str] = &[
    "386",
    "amd64",
    "amd64p32",
    "arm",
    "armbe",
    "arm64",
    "arm64be",
    "loong64",
    "mips",
    "mipsle",
    "mips64",
    "mips64le",
    "mips64p32",
    "mips64p32le",
    "ppc",
    "ppc64",
    "ppc64le",
    "riscv",
    "riscv64",
    "s390",
    "s390x",
    "sparc",
    "sparc64",
    "wasm",
];

enum RuntimeConst {
    Goos,
    Goarch,
}

impl RuntimeConst {
    /// `(const_name, kind, examples)` for the diagnostic message.
    fn describe(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            RuntimeConst::Goos => (
                "GOOS",
                "operating system",
                "`linux`, `darwin`, or `windows`",
            ),
            RuntimeConst::Goarch => ("GOARCH", "architecture", "`amd64`, `arm64`, or `386`"),
        }
    }

    fn is_known(&self, value: &str) -> bool {
        match self {
            RuntimeConst::Goos => KNOWN_OS.contains(&value),
            RuntimeConst::Goarch => KNOWN_ARCH.contains(&value),
        }
    }
}

pub fn check_goos_goarch_comparison(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    let always_true = match operator {
        BinaryOperator::Equal => false,
        BinaryOperator::NotEqual => true,
        _ => return,
    };

    let left = left.unwrap_parens();
    let right = right.unwrap_parens();

    let Some((runtime_const, value)) =
        compared_pair(left, right).or_else(|| compared_pair(right, left))
    else {
        return;
    };

    if runtime_const.is_known(value) {
        return;
    }

    let (const_name, kind, examples) = runtime_const.describe();
    ctx.sink.push(diagnostics::lint::goos_goarch_comparison(
        span,
        always_true,
        const_name,
        kind,
        examples,
    ));
}

/// `(runtime_const, literal)` when `subject` is `runtime.GOOS`/`GOARCH` and
/// `other` is a plain string literal.
fn compared_pair<'a>(
    subject: &Expression,
    other: &'a Expression,
) -> Option<(RuntimeConst, &'a str)> {
    Some((runtime_const(subject)?, string_literal(other)?))
}

fn runtime_const(expression: &Expression) -> Option<RuntimeConst> {
    let Expression::DotAccess {
        expression: base,
        member,
        ..
    } = expression
    else {
        return None;
    };
    let Type::ImportNamespace(module) = base.get_type().strip_refs() else {
        return None;
    };
    if module != "go:runtime" {
        return None;
    }
    match member.as_str() {
        "GOOS" => Some(RuntimeConst::Goos),
        "GOARCH" => Some(RuntimeConst::Goarch),
        _ => None,
    }
}

fn string_literal(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Literal {
            literal: Literal::String { value, .. },
            ..
        } => Some(value),
        _ => None,
    }
}
