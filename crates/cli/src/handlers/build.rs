use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use crate::cli_error;
use crate::go_cli;
use crate::lock::acquire_target_lock;
use crate::output::{format_elapsed, print_warning, use_color};
use crate::workspace::{GoWorkspace, WorkspaceBindgen, warm_typedefs};
use diagnostics::render::{self, Filter};
use lisette::fs::{LocalFileSystem, prune_orphan_go_files, prune_stale_root_go, relative_to_cwd};
use lisette::pipeline::{
    CompileConfig, CompileEntry, CompileInput, CompileMode, CompileScope, ProjectKind, Sources,
    TestIndex, compile,
};
use semantics::loader::{
    EXTERNAL_TESTS_DIR, ExternalTestFileIssue, ROOT_IMPORT, external_test_file_issue,
    is_production_module_file,
};
use semantics::store::ENTRY_MODULE_ID;

pub fn emit(path: Option<String>, sourcemap: bool) -> i32 {
    with_locked_project(path, |prep| {
        match build_locked(prep, BuildPurpose::Emit { sourcemap }) {
            Ok(_) => 0,
            Err(code) => code,
        }
    })
}

pub fn build(path: Option<String>, sourcemap: bool, go_flags: Vec<String>) -> i32 {
    with_locked_project(path, |prep| {
        if prep.kind == ProjectKind::Library {
            if go_flags.iter().any(|f| go_cli::is_go_output_flag(f)) {
                cli_error!(
                    "Unsupported flag",
                    "`-o` has no meaning for a library build, which produces no single artifact",
                    "Remove `-o`"
                );
                return 1;
            }
            crate::output::print_preview_notice("Library projects", true);
        }

        if let Err(code) = build_locked(prep, BuildPurpose::Emit { sourcemap }) {
            return code;
        }

        let target = stdlib::Target::host();

        if prep.kind == ProjectKind::Library {
            return build_library(prep, &go_flags, target);
        }

        let output_path =
            match link_project_binary(prep, &go_flags, target, "Failed to build project") {
                Ok(p) => p,
                Err(code) => return code,
            };

        let user_chose_output = go_flags.iter().any(|f| go_cli::is_go_output_flag(f));
        if user_chose_output {
            eprintln!("  ✓ Binary built");
        } else {
            let shown =
                relative_to_cwd(&output_path).unwrap_or_else(|| output_path.display().to_string());
            if use_color() {
                use owo_colors::OwoColorize;
                eprintln!("  ✓ Binary at {}", shown.bright_magenta());
            } else {
                eprintln!("  ✓ Binary at `{}`", shown);
            }
        }

        0
    })
}

fn build_library(project: &LockedProject, go_flags: &[String], target: stdlib::Target) -> i32 {
    let prep = &project.prep;
    if let Err(e) = go_cli::verify_go_packages(&prep.target_dir, target, go_flags) {
        cli_error!("Failed to build library", e.message, e.hint);
        return 1;
    }

    let name = &prep.manifest.project.name;
    if use_color() {
        use owo_colors::OwoColorize;
        eprintln!(
            "  ✓ Library {} at {}",
            name.bright_magenta(),
            "target/".bright_magenta()
        );
    } else {
        eprintln!("  ✓ Library `{}` at `target/`", name);
    }

    if !deps::is_third_party(name) {
        print_warning(&format!(
            "`{}` is not fetchable from other machines. Use a full module path in `lisette.toml`, like `github.com/you/{}`",
            name, name
        ));
    }
    0
}

pub(super) fn with_locked_project(
    path: Option<String>,
    f: impl FnOnce(&LockedProject) -> i32,
) -> i32 {
    let project_root = path.unwrap_or_else(|| ".".to_string());
    let project_path = Path::new(&project_root);

    let project = match LockedProject::acquire(project_path) {
        Ok(project) => project,
        Err(code) => return code,
    };

    f(&project)
}

pub(super) fn link_project_binary(
    project: &LockedProject,
    go_flags: &[String],
    target: stdlib::Target,
    heading: &str,
) -> Result<PathBuf, i32> {
    let prep = &project.prep;
    let build_dir = match prep.target_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to resolve `{}`: {}", prep.target_dir.display(), e),
                "Check that the directory exists"
            );
            return Err(1);
        }
    };

    let binary_name = go_cli::binary_name(&prep.manifest.project.name, target);
    let output_path = build_dir.join(".lisette").join("bin").join(&binary_name);

    if let Err(e) = go_cli::build_binary(&build_dir, &output_path, target, go_flags) {
        cli_error!(heading, e.message, e.hint);
        return Err(1);
    }

    Ok(output_path)
}

