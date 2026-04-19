use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use deps::{GoModule, GoPackage};
use syntax::ast::Expression;
use syntax::parse::Parser;

const BINDGEN_GO_MODULE: &str = "github.com/ivov/lisette/bindgen";
const BINDGEN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Information about a Go module from `go list -m -json`.
pub struct GoModuleInfo {
    pub path: String,
    pub version: String,
}

/// A directory with a `go.mod` that `go` commands run against.
pub struct GoWorkspace<'a> {
    /// The dir with the `go.mod` that `go` commands run against.
    root: &'a Path,
    /// The typedef cache root, e.g. `~/.lisette/cache/typedefs/lis@v0.1.7`.
    pub typedef_cache_dir: &'a Path,
}

impl<'a> GoWorkspace<'a> {
    pub fn new(root: &'a Path, typedef_cache_dir: &'a Path) -> Self {
        Self {
            root,
            typedef_cache_dir,
        }
    }

    /// Run a `go` subcommand and return its stdout on success.
    fn run_go(&self, args: &[&str]) -> Result<String, String> {
        let cmd_display = format!("go {}", args.join(" "));
        let output = crate::go_cli::go_command()
            .args(args)
            .current_dir(self.root)
            .output()
            .map_err(|e| format!("Failed to run `{}`: {}", cmd_display, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(translate_go_error(args, stderr.trim()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Download a Go module. Runs `go get {module}@{version}`.
    pub fn go_get(&self, module: GoModule) -> Result<(), String> {
        let target = format!("{}@{}", module.path, module.version);
        self.run_go(&["get", &target])?;
        Ok(())
    }

    /// Query a Go module's current graph version.
    pub fn query_version(&self, module: &str) -> Result<String, String> {
        let info = self.query_module(module)?;
        if info.version.is_empty() {
            return Err(format!("`go list -m -json {}` returned no version", module));
        }
        Ok(info.version)
    }

    /// Query a Go module's path, version, and local directory.
    ///
    /// ```text
    /// query_module("github.com/gorilla/mux")          // version from go.mod
    /// query_module("github.com/gorilla/mux@v1.8.0")   // specific version
    /// ```
    pub fn query_module(&self, query: &str) -> Result<GoModuleInfo, String> {
        let stdout = self.run_go(&["list", "-m", "-json", query])?;
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse Go module JSON: {}", e))?;

        Ok(GoModuleInfo {
            path: value["Path"].as_str().unwrap_or("").to_string(),
            version: value["Version"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Resolve a module's `@latest` alias to a concrete version.
    ///
    /// Uses `-mod=mod` so Go is allowed to refresh `go.sum` if the proxy
    /// returns a version that matches the existing pin (a plain readonly
    /// `go list -m -json X@latest` errors with `updates to go.sum needed`
    /// in that case).
    pub fn query_latest_version(&self, module_path: &str) -> Result<String, String> {
        let target = format!("{}@latest", module_path);
        let stdout = self.run_go(&["list", "-mod=mod", "-m", "-json", &target])?;
        let value: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse Go module JSON: {}", e))?;
        let version = value["Version"].as_str().unwrap_or("").to_string();
        if version.is_empty() {
            return Err(format!("`go list -m -json {}` returned no version", target));
        }
        Ok(version)
    }

    /// List all public packages in a Go module.
    ///
    /// Uses `-mod=mod` so the BFS reconcile can add newly-discovered transitives
    /// to `target/go.mod` while resolving the package list. Without it, deep
    /// graphs (otel, gRPC) hit `updates to go.mod needed; to update it: go mod
    /// tidy` mid-walk and abort the whole add.
    pub fn list_packages(&self, module_path: &str) -> Result<Vec<String>, String> {
        let pattern = format!("{}/...", module_path);
        let stdout = self.run_go(&["list", "-mod=mod", "-e", &pattern])?;
        let packages: Vec<String> = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .filter(|l| {
                let relative = l.strip_prefix(module_path).unwrap_or(l);
                !relative.split('/').any(|segment| segment == "internal")
            })
            .map(|l| l.to_string())
            .collect();

        Ok(packages)
    }

    /// Find the Go module that contains a package path.
    ///
    /// Queries `go list -m -json` with progressively shorter path prefixes
    /// until a module is found:
    ///
    /// ```text
    /// github.com/gorilla/mux/middleware → github.com/gorilla/mux
    /// github.com/gorilla/mux            → github.com/gorilla/mux
    /// ```
    ///
    /// Requires the module to be in the build graph (direct or indirect).
    pub fn find_containing_module(&self, pkg_path: &str) -> Result<GoModuleInfo, String> {
        if let Ok(info) = self.query_module(pkg_path)
            && !info.path.is_empty()
        {
            return Ok(info);
        }

        let mut path = pkg_path;
        while let Some(pos) = path.rfind('/') {
            path = &path[..pos];
            if let Ok(info) = self.query_module(path)
                && !info.path.is_empty()
            {
                return Ok(info);
            }
        }

        Err(format!(
            "Could not find containing module for package `{}`",
            pkg_path
        ))
    }

    /// Run bindgen on a Go package and return the generated typedef content.
    ///
    /// - For local dev, runs: `bindgen/bin/bindgen pkg {package}`
    /// - For end users, runs: `go run github.com/ivov/lisette/bindgen@v{version} pkg {package}`
    pub fn run_bindgen(&self, package: &str) -> Result<String, String> {
        let mut cmd = if let Some(bin) = dev_bindgen_path() {
            let mut c = Command::new(bin);
            c.args(["pkg", package]);
            c
        } else {
            let bindgen_at_version = format!("{}@v{}", BINDGEN_GO_MODULE, BINDGEN_VERSION);
            let mut c = crate::go_cli::go_command();
            c.args(["run", &bindgen_at_version, "pkg", package]);
            c
        };

        let result = cmd
            .current_dir(self.root)
            .output()
            .map_err(|e| format!("Failed to run bindgen for `{}`: {}", package, e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!(
                "Bindgen failed for `{}`: {}",
                package,
                stderr.trim()
            ));
        }

        String::from_utf8(result.stdout)
            .map_err(|e| format!("Bindgen produced invalid UTF-8 for `{}`: {}", package, e))
    }

    /// Ensure typedefs exist in cache for every public package in a Go module.
    ///
    /// Returns the list of packages in the module.
    pub fn reconcile(&self, module: GoModule) -> Result<Vec<String>, String> {
        self.go_get(module)?;

        let packages = self.list_packages(module.path)?;

        for pkg_path in &packages {
            let pkg = GoPackage {
                module,
                package: pkg_path,
            };
            let pkg_typedef_path = pkg.typedef_path(self.typedef_cache_dir);

            if pkg_typedef_path.exists() {
                continue;
            }

            let typedef = self.run_bindgen(pkg_path)?;

            validate_typedef_parses(pkg_path, &typedef)?;

            if let Some(parent_dir) = pkg_typedef_path.parent() {
                fs::create_dir_all(parent_dir)
                    .map_err(|e| format!("Failed to create cache directory: {}", e))?;
            }

            fs::write(&pkg_typedef_path, &typedef)
                .map_err(|e| format!("Failed to cache typedef for `{}`: {}", pkg_path, e))?;
        }

        Ok(packages)
    }

    /// Find the third-party Go modules this module's typedefs depend on.
    pub fn find_third_party_deps(
        &self,
        module: GoModule,
        module_packages: &[String],
    ) -> Result<Vec<String>, String> {
        let mut third_party_deps = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for pkg_path in module_packages {
            let pkg = GoPackage {
                module,
                package: pkg_path,
            };
            let pkg_typedef_path = pkg.typedef_path(self.typedef_cache_dir);

            let typedef = fs::read_to_string(&pkg_typedef_path)
                .map_err(|e| format!("Failed to read cached typedef for `{}`: {}", pkg_path, e))?;

            for import_path in extract_third_party_imports(&typedef) {
                let containing = self.find_containing_module(&import_path).map_err(|e| {
                    format!(
                        "Failed to resolve transitive import `{}` from `{}`: {}",
                        import_path, pkg_path, e
                    )
                })?;

                if containing.path == module.path || seen.contains(&containing.path) {
                    continue;
                }

                seen.insert(containing.path.clone());
                third_party_deps.push(containing.path);
            }
        }

        Ok(third_party_deps)
    }
}

/// Translate raw `go` stderr into a one-line message for the common failure modes.
///
/// Falls back to the trimmed stderr verbatim if no pattern matches, so callers
/// never lose information.
fn translate_go_error(args: &[&str], stderr: &str) -> String {
    let target = args
        .iter()
        .find(|a| {
            !a.starts_with('-')
                && **a != "get"
                && **a != "list"
                && **a != "-m"
                && **a != "-json"
                && **a != "-e"
        })
        .copied()
        .unwrap_or("");
    let module = target.rsplit_once('@').map(|(m, _)| m).unwrap_or(target);

    if stderr.contains("unknown revision") {
        return format!("Version not found for `{}`", target);
    }
    if stderr.contains("Repository not found") || stderr.contains("repository not found") {
        return format!("Module `{}` not found", module);
    }
    if stderr.contains("no matching versions for query") {
        return format!("No matching versions found for `{}`", target);
    }
    if let Some(corrected) = extract_post_v_path(stderr) {
        return format!(
            "`{}` is a v2+ Go module and requires the major-version suffix `{}` (try `{}@<version>`)",
            module, corrected, corrected
        );
    }
    if stderr.contains("module declares its path as") {
        if let Some((declared, required)) = extract_path_mismatch(stderr) {
            return format!(
                "Module path mismatch: `{}` was required, but the upstream module declares its path as `{}` (try `{}` instead). If `{}` is in your `lisette.toml`, fix it there.",
                required, declared, declared, required
            );
        }
        return format!(
            "Module path mismatch: `{}` does not match the module's declared path",
            module
        );
    }
    if stderr.contains("malformed module path") {
        return format!(
            "`{}` is not a valid module path. If this is a Go package import path, use the module root instead (e.g. `k8s.io/api`, not `k8s.io/api/core/v1`)",
            module
        );
    }
    if stderr.contains("errors parsing go.mod") {
        if let Some(culprit) = extract_invalid_pin(stderr) {
            return format!(
                "`lisette.toml` has an invalid Go version for `{}` (`{}`); fix the pin and retry",
                culprit.0, culprit.1
            );
        }
        return "`lisette.toml` contains an invalid Go version; fix the offending pin and retry"
            .to_string();
    }
    // Must precede the generic `invalid version` branch below; Go's
    // `invalid version control suffix` error string contains `invalid version`
    // as a substring and would otherwise hit the wrong branch.
    if stderr.contains("invalid version control suffix") {
        return format!(
            "`{}` is not a valid Go module path (do not include `.git` or other VCS suffixes)",
            module
        );
    }

    let target_version_error =
        !target.is_empty() && stderr.contains(&format!("{}: invalid version", target));

    if target_version_error {
        return format!(
            "Invalid Go module version in `{}` (must look like `v1.2.3`)",
            target
        );
    }
    if stderr.contains("invalid github.com import path") {
        if let Some(rest) = module.strip_prefix("github.com/")
            && !rest.contains('/')
        {
            return format!(
                "`{}` is missing the repository segment; try `github.com/{}/<repo>`",
                module, rest
            );
        }
        return format!(
            "`{}` is not a valid github.com import path (github only allows letters, digits, and `.-_`)",
            module
        );
    }
    if let Some((found, missing)) = extract_missing_subpackage(stderr) {
        return format!(
            "Module `{}` exists but does not contain package `{}`; v1 Go modules do not use a `/v1` suffix (only v2+ require the major-version suffix)",
            found, missing
        );
    }
    if stderr.contains("no required module provides package")
        || stderr.contains("cannot find module providing package")
    {
        return format!("No module provides package `{}`", module);
    }
    if stderr.contains("existing contents have changed since last read") {
        return "Another `lis add` is in progress against this project; wait for it to finish and retry".to_string();
    }
    if stderr.contains("unable to access") || stderr.contains("requested URL returned error: 4") {
        return format!(
            "Module `{}` is unreachable (the host returned an error)",
            module
        );
    }
    if stderr.contains("module lookup disabled by GOPROXY") {
        return format!(
            "Module `{}` is not in the local cache and `GOPROXY=off` disables remote lookups; unset `GOPROXY` or set it to a working proxy",
            module
        );
    }
    if stderr.contains("modules disabled by GO111MODULE") {
        return "`GO111MODULE=off` disables Go modules entirely; unset `GO111MODULE` (Go modules are required by lisette)".to_string();
    }
    if stderr.contains("-insecure flag is no longer supported") {
        return "`-insecure` is no longer a valid Go flag; remove it from `GOFLAGS` or set `GOINSECURE` instead".to_string();
    }
    if stderr.contains("unrecognized import path") {
        return format!(
            "`{}` is not a recognized Go module path; the host does not serve `go-import` metadata",
            module
        );
    }
    if stderr.contains("updates to go.mod needed") {
        return format!(
            "Resolving `{}` requires updates to `target/go.mod` that lisette could not perform; please file an issue",
            target
        );
    }

    let cmd_display = format!("go {}", args.join(" "));
    format!("`{}` failed: {}", cmd_display, stderr)
}

/// Pull `(module_path, version)` out of a `go.mod` parse error like
/// `require github.com/gorilla/mux: version "v999.999.999" invalid: ...`.
fn extract_invalid_pin(stderr: &str) -> Option<(String, String)> {
    let line = stderr
        .lines()
        .find(|l| l.contains("require ") && l.contains("version "))?;
    let after_require = line.split("require ").nth(1)?;
    let module = after_require.split(':').next()?.trim().to_string();
    let after_version = line.split("version \"").nth(1)?;
    let version = after_version.split('"').next()?.to_string();
    Some((module, version))
}

/// Pull the corrected module path out of a `go.mod has post-vN module path
/// "github.com/foo/bar/vN" at revision vN.x.y` error.
fn extract_post_v_path(stderr: &str) -> Option<String> {
    let after = stderr.split("post-v").nth(1)?;
    let after_quote = after.split("module path \"").nth(1)?;
    let path = after_quote.split('"').next()?;
    Some(path.to_string())
}

/// Pull `(declared, required)` out of a Go path-mismatch error:
///
/// ```text
/// module declares its path as: golang.org/x/example
///         but was required as: github.com/golang/example
/// ```
fn extract_path_mismatch(stderr: &str) -> Option<(String, String)> {
    let declared = stderr
        .lines()
        .find_map(|l| l.split("module declares its path as:").nth(1))?
        .trim()
        .to_string();
    let required = stderr
        .lines()
        .find_map(|l| l.split("but was required as:").nth(1))?
        .trim()
        .to_string();
    if declared.is_empty() || required.is_empty() {
        return None;
    }
    Some((declared, required))
}

/// Pull `(found_module, missing_package)` out of a Go missing-subpackage error:
///
/// ```text
/// module github.com/gorilla/mux@v1.8.0 found, but does not contain package github.com/gorilla/mux/v1
/// ```
fn extract_missing_subpackage(stderr: &str) -> Option<(String, String)> {
    let after_module = stderr.split("module ").nth(1)?;
    let found = after_module
        .split('@')
        .next()
        .or_else(|| after_module.split(' ').next())?
        .trim()
        .to_string();
    let after_pkg = stderr.split("does not contain package ").nth(1)?;
    let missing = after_pkg
        .split(|c: char| c.is_whitespace())
        .next()?
        .to_string();
    if found.is_empty() || missing.is_empty() {
        return None;
    }
    Some((found, missing))
}

fn validate_typedef_parses(pkg_path: &str, typedef: &str) -> Result<(), String> {
    let parse = Parser::lex_and_parse_file(typedef, 0);
    if !parse.failed() {
        return Ok(());
    }
    Err(format!(
        "Bindgen produced unparseable typedef for `{}`: {} parse error(s); first: {}",
        pkg_path,
        parse.errors.len(),
        parse.errors[0].message,
    ))
}

fn extract_third_party_imports(typedef: &str) -> Vec<String> {
    let parse_result = Parser::lex_and_parse_file(typedef, 0);

    parse_result
        .ast
        .iter()
        .filter_map(|expr| match expr {
            Expression::ModuleImport { name, .. } => {
                let pkg = name.strip_prefix("go:")?;
                if deps::is_third_party(pkg) {
                    Some(pkg.to_string())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect()
}

#[cfg(debug_assertions)]
fn dev_bindgen_path() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../bindgen/bin/bindgen"
    ));
    path.canonicalize().ok()
}

#[cfg(not(debug_assertions))]
fn dev_bindgen_path() -> Option<std::path::PathBuf> {
    None
}
