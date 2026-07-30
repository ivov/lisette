use std::io::IsTerminal;
use std::sync::LazyLock;

use owo_colors::OwoColorize;

static USE_COLOR: LazyLock<bool> =
    LazyLock::new(|| std::env::var("NO_COLOR").is_err() && std::io::stderr().is_terminal());

pub fn use_color() -> bool {
    *USE_COLOR
}

pub fn terminal_width() -> usize {
    normalize_terminal_width(platform_terminal_width())
}

fn normalize_terminal_width(width: Option<u16>) -> usize {
    width.filter(|width| *width > 0).map_or(100, usize::from)
}

// Platform implementations adapted from terminal_size 0.4.3:
// https://github.com/eminence/terminal-size
// Copyright (c) 2015 The terminal-size Developers.
// Licensed under MIT

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn platform_terminal_width() -> Option<u16> {
    use std::os::fd::AsRawFd;
    use std::os::raw::{c_int, c_ulong};

    #[derive(Default)]
    #[repr(C)]
    struct WindowSize {
        rows: u16,
        columns: u16,
        pixel_width: u16,
        pixel_height: u16,
    }

    unsafe extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    #[cfg(target_os = "linux")]
    const GET_WINDOW_SIZE: c_ulong = 0x5413;
    #[cfg(target_os = "macos")]
    const GET_WINDOW_SIZE: c_ulong = 0x4008_7468;

    let mut size = WindowSize::default();
    let stderr = std::io::stderr();
    // SAFETY: stderr supplies a live file descriptor for the duration of the
    // call, and `size` is the C-compatible buffer required by TIOCGWINSZ.
    let result = unsafe { ioctl(stderr.as_raw_fd(), GET_WINDOW_SIZE, &mut size) };
    (result == 0).then_some(size.columns)
}

