use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use crate::cli_error;
use crate::go_cli;
use diagnostics::render::{self, Filter};
use lisette::pipeline::{
    CompileConfig, CompileEntry, CompileInput, CompileMode, CompileResult, CompileScope, compile,
};
use semantics::loader::MemoryLoader;

const GO_MODULE: &str = "lis-script";

pub(super) struct ScriptBuild {
    pub(super) dir: PathBuf,
    pub(super) target: stdlib::Target,
    diagnostics_shown: bool,
}

pub(super) fn prepare(
    file: &Path,
    sourcemap: bool,
    inside_project: bool,
    heading: &str,
) -> Result<ScriptBuild, i32> {
    let locator = deps::TypedefLocator::default();
    let (mut result, diagnostics_shown) =
        compile_file(file, sourcemap, inside_project, &locator, heading)?;
    let dir = build_dir(file, heading)?;
    let target = locator.target();

    for file in &mut result.output {
        if let Some(staged) = stage_name(&file.name) {
            file.name = staged;
        }
    }

    if let Err(e) = go_cli::write_go_mod(&dir, GO_MODULE, &locator) {
        cli_error!(heading, e, "Check file permissions");
        return Err(1);
    }

    let emit = match go_cli::write_go_outputs(&dir, &result.output) {
        Ok(emit) => emit,
        Err(e) => {
            cli_error!(heading, e.message, e.hint);
            return Err(1);
        }
    };

    let emitted: Vec<&str> = result
        .output
        .iter()
        .map(|file| file.name.as_str())
        .collect();
    prune_stale_go(&dir, &emitted);

    let mut manifest = emit.new_manifest;
    manifest.retain(|entry| emitted.contains(&entry.name.as_str()));

    let import_set_hash = go_cli::compute_import_set_hash(&manifest, GO_MODULE);
    if let Err(e) = go_cli::finalize_go_dir(&dir, target, &emit.changed, import_set_hash) {
        cli_error!(heading, e.message, e.hint);
        return Err(1);
    }

    go_cli::write_emit_manifest(&dir, &manifest);

    Ok(ScriptBuild {
        dir,
        target,
        diagnostics_shown,
    })
}

pub(super) fn emit(
    file: &Path,
    sourcemap: bool,
    output: Option<&str>,
    inside_project: bool,
) -> i32 {
    let heading = "Failed to emit script";
    let requested = match output {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("{}.go", stem(file))),
    };
    let destination = match check_destination(file, &requested, heading) {
        Ok(destination) => destination,
        Err(code) => return code,
    };

    let locator = deps::TypedefLocator::default();
    let Ok((result, diagnostics_shown)) =
        compile_file(file, sourcemap, inside_project, &locator, heading)
    else {
        return 1;
    };

    let [go_file] = result.output.as_slice() else {
        cli_error!(
            heading,
            format!("A script emits one Go file, got {}", result.output.len()),
            "Report this as a bug"
        );
        return 1;
    };

    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty())
        && let Err(e) = fs::create_dir_all(parent)
    {
        cli_error!(
            heading,
            format!("Failed to create `{}`: {}", parent.display(), e),
            "Check directory permissions"
        );
        return 1;
    }

    if let Err(e) = fs::write(&destination, go_file.to_go()) {
        cli_error!(
            heading,
            format!("Failed to write `{}`: {}", destination.display(), e),
            "Check directory permissions"
        );
        return 1;
    }

    report_written("Go file", &destination, diagnostics_shown);
    0
}

pub(super) fn build(
    file: &Path,
    sourcemap: bool,
    go_flags: &[String],
    output: Option<&str>,
    inside_project: bool,
) -> i32 {
    let heading = "Failed to build script";
    if let Some(flag) = go_flags.iter().find(|flag| go_cli::is_go_output_flag(flag)) {
        cli_error!(
            "Unsupported flag",
            format!(
                "`{}` cannot be passed to a script build via `--go-flags`",
                flag
            ),
            "Use `lis build <file> -o <path>` to choose where the binary lands"
        );
        return 1;
    }

    let target = stdlib::Target::host();
    let requested = match output {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(go_cli::binary_name(stem(file), target)),
    };
    let destination = match check_destination(file, &requested, heading) {
        Ok(destination) => destination,
        Err(code) => return code,
    };

    let build = match prepare(file, sourcemap, inside_project, heading) {
        Ok(build) => build,
        Err(code) => return code,
    };

    if let Err(e) = go_cli::build_binary(&build.dir, &destination, build.target, go_flags) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }

    report_written("Binary", &destination, build.diagnostics_shown);
    0
}

fn compile_file(
    file: &Path,
    sourcemap: bool,
    inside_project: bool,
    locator: &deps::TypedefLocator,
    heading: &str,
) -> Result<(CompileResult, bool), i32> {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to read `{}`: {}", file.display(), e),
                "Check file permissions"
            );
            return Err(1);
        }
    };

    let entry_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("main.lis")
        .to_string();
    let entry_display =
        lisette::fs::relative_to_cwd(file).unwrap_or_else(|| file.display().to_string());

    let result = compile(
        CompileInput::Binary(CompileEntry {
            source: &source,
            filename: &entry_name,
            display_path: &entry_display,
        }),
        CompileConfig {
            mode: CompileMode::Emit { sourcemap },
            go_module: GO_MODULE,
            entry_package_name: "main",
            scope: CompileScope::Script { inside_project },
            locator,
        },
        &MemoryLoader::new(),
    );

    let counts = render::render_all(
        &result.diagnostics,
        render::SourceCache::new(|file_id| {
            result
                .sources
                .get(&file_id)
                .map(|info| (info.source.clone(), info.filename.clone()))
        }),
        result.user_file_count,
        &Filter::All,
    );

    if counts.errors > 0 {
        return Err(1);
    }

    Ok((result, counts.warnings + counts.info > 0))
}