fn prepare_project_build(project_path: &Path) -> Result<BuildPrep, i32> {
    let layout = match validate_project(project_path) {
        Some(layout) => layout,
        None => return Err(1),
    };

    let (manifest, locator) = match deps::TypedefLocator::from_project_with_manifest(project_path) {
        Ok(pair) => pair,
        Err(msg) => {
            cli_error!(
                "Failed to compile Lisette project to Go",
                msg,
                "Run `lis new <name>` to create a project, or fix `lisette.toml`"
            );
            return Err(1);
        }
    };

    let target_dir = project_path.join("target");
    if let Err(e) = fs::create_dir_all(&target_dir) {
        cli_error!(
            "Failed to compile Lisette project to Go",
            format!("Failed to create `target/` directory: {}", e),
            "Check directory permissions"
        );
        return Err(1);
    }

    Ok(BuildPrep {
        project_path: project_path.to_path_buf(),
        target_dir,
        manifest,
        locator,
        kind: layout.kind,
        sources: layout.sources,
        test_sources: layout.test_sources,
    })
}

pub(super) struct BuildPrep {
    pub project_path: PathBuf,
    pub target_dir: PathBuf,
    pub manifest: deps::Manifest,
    pub locator: deps::TypedefLocator,
    pub kind: ProjectKind,
    pub sources: Vec<PathBuf>,
    pub test_sources: Vec<PathBuf>,
}

pub(super) struct LockedProject {
    prep: BuildPrep,
    _target_lock: fs::File,
}

impl LockedProject {
    pub(super) fn acquire(project_path: &Path) -> Result<Self, i32> {
        let prep = prepare_project_build(project_path)?;
        let target_lock = acquire_target_lock(&prep.target_dir)?;
        Ok(Self {
            prep,
            _target_lock: target_lock,
        })
    }
}

impl std::ops::Deref for LockedProject {
    type Target = BuildPrep;

    fn deref(&self) -> &Self::Target {
        &self.prep
    }
}

pub(super) enum BuildPurpose {
    Emit { sourcemap: bool },
    Run { sourcemap: bool },
    Test,
}

impl BuildPurpose {
    fn compile_mode(&self) -> CompileMode {
        match self {
            Self::Emit { sourcemap } | Self::Run { sourcemap } => CompileMode::Emit {
                sourcemap: *sourcemap,
            },
            Self::Test => CompileMode::Test,
        }
    }

    fn completion_label(&self) -> Option<&'static str> {
        match self {
            Self::Emit { .. } => Some("Emit completed"),
            Self::Run { .. } => None,
            Self::Test => Some("Compiled"),
        }
    }
}

pub(super) struct BuildArtifacts {
    pub test_index: TestIndex,
    pub sources: Sources,
}

