use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub goos: &'static str,
    pub goarch: &'static str,
}

impl Target {
    pub const fn new(goos: &'static str, goarch: &'static str) -> Self {
        Self { goos, goarch }
    }

    pub fn host() -> Self {
        let goos = match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        };
        let goarch = match std::env::consts::ARCH {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            "x86" => "386",
            other => other,
        };
        Self { goos, goarch }
    }
}

impl Default for Target {
    fn default() -> Self {
        Self::host()
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.goos, self.goarch)
    }
}

/// Format a `(goos, goarch)` slice as a comma-separated `goos/goarch` list,
/// for "Available on: ..." diagnostics.
pub fn format_targets(targets: &[(&str, &str)]) -> String {
    targets
        .iter()
        .map(|(goos, goarch)| format!("{}/{}", goos, goarch))
        .collect::<Vec<_>>()
        .join(", ")
}