fn build_dir(file: &Path, heading: &str) -> Result<PathBuf, i32> {
    let absolute = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut hasher = DefaultHasher::new();
    absolute.hash(&mut hasher);
    let dir = std::env::temp_dir().join(format!("lis-script-{:x}", hasher.finish()));

    if let Err(e) = fs::create_dir_all(&dir) {
        cli_error!(
            heading,
            format!("Failed to create temporary directory: {}", e),
            "Check permissions on temp directory"
        );
        return Err(1);
    }

    // Absolute path required: a relative `TMPDIR` would break the `-o`/exec contract.
    dir.canonicalize().map_err(|e| {
        cli_error!(
            heading,
            format!("Failed to resolve temporary directory: {}", e),
            "Check permissions on temp directory"
        );
        1
    })
}

fn absolute(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    Some(std::env::current_dir().ok()?.join(path))
}

fn prune_stale_go(dir: &Path, emitted: &[&str]) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".go") && !emitted.contains(&name) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn stage_name(name: &str) -> Option<String> {
    name.starts_with(['_', '.', '-'])
        .then(|| format!("lis{}", name))
}

fn stem(file: &Path) -> &str {
    file.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("main")
}

/// Write to the returned path: checking one path and writing another is how a
/// symlinked parent slips through.
fn check_destination(input: &Path, output: &Path, heading: &str) -> Result<PathBuf, i32> {
    if demands_a_directory(output) {
        cli_error!(
            heading,
            format!("`{}` names a directory", output.display()),
            "Drop the trailing separator or `.` to write a file"
        );
        return Err(1);
    }

    let destination = match resolve(output) {
        Ok(destination) => destination,
        Err(PathError::NotADirectory(blocker)) => {
            cli_error!(
                heading,
                format!("`{}` is not a directory", shown(&blocker)),
                "Every path component before the filename must be a directory"
            );
            return Err(1);
        }
        Err(PathError::Unresolvable) => {
            cli_error!(
                heading,
                format!("Failed to resolve `{}`", output.display()),
                "Check the output path"
            );
            return Err(1);
        }
    };

    // `go build -o <dir>` writes inside it, so the reported path would not be
    // the artifact.
    if destination.is_dir() {
        cli_error!(
            heading,
            format!("`{}` is a directory", shown(output)),
            "Pass `-o <path>` naming the file to write"
        );
        return Err(1);
    }

    let same = is_same_file_on_disk(input, &destination)
        || resolve(input).is_ok_and(|input| input == destination);
    if same {
        cli_error!(
            heading,
            format!("`{}` is the script being compiled", shown(output)),
            "Pass `-o <path>` to write somewhere else"
        );
        return Err(1);
    }

    Ok(destination)
}

/// True for a trailing separator or a terminal `.`, which `Path::components()`
/// drops, so this reads the path as written.
fn demands_a_directory(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    if text.ends_with(std::path::is_separator) {
        return true;
    }

    text.strip_suffix('.')
        .is_some_and(|rest| rest.is_empty() || rest.ends_with(std::path::is_separator))
}

enum PathError {
    Unresolvable,
    /// What the kernel answers with `ENOTDIR` rather than folding the `..` away.
    NotADirectory(PathBuf),
}

/// Each component resolves as soon as it exists, so a symlink is followed
/// before the `..` crossing it. The tail folds lexically.
fn resolve(path: &Path) -> Result<PathBuf, PathError> {
    let absolute = absolute(path).ok_or(PathError::Unresolvable)?;
    let mut components = absolute.components().peekable();
    let mut resolved = PathBuf::new();

    while let Some(component) = components.next() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            other => resolved.push(other),
        }
        if let Ok(canonical) = resolved.canonicalize() {
            resolved = canonical;
        }
        if components.peek().is_some() && resolved.exists() && !resolved.is_dir() {
            return Err(PathError::NotADirectory(resolved));
        }
    }

    Ok(resolved)
}

#[cfg(unix)]
fn is_same_file_on_disk(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(left), Ok(right)) = (left.metadata(), right.metadata()) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn is_same_file_on_disk(left: &Path, right: &Path) -> bool {
    file_identity(left).is_some_and(|left| file_identity(right) == Some(left))
}

/// Windows exposes no file index on stable, so identity comes from the handle.
#[cfg(windows)]
fn file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[derive(Clone, Copy, Default)]
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[derive(Default)]
    #[repr(C)]
    struct ByHandleFileInformation {
        attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        #[link_name = "GetFileInformationByHandle"]
        fn get_file_information_by_handle(
            file: *mut c_void,
            info: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let file = fs::File::open(path).ok()?;
    let mut info = ByHandleFileInformation::default();
    // SAFETY: the owned file handle remains live for the call, and `info`
    // exactly matches the Windows BY_HANDLE_FILE_INFORMATION layout.
    let result = unsafe { get_file_information_by_handle(file.as_raw_handle().cast(), &mut info) };
    if result == 0 {
        return None;
    }

    let index = (u64::from(info.file_index_high) << 32) | u64::from(info.file_index_low);
    Some((info.volume_serial_number, index))
}

fn shown(path: &Path) -> String {
    lisette::fs::relative_to_cwd(path).unwrap_or_else(|| path.display().to_string())
}

fn report_written(what: &str, path: &Path, diagnostics_shown: bool) {
    if !diagnostics_shown {
        eprintln!();
    }
    let path = shown(path);
    if crate::output::use_color() {
        use owo_colors::OwoColorize;
        eprintln!("  ✓ {} at {}", what, path.bright_magenta());
    } else {
        eprintln!("  ✓ {} at `{}`", what, path);
    }
}