fn remove_stale_test_outputs(
    target_dir: &Path,
    manifest: &mut Vec<go_cli::ManifestEntry>,
) -> std::io::Result<()> {
    for entry in manifest.iter() {
        if entry.name.ends_with("_test.go") {
            match fs::remove_file(target_dir.join(&entry.name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    manifest.retain(|entry| !entry.name.ends_with("_test.go"));
    Ok(())
}

pub(super) fn build_locked(
    project: &LockedProject,
    purpose: BuildPurpose,
) -> Result<BuildArtifacts, i32> {
    let prep = &project.prep;
    let mode = purpose.compile_mode();
    let sourcemap = matches!(mode, CompileMode::Emit { sourcemap: true });
    let emit_tests = mode == CompileMode::Test;
    let start = Instant::now();

    reject_library_replace(prep, emit_tests)?;
    write_initial_go_mod(prep)?;
    let locator = workspace_locator(prep);
    let entry = EntryPoint::resolve(prep)?;

    let result = compile_project(prep, mode, &entry, &locator);
    let counts = render_diagnostics(&result, &entry);
    if counts.errors > 0 {
        return Err(1);
    }

    let mut emit = write_and_prune_outputs(prep, &result, sourcemap, emit_tests)?;
    reconcile_target_manifest(prep, &locator, &mut emit)?;

    if !sourcemap {
        commit_emit_stamps(prep, &result);
    }
    // Committed only after gofmt + tidy succeed.
    go_cli::write_emit_manifest(&prep.target_dir, &emit.new_manifest);

    if let Some(label) = purpose.completion_label() {
        print_completion(label, prep, &counts, start);
    }

    Ok(BuildArtifacts {
        test_index: result.test_index,
        sources: result.sources,
    })
}

enum EntryPoint {
    Binary { source: String, display: String },
    Library,
}

impl EntryPoint {
    fn resolve(prep: &BuildPrep) -> Result<Self, i32> {
        match prep.kind {
            ProjectKind::Binary => {
                let main_lis = prep.project_path.join("src").join("main.lis");
                let source = match fs::read_to_string(&main_lis) {
                    Ok(s) => s,
                    Err(e) => {
                        cli_error!(
                            "Failed to compile Lisette project to Go",
                            format!("Failed to read `{}`: {}", main_lis.display(), e),
                            "Check file permissions"
                        );
                        return Err(1);
                    }
                };
                let display = relative_to_cwd(&main_lis).unwrap_or_else(|| "main.lis".to_string());
                Ok(Self::Binary { source, display })
            }
            ProjectKind::Library => Ok(Self::Library),
        }
    }

    fn compile_input(&self) -> CompileInput<'_> {
        match self {
            Self::Binary { source, display } => CompileInput::Binary(CompileEntry {
                source,
                filename: "main.lis",
                display_path: display,
            }),
            Self::Library => CompileInput::Library,
        }
    }

    fn source(&self) -> &str {
        match self {
            Self::Binary { source, .. } => source,
            Self::Library => "",
        }
    }

    fn display(&self) -> &str {
        match self {
            Self::Binary { display, .. } => display,
            Self::Library => "",
        }
    }
}

fn reject_library_replace(prep: &BuildPrep, emit_tests: bool) -> Result<(), i32> {
    if prep.kind != ProjectKind::Library || emit_tests {
        return Ok(());
    }
    let Some(key) = prep
        .manifest
        .go_deps()
        .iter()
        .find(|(_, dep)| matches!(dep, deps::GoDependency::Replaced { .. }))
        .map(|(key, _)| key.clone())
    else {
        return Ok(());
    };
    cli_error!(
        "Replaced dependency in a library",
        format!(
            "`{}` uses a `replace`, which Go ignores when this library is imported",
            key
        ),
        "Depend on a published version, or keep this project a binary"
    );
    Err(1)
}

fn write_initial_go_mod(prep: &BuildPrep) -> Result<(), i32> {
    if let Err(e) =
        go_cli::write_go_mod(&prep.target_dir, &prep.manifest.project.name, &prep.locator)
    {
        cli_error!(
            "Failed to compile Lisette project to Go",
            e,
            "Check file permissions on `target/go.mod`"
        );
        return Err(1);
    }
    Ok(())
}

fn workspace_locator(prep: &BuildPrep) -> deps::TypedefLocator {
    let typedef_cache_dir = deps::typedef_cache_dir(&prep.project_path);

    // Batch-warm the typedef cache so the lazy path during compile is all hits.
    {
        let workspace =
            GoWorkspace::new(&prep.target_dir, &typedef_cache_dir, prep.locator.target());
        warm_typedefs(&prep.project_path, &workspace, &prep.locator);
    }

    let bindgen = Arc::new(WorkspaceBindgen::new(
        prep.target_dir.clone(),
        typedef_cache_dir,
        prep.locator.target(),
    ));
    prep.locator.clone().with_bindgen(bindgen)
}

fn compile_project(
    prep: &BuildPrep,
    mode: CompileMode,
    entry: &EntryPoint,
    locator: &deps::TypedefLocator,
) -> lisette::pipeline::CompileResult {
    let go_module_name = &prep.manifest.project.name;
    let compile_config = CompileConfig {
        mode,
        go_module: go_module_name.to_string(),
        entry_package_name: match prep.kind {
            ProjectKind::Binary => "main".to_string(),
            ProjectKind::Library => emit::root_package_name(go_module_name),
        },
        scope: CompileScope::Project(prep.project_path.clone()),
        locator: locator.clone(),
    };

    let src_dir = prep.project_path.join("src");
    let local_fs = LocalFileSystem::with_scanned_sources(
        &src_dir,
        Some(&prep.project_path),
        prep.sources.clone(),
        prep.test_sources.clone(),
    );
    compile(entry.compile_input(), &compile_config, &local_fs)
}

fn render_diagnostics(
    result: &lisette::pipeline::CompileResult,
    entry: &EntryPoint,
) -> render::Counts {
    render::render_all(
        &result.errors,
        &result.lints,
        render::SourceCache::new(
            |file_id| {
                result
                    .sources
                    .get(&file_id)
                    .map(|info| (info.source.clone(), info.filename.clone()))
            },
            entry.source(),
            entry.display(),
        ),
        result.user_file_count,
        &Filter::All,
    )
}

fn write_and_prune_outputs(
    prep: &BuildPrep,
    result: &lisette::pipeline::CompileResult,
    sourcemap: bool,
    emit_tests: bool,
) -> Result<go_cli::EmitWriteResult, i32> {
    let heading = "Failed to compile Lisette project to Go";
    let produced: Vec<&str> = result.output.iter().map(|f| f.name.as_str()).collect();

    if sourcemap
        && let Err(e) = semantics::cache::apply_emit_stamps(
            &prep.project_path,
            &result
                .emit_stamps
                .iter()
                .map(|s| (s.clone(), None))
                .collect::<Vec<_>>(),
        )
    {
        cli_error!(
            heading,
            format!("Failed to invalidate emit stamps before sourcemap write: {e}"),
            "Check file permissions on `target/.lisette/cache/`, or delete the directory and retry"
        );
        return Err(1);
    }

    let mut emit = match go_cli::write_go_outputs(&prep.target_dir, &result.output) {
        Ok(emit) => emit,
        Err(e) => {
            cli_error!(heading, e.message, e.hint);
            return Err(1);
        }
    };

    let emitted: Vec<&str> = emit
        .new_manifest
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    if let Err(e) =
        prune_orphan_go_files(&prep.target_dir, &produced, &emitted, &result.live_modules)
    {
        cli_error!(
            heading,
            format!("Failed to prune stale Go files: {}", e),
            "Check file permissions"
        );
        return Err(1);
    }

    if prep.kind == ProjectKind::Library
        && let Err(e) = prune_stale_root_go(&prep.target_dir, &produced)
    {
        cli_error!(
            heading,
            format!("Failed to prune stale Go files: {}", e),
            "Check file permissions"
        );
        return Err(1);
    }

    if !emit_tests
        && let Err(e) = remove_stale_test_outputs(&prep.target_dir, &mut emit.new_manifest)
    {
        cli_error!(
            heading,
            format!("Failed to remove stale test file: {}", e),
            "Check file permissions"
        );
        return Err(1);
    }

    Ok(emit)
}

fn reconcile_target_manifest(
    prep: &BuildPrep,
    locator: &deps::TypedefLocator,
    emit: &mut go_cli::EmitWriteResult,
) -> Result<(), i32> {
    let heading = "Failed to compile Lisette project to Go";

    // Drop manifest entries whose files pruning removed, so the import-set hash
    // below reflects only surviving output.
    emit.new_manifest
        .retain(|entry| prep.target_dir.join(&entry.name).exists());

    // Force a maximal go.mod rewrite only if a prior tidy marker exists and the
    // external import set changed since it was written.
    let import_set_hash =
        go_cli::compute_import_set_hash(&emit.new_manifest, &prep.manifest.project.name);
    if let Some(prior) = go_cli::read_tidy_marker(&prep.target_dir)
        && prior != import_set_hash
    {
        go_cli::invalidate_go_mod_stamp(&prep.target_dir);
        if let Err(e) = go_cli::write_go_mod(&prep.target_dir, &prep.manifest.project.name, locator)
        {
            cli_error!(heading, e, "Check file permissions on `target/go.mod`");
            return Err(1);
        }
    }

    if let Err(e) = go_cli::finalize_go_dir(
        &prep.target_dir,
        locator.target(),
        &emit.changed,
        import_set_hash,
    ) {
        let message = super::reconciliation::augment_go_error_with_local_hint(
            e.message,
            &prep.project_path,
            &prep.target_dir,
            &prep.manifest,
        );
        cli_error!(heading, message, e.hint);
        return Err(1);
    }
    Ok(())
}

fn commit_emit_stamps(prep: &BuildPrep, result: &lisette::pipeline::CompileResult) {
    if let Err(e) = semantics::cache::apply_emit_stamps(
        &prep.project_path,
        &result
            .emit_stamps
            .iter()
            .map(|s| (s.clone(), Some(s.artifact_hash)))
            .collect::<Vec<_>>(),
    ) {
        eprintln!("warning: failed to write emit stamps: {e}");
    }
}

fn print_completion(label: &str, prep: &BuildPrep, counts: &render::Counts, start: Instant) {
    if counts.errors + counts.warnings + counts.info == 0 {
        eprintln!();
    }
    let go_module_name = &prep.manifest.project.name;
    let project_name = go_module_name.rsplit('/').next().unwrap_or(go_module_name);
    let version = &prep.manifest.project.version;
    if use_color() {
        use owo_colors::OwoColorize;
        eprintln!(
            "  ✓ {} {} v{} {}",
            label,
            project_name.bright_magenta(),
            version,
            format_elapsed(start.elapsed())
        );
    } else {
        eprintln!(
            "  ✓ {} `{}` v{} {}",
            label,
            project_name,
            version,
            format_elapsed(start.elapsed())
        );
    }
}

pub(super) struct ProjectLayout {
    pub kind: ProjectKind,
    pub sources: Vec<PathBuf>,
    pub test_sources: Vec<PathBuf>,
}

pub(super) fn resolve_project_layout(project_path: &Path) -> Option<ProjectLayout> {
    if project_path.join("main.lis").exists() {
        cli_error!(
            "Misplaced entrypoint",
            "Found `main.lis` in project root, expected it at `src/main.lis`",
            "Move `main.lis` to `src/main.lis`"
        );
        return None;
    }

    let src = project_path.join("src");
    let sources = lisette::fs::collect_lis_filepaths_recursive(&src);

    if let Some(rel) = sources.iter().find_map(|path| {
        path.strip_prefix(&src)
            .ok()
            .filter(|rel| rel.starts_with(ENTRY_MODULE_ID))
    }) {
        cli_error!(
            "Reserved module directory",
            format!(
                "`src/{}` sits under `src/{ENTRY_MODULE_ID}/`, which collides with the compiler's internal entry module",
                rel.display()
            ),
            "Rename the module"
        );
        return None;
    }

    if let Some((heading, reason, hint)) = go_ignored_shape(&src, "src", &sources) {
        cli_error!(heading, reason, hint);
        return None;
    }

    if let Some(rel) = sources.iter().find_map(|path| {
        path.strip_prefix(&src)
            .ok()
            .filter(|rel| rel.starts_with(EXTERNAL_TESTS_DIR))
    }) {
        cli_error!(
            "Reserved module directory",
            format!(
                "`src/{}` sits under `src/{EXTERNAL_TESTS_DIR}/`, which collides with the external test directory `{EXTERNAL_TESTS_DIR}/` at the project root",
                rel.display()
            ),
            "Rename the module"
        );
        return None;
    }

    if let Some(rel) = sources.iter().find_map(|path| {
        path.strip_prefix(&src)
            .ok()
            .filter(|rel| rel.starts_with(ROOT_IMPORT))
    }) {
        cli_error!(
            "Reserved module directory",
            format!(
                "`src/{}` sits under `src/{ROOT_IMPORT}/`, which collides with the reserved `{ROOT_IMPORT}` spelling for the library's root package",
                rel.display()
            ),
            "Rename the module"
        );
        return None;
    }

    let tests_dir = project_path.join(EXTERNAL_TESTS_DIR);
    let test_sources = lisette::fs::collect_lis_filepaths_recursive(&tests_dir);

    if let Some((rel, issue)) = test_sources.iter().find_map(|path| {
        let rel = path
            .strip_prefix(&tests_dir)
            .ok()?
            .to_string_lossy()
            .into_owned();
        external_test_file_issue(&rel).map(|issue| (rel, issue))
    }) {
        match issue {
            ExternalTestFileIssue::WrongSuffix => {
                let stem = rel.strip_suffix("_test.lis").unwrap_or(rel.as_str());
                cli_error!(
                    "Misnamed test file",
                    format!(
                        "`{EXTERNAL_TESTS_DIR}/{rel}` uses `_test.lis`, but Lisette test files end in `.test.lis`"
                    ),
                    format!("Rename the file to `{EXTERNAL_TESTS_DIR}/{stem}.test.lis`")
                );
            }
            ExternalTestFileIssue::NotATestFile => {
                cli_error!(
                    "Non-test file under `tests/`",
                    format!("`{EXTERNAL_TESTS_DIR}/{rel}` is not a `.test.lis` file"),
                    "Rename the file with a `.test.lis` suffix"
                );
            }
        }
        return None;
    }

    if let Some((heading, reason, hint)) =
        go_ignored_shape(&tests_dir, EXTERNAL_TESTS_DIR, &test_sources)
    {
        cli_error!(heading, reason, hint);
        return None;
    }

    if src.join("main.lis").exists() {
        return Some(ProjectLayout {
            kind: ProjectKind::Binary,
            sources,
            test_sources,
        });
    }

    let has_production = sources.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(is_production_module_file)
    });

    if !has_production {
        cli_error!(
            "No Lisette sources",
            format!(
                "No production `.lis` files under `src/` in `{}`",
                project_path.display()
            ),
            "Create `src/main.lis` for a binary, or any `src/<name>.lis` module for a library"
        );
        return None;
    }

    Some(ProjectLayout {
        kind: ProjectKind::Library,
        sources,
        test_sources,
    })
}

const GO_OS_NAMES: &[&str] = &[
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

const GO_ARCH_NAMES: &[&str] = &[
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

fn go_platform_suffix(go_filename: &str) -> Option<String> {
    let name = go_filename.split('.').next()?;
    let first_underscore = name.find('_')?;
    let mut parts: Vec<&str> = name[first_underscore..].split('_').collect();
    if parts.last() == Some(&"test") {
        parts.pop();
    }

    let last = *parts.last()?;
    if let Some(&preceding) = parts.iter().nth_back(1)
        && GO_OS_NAMES.contains(&preceding)
        && GO_ARCH_NAMES.contains(&last)
    {
        return Some(format!("{preceding}_{last}"));
    }
    if GO_OS_NAMES.contains(&last) || GO_ARCH_NAMES.contains(&last) {
        return Some(last.to_string());
    }
    None
}

fn go_ignored_shape(
    root: &Path,
    root_label: &str,
    sources: &[PathBuf],
) -> Option<(&'static str, String, String)> {
    for path in sources {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".d.lis") {
            continue;
        }

        if let Some(parent) = rel.parent() {
            for segment in parent.components().filter_map(|c| c.as_os_str().to_str()) {
                if matches!(segment, "testdata" | "vendor")
                    || segment.starts_with('.')
                    || segment.starts_with('_')
                {
                    return Some((
                        "Go-ignored module directory",
                        format!(
                            "`{root_label}/{}` sits under `{}`, which the Go toolchain skips",
                            rel.display(),
                            segment
                        ),
                        "Rename it: Go skips `testdata`, `vendor`, and directories beginning with `.` or `_`".to_string(),
                    ));
                }
            }
        }

        let go_filename = match name.strip_suffix(".test.lis") {
            Some(stem) => format!("{stem}_test.go"),
            None => match name.strip_suffix(".lis") {
                Some(stem) => format!("{stem}.go"),
                None => continue,
            },
        };

        if go_filename.starts_with('.') || go_filename.starts_with('_') {
            return Some((
                "Go-ignored source file",
                format!(
                    "`{root_label}/{}` compiles to `{}`, which the Go toolchain skips",
                    rel.display(),
                    go_filename
                ),
                "Rename it: Go ignores filenames beginning with `.` or `_`".to_string(),
            ));
        }

        if let Some(suffix) = go_platform_suffix(&go_filename) {
            return Some((
                "Platform-suffixed source file",
                format!(
                    "`{root_label}/{}` compiles to `{}`, which Go builds only on `{}`",
                    rel.display(),
                    go_filename,
                    suffix.replace('_', "/")
                ),
                format!("Rename the file to drop the trailing `_{suffix}`"),
            ));
        }
    }
    None
}

fn validate_project(project_path: &Path) -> Option<ProjectLayout> {
    if !project_path.exists() {
        cli_error!(
            "Project not found",
            format!("Path `{}` does not exist", project_path.display()),
            "Check the path and try again"
        );
        return None;
    }

    if project_path.is_file() {
        cli_error!(
            "Not a project directory",
            format!(
                "Path `{}` is a file, not a project directory",
                project_path.display()
            ),
            "`lis build <path/to/dir>` to build a project, or use `lis run <path/to/file>` to run a single file standalone"
        );
        return None;
    }

    resolve_project_layout(project_path)
}
