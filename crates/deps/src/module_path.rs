//! Ported from `golang.org/x/mod/module`, rather than shelled out to, because
//! this runs inside `lis check` and the language server, where the Go toolchain
//! must not be invoked.

const BAD_WINDOWS_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Go's `module.CheckPath`.
pub fn check_module_path(path: &str) -> Result<(), String> {
    check_path_elements(path)?;

    let first = path.split('/').next().unwrap_or(path);
    if first.is_empty() {
        return Err("leading slash".to_string());
    }
    if !first.contains('.') {
        return Err("missing dot in first path element".to_string());
    }
    if path.starts_with('-') {
        return Err("leading dash in first path element".to_string());
    }
    if let Some(bad) = first.chars().find(|&c| !first_path_ok(c)) {
        return Err(format!("invalid char `{}` in first path element", bad));
    }
    if split_path_version(path).is_none() {
        return Err("invalid major version suffix".to_string());
    }
    Ok(())
}

fn check_path_elements(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("empty path".to_string());
    }
    if path.starts_with('-') {
        return Err("leading dash".to_string());
    }
    if path.contains("//") {
        return Err("double slash".to_string());
    }
    if path.ends_with('/') {
        return Err("trailing slash".to_string());
    }
    for element in path.split('/') {
        check_element(element)?;
    }
    Ok(())
}

fn check_element(element: &str) -> Result<(), String> {
    if element.is_empty() {
        return Err("empty path element".to_string());
    }
    if element.chars().all(|c| c == '.') {
        return Err(format!("invalid path element `{}`", element));
    }
    if element.starts_with('.') {
        return Err("leading dot in path element".to_string());
    }
    if element.ends_with('.') {
        return Err("trailing dot in path element".to_string());
    }
    if let Some(bad) = element.chars().find(|&c| !mod_path_ok(c)) {
        return Err(format!("invalid char `{}`", bad));
    }

    let short = element.split('.').next().unwrap_or(element);
    if let Some(bad) = BAD_WINDOWS_NAMES
        .iter()
        .find(|name| name.eq_ignore_ascii_case(short))
    {
        return Err(format!(
            "`{}` disallowed as a path element component on Windows",
            bad
        ));
    }

    // A Windows short-name ends in a tilde and digits.
    if let Some(tilde) = short.rfind('~')
        && tilde < short.len() - 1
        && short[tilde + 1..].bytes().all(|b| b.is_ascii_digit())
    {
        return Err("trailing tilde and digits in path element".to_string());
    }
    Ok(())
}

fn mod_path_ok(c: char) -> bool {
    matches!(c, '-' | '.' | '_' | '~') || c.is_ascii_alphanumeric()
}

fn first_path_ok(c: char) -> bool {
    matches!(c, '-' | '.') || c.is_ascii_digit() || c.is_ascii_lowercase()
}

/// Go's `module.SplitPathVersion`. `None` means a suffix present but malformed.
pub(crate) fn split_path_version(path: &str) -> Option<(&str, &str)> {
    if path.starts_with("gopkg.in/") {
        return split_gopkg_in(path);
    }

    let bytes = path.as_bytes();
    let mut i = bytes.len();
    let mut dot = false;
    while i > 0 && (bytes[i - 1].is_ascii_digit() || bytes[i - 1] == b'.') {
        dot |= bytes[i - 1] == b'.';
        i -= 1;
    }
    if i <= 1 || i == bytes.len() || bytes[i - 1] != b'v' || bytes[i - 2] != b'/' {
        return Some((path, ""));
    }

    let path_major = &path[i - 2..];
    if dot || path_major.len() <= 2 || path_major.as_bytes()[2] == b'0' || path_major == "/v1" {
        return None;
    }
    Some((&path[..i - 2], path_major))
}

/// Any major is legal here, `.v0` included, unlike a leading zero elsewhere.
fn split_gopkg_in(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    let mut i = path.len();
    if path.ends_with("-unstable") {
        i -= "-unstable".len();
    }
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i <= 1 || bytes[i - 1] != b'v' || bytes[i - 2] != b'.' {
        return None;
    }
    let path_major = &path[i - 2..];
    if path_major.len() <= 2 || (path_major.as_bytes()[2] == b'0' && path_major != ".v0") {
        return None;
    }
    Some((&path[..i - 2], path_major))
}

#[cfg(test)]
mod module_path_tests {
    use super::*;

