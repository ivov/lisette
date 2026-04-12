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
    pub dir: String,
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
        let output = Command::new("go")
            .args(args)
            .current_dir(self.root)
            .output()
            .map_err(|e| format!("Failed to run `{}`: {}", cmd_display, e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("`{}` failed: {}", cmd_display, stderr.trim()));
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
            dir: value["Dir"].as_str().unwrap_or("").to_string(),
        })
    }

    /// List all public packages in a Go module.
    pub fn list_packages(&self, module_path: &str) -> Result<Vec<String>, String> {
        let pattern = format!("{}/...", module_path);
        let stdout = self.run_go(&["list", "-e", &pattern])?;
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
    /// Requires a prior `go get` so the module is in `target/go.mod`.
    pub fn find_containing_module(&self, pkg_path: &str) -> Result<GoModuleInfo, String> {
        if let Ok(info) = self.query_module(pkg_path)
            && !info.dir.is_empty()
        {
            return Ok(info);
        }

        let mut path = pkg_path;
        while let Some(pos) = path.rfind('/') {
            path = &path[..pos];
            if let Ok(info) = self.query_module(path)
                && !info.dir.is_empty()
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
            let mut c = Command::new("go");
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
