use std::env::consts;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub goos: &'static str,
    pub goarch: &'static str,
}

/// The targets with stdlib typedefs, mirroring the `justfile`'s `_supported-targets`.
pub const SUPPORTED_TARGETS: &[Target] = &[
    Target::new("linux", "amd64"),
    Target::new("linux", "arm64"),
    Target::new("darwin", "amd64"),
    Target::new("darwin", "arm64"),
    Target::new("windows", "amd64"),
    Target::new("windows", "arm64"),
];

impl Target {
    pub const fn new(goos: &'static str, goarch: &'static str) -> Self {
        Self { goos, goarch }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let (goos, goarch) = text.split_once('/')?;
        SUPPORTED_TARGETS
            .iter()
            .copied()
            .find(|target| target.goos == goos && target.goarch == goarch)
    }

    pub fn cache_segment(self) -> String {
        format!("{}_{}", self.goos, self.goarch)
    }

    pub fn is_host(self) -> bool {
        self == Self::host()
    }

    pub fn host() -> Self {
        let goos = match consts::OS {
            "macos" => "darwin",
            other => other,
        };
        let goarch = match consts::ARCH {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_go_stdlib_typedef;

    #[test]
    fn every_supported_target_has_an_overlay() {
        for target in SUPPORTED_TARGETS {
            assert!(
                get_go_stdlib_typedef("os", *target).is_some(),
                "`{target}` has no typedef overlay"
            );
        }
    }

    #[test]
    fn parse_accepts_only_supported_targets() {
        assert_eq!(Target::parse("linux/amd64"), Some(SUPPORTED_TARGETS[0]));
        assert_eq!(Target::parse("freebsd/amd64"), None);
        assert_eq!(Target::parse("linux"), None);
        assert_eq!(Target::parse("linux/amd64/extra"), None);
        assert_eq!(Target::parse(""), None);
    }
}