#[cfg(windows)]
fn platform_terminal_width() -> Option<u16> {
    use std::ffi::c_void;
    use std::os::windows::io::{AsHandle, AsRawHandle};

    #[derive(Clone, Copy, Default)]
    #[repr(C)]
    struct Coordinate {
        x: i16,
        y: i16,
    }

    #[derive(Clone, Copy, Default)]
    #[repr(C)]
    struct SmallRectangle {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }

    #[derive(Default)]
    #[repr(C)]
    struct ConsoleScreenBufferInfo {
        size: Coordinate,
        cursor_position: Coordinate,
        attributes: u16,
        window: SmallRectangle,
        maximum_window_size: Coordinate,
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        #[link_name = "GetConsoleScreenBufferInfo"]
        fn get_console_screen_buffer_info(
            output: *mut c_void,
            info: *mut ConsoleScreenBufferInfo,
        ) -> i32;
    }

    let stderr = std::io::stderr();
    let mut info = ConsoleScreenBufferInfo::default();
    // SAFETY: the borrowed stderr handle remains live for the call, and
    // `info` exactly matches the Windows CONSOLE_SCREEN_BUFFER_INFO layout.
    let result = unsafe {
        get_console_screen_buffer_info(stderr.as_handle().as_raw_handle().cast(), &mut info)
    };
    if result == 0 {
        return None;
    }

    let width = i32::from(info.window.right) - i32::from(info.window.left) + 1;
    u16::try_from(width).ok()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn platform_terminal_width() -> Option<u16> {
    None
}

pub fn format_elapsed(elapsed: std::time::Duration) -> String {
    let time_str = if elapsed.as_secs() >= 1 {
        format!("{:.2}s", elapsed.as_secs_f64())
    } else if elapsed.as_millis() > 0 {
        format!("{}ms", elapsed.as_millis())
    } else {
        format!("{}μs", elapsed.as_micros())
    };

    if use_color() {
        format!("{}", format!("({})", time_str).dimmed())
    } else {
        format!("({})", time_str)
    }
}

pub fn format_backticks(text: &str, use_color: bool) -> String {
    if !use_color {
        return text.to_string();
    }

    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    let mut segment_start = 0;

    while let Some((i, ch)) = chars.next() {
        if ch == '`' {
            if i > segment_start {
                result.push_str(&text[segment_start..i]);
            }

            let mut found_closing = false;
            for (j, inner_ch) in chars.by_ref() {
                if inner_ch == '`' {
                    let quoted = &text[i + 1..j];
                    result.push_str(&format!("{}", quoted.bright_magenta()));
                    segment_start = j + 1;
                    found_closing = true;
                    break;
                }
            }

            if !found_closing {
                result.push_str(&text[i..]);
                segment_start = text.len();
            }
        }
    }

    if segment_start < text.len() {
        result.push_str(&text[segment_start..]);
    }

    result
}

fn format_help_text(text: &str, use_color: bool) -> String {
    let mut out = String::new();
    let mut chars = text.chars();

    while let Some(ch) = chars.next() {
        match ch {
            '`' => {
                let (content, closed) = take_until(&mut chars, '`');
                if !closed {
                    out.push('`');
                    out.push_str(&content);
                } else if use_color {
                    out.push_str(&format!("{}", content.bright_magenta()));
                } else {
                    out.push_str(&content);
                }
            }
            '<' => {
                let (content, closed) = take_until(&mut chars, '>');
                if !closed {
                    out.push('<');
                    out.push_str(&content);
                } else if use_color {
                    out.push_str(&format!("{}", format!("<{}>", content).green()));
                } else {
                    out.push('<');
                    out.push_str(&content);
                    out.push('>');
                }
            }
            '{' => {
                let (content, closed) = take_until(&mut chars, '}');
                if !closed {
                    out.push('{');
                    out.push_str(&content);
                    continue;
                }
                let (inner, style) = if let Some(rest) = content.strip_suffix(":b") {
                    (rest, 'b')
                } else if let Some(rest) = content.strip_suffix(":g") {
                    (rest, 'g')
                } else if let Some(rest) = content.strip_suffix(":d") {
                    (rest, 'd')
                } else {
                    (content.as_str(), 'g')
                };
                if use_color {
                    let painted = match style {
                        'b' => format!("{}", inner.blue()),
                        'd' => format!("{}", inner.dimmed()),
                        _ => format!("{}", inner.green()),
                    };
                    out.push_str(&painted);
                } else {
                    out.push_str(inner);
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

fn take_until(chars: &mut std::str::Chars<'_>, close: char) -> (String, bool) {
    let mut content = String::new();
    for c in chars.by_ref() {
        if c == close {
            return (content, true);
        }
        content.push(c);
    }
    (content, false)
}

pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn print_preview_notice(feature: &str, plural: bool) {
    eprintln!();
    let verb = if plural { "are" } else { "is" };
    if use_color() {
        eprintln!(
            "  ! {feature} {verb} in {} · Bug reports are welcome",
            "early preview".yellow().underline()
        );
    } else {
        eprintln!("  ! {feature} {verb} in early preview · Bug reports are welcome");
    }
}

/// How an added replaced dependency is labeled in the success line.
pub enum ReplacementLabel<'a> {
    Module { path: &'a str, version: &'a str },
    Local { path: &'a str },
}

pub fn print_add_success(
    module_path: &str,
    version: &str,
    edges: &std::collections::HashMap<String, Vec<String>>,
    versions: &std::collections::HashMap<String, String>,
    upgraded_directs: &[(&str, &str, &str)],
    replacement: Option<ReplacementLabel<'_>>,
) {
    eprintln!();

    let colored = use_color();
    for (path, old, new) in upgraded_directs {
        if colored {
            eprintln!(
                "  ↑ Upgraded {} {} → {}",
                path.green(),
                old.blue(),
                new.blue()
            );
        } else {
            eprintln!("  ↑ Upgraded {} {} → {}", path, old, new);
        }
    }
    if !upgraded_directs.is_empty() {
        eprintln!();
    }

    match replacement {
        Some(ReplacementLabel::Module {
            path: replacement_path,
            version: replacement_version,
        }) if colored => eprintln!(
            "  ✓ Added {} (replaced by {} {})",
            module_path.green(),
            replacement_path.green(),
            replacement_version.blue()
        ),
        Some(ReplacementLabel::Module {
            path: replacement_path,
            version: replacement_version,
        }) => eprintln!(
            "  ✓ Added {} (replaced by {} {})",
            module_path, replacement_path, replacement_version
        ),
        Some(ReplacementLabel::Local { path }) if colored => eprintln!(
            "  ✓ Added {} (local dir {})",
            module_path.green(),
            path.green()
        ),
        Some(ReplacementLabel::Local { path }) => {
            eprintln!("  ✓ Added {} (local dir {})", module_path, path)
        }
        None if colored => eprintln!("  ✓ Added {} {}", module_path.green(), version.blue()),
        None => eprintln!("  ✓ Added {} {}", module_path, version),
    }

    let mut printer = TreePrinter {
        edges,
        versions,
        colored,
        visited: std::collections::HashSet::new(),
    };
    printer.visited.insert(module_path.to_string());
    printer.print_children(module_path, "    ");
}

struct TreePrinter<'a> {
    edges: &'a std::collections::HashMap<String, Vec<String>>,
    versions: &'a std::collections::HashMap<String, String>,
    colored: bool,
    visited: std::collections::HashSet<String>,
}

impl TreePrinter<'_> {
    fn print_children(&mut self, node: &str, prefix: &str) {
        let Some(children) = self.edges.get(node) else {
            return;
        };
        let mut sorted: Vec<&String> = children.iter().collect();
        sorted.sort();
        for (i, child) in sorted.iter().enumerate() {
            let is_last = i == sorted.len() - 1;
            self.print_node(child, prefix, is_last);
        }
    }

    fn print_node(&mut self, node: &str, prefix: &str, is_last: bool) {
        let branch = if is_last { "└─ " } else { "├─ " };
        let version = self.versions.get(node).map(String::as_str).unwrap_or("");
        let already_seen = !self.visited.insert(node.to_string());

        if self.colored {
            if already_seen {
                eprintln!(
                    "{}{}{} {} {}",
                    prefix,
                    branch,
                    node.green(),
                    version.blue(),
                    "(*)".dimmed()
                );
            } else {
                eprintln!("{}{}{} {}", prefix, branch, node.green(), version.blue());
            }
        } else if already_seen {
            eprintln!("{}{}{} {} (*)", prefix, branch, node, version);
        } else {
            eprintln!("{}{}{} {}", prefix, branch, node, version);
        }

        if already_seen {
            return;
        }

        let child_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });
        self.print_children(node, &child_prefix);
    }
}

pub fn print_sync_summary(
    trimmed: &[deps::TrimmedVia],
    promoted: &[String],
    removed: &[String],
    leading_blank: bool,
) {
    if leading_blank {
        eprintln!();
    }

    if trimmed.is_empty() && promoted.is_empty() && removed.is_empty() {
        eprintln!("  ✓ Manifest already in sync");
        return;
    }

    let colored = use_color();

    let promoted_set: std::collections::HashSet<&str> =
        promoted.iter().map(String::as_str).collect();
    let removed_set: std::collections::HashSet<&str> = removed.iter().map(String::as_str).collect();

    for entry in trimmed {
        if promoted_set.contains(entry.module_path.as_str())
            || removed_set.contains(entry.module_path.as_str())
        {
            continue;
        }
        let parents = entry.removed_parents.join(", ");
        if colored {
            eprintln!(
                "  ↓ Trimmed via for {} (removed: {})",
                entry.module_path.green(),
                parents.blue()
            );
        } else {
            eprintln!(
                "  ↓ Trimmed via for {} (removed: {})",
                entry.module_path, parents
            );
        }
    }

    for path in promoted {
        if colored {
            eprintln!("  ↑ Promoted {} to direct", path.green());
        } else {
            eprintln!("  ↑ Promoted {} to direct", path);
        }
    }

    for path in removed {
        if colored {
            eprintln!("  − Removed {}", path.green());
        } else {
            eprintln!("  − Removed {}", path);
        }
    }
}

pub fn print_progress(msg: &str) {
    if use_color() {
        eprintln!("  · {}", msg.dimmed());
    } else {
        eprintln!("  · {}", msg);
    }
}

pub fn print_warning(msg: &str) {
    if use_color() {
        eprintln!("  {} {}", "!".yellow(), msg);
    } else {
        eprintln!("  ! {}", msg);
    }
}

pub fn print_help(text: &str) {
    println!();
    println!("{}", format_help_text(text, use_color()));
}

pub fn print_dimmed(text: &str) {
    if use_color() {
        println!("{}", text.dimmed());
    } else {
        println!("{}", text);
    }
}

#[macro_export]
macro_rules! error {
    ($msg:expr, $reason:expr) => {{
        let msg = $crate::output::capitalize_first(&$msg);
        let reason = $reason;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
        }
    }};
}

#[macro_export]
macro_rules! cli_error {
    ($msg:literal, $reason:literal, $hint:literal) => {{
        let msg = $crate::output::capitalize_first($msg);
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks($reason, true);
            let formatted_hint = $crate::output::format_backticks($hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", $reason);
            eprintln!(" · help: {}", $hint);
        }
    }};
    ($msg:expr, $reason:expr, $hint:literal) => {{
        let msg = $crate::output::capitalize_first(&$msg);
        let reason = $reason;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            let formatted_hint = $crate::output::format_backticks($hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
            eprintln!(" · help: {}", $hint);
        }
    }};
    ($msg:expr, $reason:expr, $hint:expr) => {{
        let msg = $crate::output::capitalize_first(&$msg);
        let reason = $reason;
        let hint = $hint;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            let formatted_hint = $crate::output::format_backticks(&hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
            eprintln!(" · help: {}", hint);
        }
    }};
}

#[cfg(test)]
mod terminal_width_tests {
    use super::normalize_terminal_width;

    #[test]
    fn uses_detected_width() {
        assert_eq!(normalize_terminal_width(Some(132)), 132);
    }

    #[test]
    fn falls_back_when_detection_fails() {
        assert_eq!(normalize_terminal_width(None), 100);
    }

    #[test]
    fn falls_back_when_terminal_reports_zero() {
        assert_eq!(normalize_terminal_width(Some(0)), 100);
    }
}
