use std::backtrace::Backtrace;
use std::env::consts;
use std::io;
use std::io::{IsTerminal, Write};
use std::panic;
use std::panic::PanicHookInfo;

include!(concat!(env!("OUT_DIR"), "/go_version.rs"));

pub fn add_handler() {
    panic::set_hook(Box::new(|info: &PanicHookInfo<'_>| {
        print_compiler_bug_message(info);
    }));
}

fn print_compiler_bug_message(info: &PanicHookInfo<'_>) {
    let message = match (
        info.payload().downcast_ref::<&str>(),
        info.payload().downcast_ref::<String>(),
    ) {
        (Some(s), _) => (*s).to_string(),
        (_, Some(s)) => s.to_string(),
        (None, None) => "unknown error".into(),
    };

    let location = match info.location() {
        None => String::new(),
        Some(loc) => format!("{}:{}", loc.file(), loc.line()),
    };

    let backtrace = Backtrace::force_capture()
        .to_string()
        .lines()
        .map(|line| format!("  {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    let use_color = io::stderr().is_terminal();

    let (badge, reset) = if use_color {
        ("\x1b[41;30;1m", "\x1b[0m")
    } else {
        ("", "")
    };

    let _ = writeln!(
        io::stderr(),
        r#"
{badge} INTERNAL COMPILER ERROR {reset}

This is a bug in the Lisette compiler, not your code.

Please report this issue at: {blue}https://github.com/ivov/lisette/issues{reset}
Include the following data, and add a minimal way to reproduce if you can.

  Message: {red}{message}{reset}
  Location: {red}{location}{reset}
  Backtrace:
{red}{backtrace}{reset}

Lisette {version} · Go {go_toolchain} · {os}/{arch}"#,
        badge = badge,
        red = if use_color { "\x1b[31m" } else { "" },
        blue = if use_color { "\x1b[34m" } else { "" },
        reset = reset,
        message = message,
        location = location,
        backtrace = backtrace,
        version = env!("CARGO_PKG_VERSION"),
        go_toolchain = GO_TOOLCHAIN_VERSION,
        os = consts::OS,
        arch = consts::ARCH,
    );
}