    /// Go's `checkPathTests` from x/mod v0.38.0, read against its `ok` column.
    /// Its 105th case, `x.y/\xFFz`, tests invalid UTF-8, which a `&str` cannot
    /// hold.
    #[test]
    fn agrees_with_go_check_path() {
        const GO_TABLE: [(&str, bool); 104] = [
            ("x.y/z", true),
            ("x.y", true),
            ("", false),
            ("/x.y/z", false),
            ("x./z", false),
            (".x/z", false),
            ("-x/z", false),
            ("x..y/z", true),
            ("x.y/z/../../w", false),
            ("x.y//z", false),
            ("x.y/z//w", false),
            ("x.y/z/", false),
            ("x.y/z/v0", false),
            ("x.y/z/v1", false),
            ("x.y/z/v2", true),
            ("x.y/z/v2.0", false),
            ("X.y/z", false),
            ("!x.y/z", false),
            ("_x.y/z", false),
            ("x.y!/z", false),
            ("x.y\"/z", false),
            ("x.y#/z", false),
            ("x.y$/z", false),
            ("x.y%/z", false),
            ("x.y&/z", false),
            ("x.y'/z", false),
            ("x.y(/z", false),
            ("x.y)/z", false),
            ("x.y*/z", false),
            ("x.y+/z", false),
            ("x.y,/z", false),
            ("x.y-/z", true),
            ("x.y./zt", false),
            ("x.y:/z", false),
            ("x.y;/z", false),
            ("x.y</z", false),
            ("x.y=/z", false),
            ("x.y>/z", false),
            ("x.y?/z", false),
            ("x.y@/z", false),
            ("x.y[/z", false),
            ("x.y\\/z", false),
            ("x.y]/z", false),
            ("x.y^/z", false),
            ("x.y_/z", false),
            ("x.y`/z", false),
            ("x.y{/z", false),
            ("x.y}/z", false),
            ("x.y~/z", false),
            ("x.y/z!", false),
            ("x.y/z\"", false),
            ("x.y/z#", false),
            ("x.y/z$", false),
            ("x.y/z%", false),
            ("x.y/z&", false),
            ("x.y/z'", false),
            ("x.y/z(", false),
            ("x.y/z)", false),
            ("x.y/z*", false),
            ("x.y/z++", false),
            ("x.y/z,", false),
            ("x.y/z-", true),
            ("x.y/z.t", true),
            ("x.y/z/t", true),
            ("x.y/z:", false),
            ("x.y/z;", false),
            ("x.y/z<", false),
            ("x.y/z=", false),
            ("x.y/z>", false),
            ("x.y/z?", false),
            ("x.y/z@", false),
            ("x.y/z[", false),
            ("x.y/z\\", false),
            ("x.y/z]", false),
            ("x.y/z^", false),
            ("x.y/z_", true),
            ("x.y/z`", false),
            ("x.y/z{", false),
            ("x.y/z}", false),
            ("x.y/z~", true),
            ("x.y/x.foo", true),
            ("x.y/aux.foo", false),
            ("x.y/prn", false),
            ("x.y/prn2", true),
            ("x.y/com", true),
            ("x.y/com1", false),
            ("x.y/com1.txt", false),
            ("x.y/calm1", true),
            ("x.y/z~", true),
            ("x.y/z~0", false),
            ("x.y/z~09", false),
            ("x.y/z09", true),
            ("x.y/z09~", true),
            ("x.y/z09~09z", true),
            ("x.y/z09~09z~09", false),
            ("github.com/!123/logrus", false),
            ("github.com/user/unicode/испытание", false),
            ("../x", false),
            ("./y", false),
            ("x:y", false),
            (r"\temp\foo", false),
            (".gitignore", false),
            (".github/ISSUE_TEMPLATE", false),
            ("x☺y", false),
        ];

        for (path, ok) in GO_TABLE {
            let result = check_module_path(path);
            assert_eq!(result.is_ok(), ok, "{path:?} gave {result:?}");
        }
    }

    #[test]
    fn splits_the_major_suffix_as_go_does() {
        assert_eq!(split_path_version("x.y/z"), Some(("x.y/z", "")));
        assert_eq!(split_path_version("x.y/v2"), Some(("x.y", "/v2")));
        assert_eq!(split_path_version("x.y/v22"), Some(("x.y", "/v22")));
        assert_eq!(
            split_path_version("gopkg.in/yaml.v2"),
            Some(("gopkg.in/yaml", ".v2"))
        );
        assert_eq!(
            split_path_version("gopkg.in/check.v1"),
            Some(("gopkg.in/check", ".v1"))
        );
        assert_eq!(
            split_path_version("gopkg.in/user/pkg.v3"),
            Some(("gopkg.in/user/pkg", ".v3"))
        );
        assert_eq!(
            split_path_version("gopkg.in/pkg.v3-unstable"),
            Some(("gopkg.in/pkg", ".v3-unstable"))
        );

        assert_eq!(split_path_version("x.y/v1"), None);
        assert_eq!(split_path_version("x.y/v02"), None);
        assert_eq!(split_path_version("x.y/v2.0"), None);
        assert_eq!(
            split_path_version("gopkg.in/pkg.v0"),
            Some(("gopkg.in/pkg", ".v0"))
        );
        assert_eq!(split_path_version("gopkg.in/pkg.v01"), None);
        assert!(check_module_path("gopkg.in/pkg.v0").is_ok());
        assert!(check_module_path("gopkg.in/user/pkg.v0").is_ok());
    }
}
