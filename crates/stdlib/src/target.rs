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
