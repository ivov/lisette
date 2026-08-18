use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(feature = "self-upgrade")]
use std::process;

use serde::Deserialize;

use crate::cli_error;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const INSTALLER_URL: &str =
    "https://github.com/ivov/lisette/releases/latest/download/lisette-installer.sh";
const WINDOWS_INSTALLER_URL: &str =
    "https://github.com/ivov/lisette/releases/latest/download/lisette-installer.ps1";

/// The other layout, `flat`, comes from `LISETTE_UNMANAGED_INSTALL`, which writes no record.
const CARGO_HOME_LAYOUT: &str = "cargo-home";

const RECORD_FILE: &str = "lisette-receipt.json";

const NOT_INSTALLER_MANAGED: &str = "Not installed by the Lisette installer";

#[derive(Debug, Deserialize)]
struct RecordFile {
    install_layout: String,
    install_prefix: String,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallRecord {
    layout: String,
    install_prefix: PathBuf,
    canonical_executable: PathBuf,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecordError {
    Missing,
    Unreadable,
}

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Brew,
    Mise,
    NoRecord { under_cargo_home: bool },
    UnreadableRecord,
    WrongPath { recorded: PathBuf },
    WrongVersion { recorded: String },
    WindowsCommand { install_prefix: PathBuf },
    RunInstaller { install_prefix: PathBuf },
    Packager,
}

struct Install {
    executable: PathBuf,
    record: Result<InstallRecord, RecordError>,
    cargo_bin: Option<PathBuf>,
    windows: bool,
    self_upgrade: bool,
    version: String,
}

pub fn upgrade() -> i32 {
    let executable = match env::current_exe() {
        Ok(path) => fs::canonicalize(&path).unwrap_or(path),
        Err(error) => {
            cli_error!(
                "Cannot locate executable",
                format!("`current_exe` failed: {}", error),
                "Upgrade with the tool that installed this copy"
            );
            return 1;
        }
    };

    let record_path = record_path();
    let record = match &record_path {
        Some(path) => load_record(path, cfg!(windows)),
        None => Err(RecordError::Missing),
    };

    let install = Install {
        executable,
        record,
        cargo_bin: cargo_bin_directory(
            non_empty_var("CARGO_HOME").map(PathBuf::from).as_deref(),
            home_directory().as_deref(),
        )
        .and_then(|path| fs::canonicalize(path).ok()),
        windows: cfg!(windows),
        self_upgrade: cfg!(feature = "self-upgrade"),
        version: VERSION.to_string(),
    };

    let action = decide(&install);
    report(&install, record_path.as_deref(), action)
}

fn decide(install: &Install) -> Action {
    if has_adjacent_components(&install.executable, "Cellar", "lisette") {
        return Action::Brew;
    }
    if has_adjacent_components(&install.executable, "installs", "lisette") {
        return Action::Mise;
    }

    let record = match &install.record {
        Ok(record) => record,
        Err(RecordError::Missing) => {
            return Action::NoRecord {
                under_cargo_home: match (&install.cargo_bin, install.executable.parent()) {
                    (Some(cargo_bin), Some(parent)) => cargo_bin == parent,
                    _ => false,
                },
            };
        }
        Err(RecordError::Unreadable) => return Action::UnreadableRecord,
    };

    if record.layout != CARGO_HOME_LAYOUT || record.canonical_executable != install.executable {
        return Action::WrongPath {
            recorded: recorded_executable(&record.install_prefix, install.windows),
        };
    }

    if record.version != install.version {
        return Action::WrongVersion {
            recorded: record.version.clone(),
        };
    }

    let install_prefix = record.install_prefix.clone();
    if install.windows {
        return Action::WindowsCommand { install_prefix };
    }
    if install.self_upgrade {
        return Action::RunInstaller { install_prefix };
    }
    Action::Packager
}

fn report(install: &Install, record_path: Option<&Path>, action: Action) -> i32 {
    match action {
        Action::Brew => {
            cli_error!(
                NOT_INSTALLER_MANAGED,
                "You installed Lisette using Homebrew",
                "To upgrade, run `brew update && brew upgrade lisette`"
            );
            1
        }
        Action::Mise => {
            cli_error!(
                NOT_INSTALLER_MANAGED,
                "You installed Lisette using mise",
                "To upgrade, run `mise upgrade lisette`"
            );
            1
        }
        Action::NoRecord { under_cargo_home } => {
            let reason = if under_cargo_home {
                "No install record was found, and this copy sits where both `cargo install` and the Lisette installer write"
            } else {
                "No install record was found for this copy"
            };
            cli_error!(
                NOT_INSTALLER_MANAGED,
                reason,
                missing_record_hint(install.windows, under_cargo_home)
            );
            1
        }
        Action::UnreadableRecord => {
            let path = record_path.map_or_else(
                || "The install record".to_string(),
                |path| path.display().to_string(),
            );
            cli_error!(
                "Unreadable install record",
                format!(
                    "{} could not be opened or parsed, so how this copy was installed is unknown",
                    path
                ),
                "Reinstall Lisette using the original installation method"
            );
            1
        }
        Action::WrongPath { recorded } => {
            cli_error!(
                "Multiple installs found",
                format!(
                    "The running executable is at {} but the installer recorded {}",
                    install.executable.display(),
                    recorded.display()
                ),
                "Upgrade the copy you intend to use"
            );
            1
        }
        Action::WrongVersion { recorded } => {
            cli_error!(
                "Replaced executable",
                format!(
                    "The installer recorded version {} but this binary is {}, so something else replaced it",
                    recorded, install.version
                ),
                "To upgrade, use the tool that installed it"
            );
            1
        }
        Action::WindowsCommand { install_prefix } => {
            cli_error!(
                "Cannot upgrade on Windows",
                "Windows locks the file of a program while that program runs, so the installer cannot copy over it",
                format!(
                    "To upgrade, run:\n    $env:LISETTE_INSTALL_DIR = {}\n    $env:LISETTE_NO_MODIFY_PATH = '1'\n    irm {} | iex",
                    powershell_literal(&install_prefix),
                    WINDOWS_INSTALLER_URL
                )
            );
            1
        }
        #[cfg(feature = "self-upgrade")]
        Action::RunInstaller { install_prefix } => run_installer(&install_prefix),
        #[cfg(not(feature = "self-upgrade"))]
        Action::RunInstaller { .. } => packager_message(),
        Action::Packager => packager_message(),
    }
}

fn packager_message() -> i32 {
    cli_error!(
        "Built without self-upgrade",
        "This copy was built with the `self-upgrade` feature off, which is how a distribution packager keeps upgrades under its own package manager",
        "To upgrade, use the tool that installed this copy"
    );
    1
}

fn missing_record_hint(windows: bool, under_cargo_home: bool) -> String {
    let installer = if windows {
        format!("`irm {} | iex`", WINDOWS_INSTALLER_URL)
    } else {
        format!("`curl -fsSL {} | sh`", INSTALLER_URL)
    };
    if under_cargo_home {
        format!(
            "To upgrade, run `cargo install lisette` if Cargo installed this copy, or {}",
            installer
        )
    } else {
        format!(
            "To upgrade, use the tool that installed this copy, or reinstall with {}",
            installer
        )
    }
}

/// Single quotes so that a `$` or a backtick in the path is not expanded when the line is pasted.
fn powershell_literal(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}

/// Components rather than a prefix string, so Linuxbrew and a custom `MISE_DATA_DIR` still match.
fn has_adjacent_components(path: &Path, first: &str, second: &str) -> bool {
    let components: Vec<&OsStr> = path
        .components()
        .map(|component| component.as_os_str())
        .collect();
    components
        .windows(2)
        .any(|pair| pair[0] == OsStr::new(first) && pair[1] == OsStr::new(second))
}

fn non_empty_var(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}

fn home_directory() -> Option<PathBuf> {
    non_empty_var("HOME")
        .or_else(|| non_empty_var("USERPROFILE"))
        .map(PathBuf::from)
}

fn record_path() -> Option<PathBuf> {
    record_directory(
        non_empty_var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .as_deref(),
        non_empty_var("LOCALAPPDATA").map(PathBuf::from).as_deref(),
        home_directory().as_deref(),
        cfg!(windows),
    )
    .map(|directory| directory.join(RECORD_FILE))
}

fn record_directory(
    xdg_config_home: Option<&Path>,
    local_app_data: Option<&Path>,
    home: Option<&Path>,
    windows: bool,
) -> Option<PathBuf> {
    let config = if windows {
        xdg_config_home.or(local_app_data).map(Path::to_path_buf)
    } else {
        xdg_config_home
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join(".config")))
    };
    config.map(|config| config.join("lisette"))
}

fn cargo_bin_directory(cargo_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let cargo_home = match cargo_home {
        Some(path) => path.to_path_buf(),
        None => home?.join(".cargo"),
    };
    Some(cargo_home.join("bin"))
}

fn load_record(path: &Path, windows: bool) -> Result<InstallRecord, RecordError> {
    match fs::read_to_string(path) {
        Ok(text) => parse_record(&text, windows),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(RecordError::Missing),
        Err(_) => Err(RecordError::Unreadable),
    }
}

fn parse_record(text: &str, windows: bool) -> Result<InstallRecord, RecordError> {
    let file: RecordFile = serde_json::from_str(text).map_err(|_| RecordError::Unreadable)?;
    let install_prefix = PathBuf::from(file.install_prefix);
    let executable = recorded_executable(&install_prefix, windows);
    Ok(InstallRecord {
        layout: file.install_layout,
        install_prefix,
        canonical_executable: fs::canonicalize(&executable).unwrap_or(executable),
        version: file.version,
    })
}

fn recorded_executable(install_prefix: &Path, windows: bool) -> PathBuf {
    install_prefix
        .join("bin")
        .join(if windows { "lis.exe" } else { "lis" })
}

#[cfg(feature = "self-upgrade")]
fn installer_fetch_command() -> process::Command {
    let mut command = process::Command::new("curl");
    command.args(["-fsSL", INSTALLER_URL]);
    command
}

/// `NO_MODIFY_PATH` because an upgrade does not move the binary.
#[cfg(feature = "self-upgrade")]
fn installer_shell_command(install_prefix: &Path) -> process::Command {
    let mut command = process::Command::new("sh");
    command
        .env("LISETTE_INSTALL_DIR", install_prefix)
        .env("LISETTE_NO_MODIFY_PATH", "1");
    command
}

/// A pipeline reports only its last stage, so a failed download reaches `sh` as an empty script.
#[cfg(feature = "self-upgrade")]
fn installer_script(downloaded: bool, stdout: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if !downloaded {
        return Err("The download did not complete");
    }
    if stdout.is_empty() {
        return Err("The download was empty");
    }
    Ok(stdout)
}

#[cfg(feature = "self-upgrade")]
fn run_installer(install_prefix: &Path) -> i32 {
    use crate::output::print_progress;
    use std::io::Write;

    print_progress(&format!("curl -fsSL {}", INSTALLER_URL));
    let download = match installer_fetch_command().output() {
        Ok(download) => download,
        Err(error) => {
            cli_error!(
                "Failed to download installer",
                format!("`curl` did not run: {}", error),
                "Install `curl`, then run `lis upgrade` again"
            );
            return 1;
        }
    };

    let script = match installer_script(download.status.success(), download.stdout) {
        Ok(script) => script,
        Err(reason) => {
            let details = String::from_utf8_lossy(&download.stderr);
            let details = details.trim();
            let explanation = if details.is_empty() {
                reason.to_string()
            } else {
                format!("{} ({})", reason, details)
            };
            cli_error!(
                "Failed to download installer",
                explanation,
                "Check the network connection and run `lis upgrade` again"
            );
            return 1;
        }
    };

    print_progress(&format!(
        "LISETTE_INSTALL_DIR={} LISETTE_NO_MODIFY_PATH=1 sh",
        install_prefix.display()
    ));
    let mut child = match installer_shell_command(install_prefix)
        .stdin(process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            cli_error!(
                "Failed to run installer",
                format!("`sh` did not start: {}", error),
                "Make sure `sh` is on the PATH, then run `lis upgrade` again"
            );
            return 1;
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(&script)
    {
        let _ = child.kill();
        cli_error!(
            "Failed to run installer",
            format!("Writing the installer to `sh` failed: {}", error),
            "Run `lis upgrade` again"
        );
        return 1;
    }

    match child.wait() {
        Ok(status) if status.success() => 0,
        Ok(_) => 1,
        Err(error) => {
            cli_error!(
                "Failed to run installer",
                format!("Waiting for `sh` failed: {}", error),
                "Run `lis upgrade` again"
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_BIN: &str = "/Users/x/.cargo/bin";

    fn record(install_prefix: &str, version: &str) -> InstallRecord {
        build_record(install_prefix, version, false)
    }

    fn windows_record(install_prefix: &str, version: &str) -> InstallRecord {
        build_record(install_prefix, version, true)
    }

    fn build_record(install_prefix: &str, version: &str, windows: bool) -> InstallRecord {
        let install_prefix = PathBuf::from(install_prefix);
        InstallRecord {
            layout: CARGO_HOME_LAYOUT.to_string(),
            canonical_executable: recorded_executable(&install_prefix, windows),
            install_prefix,
            version: version.to_string(),
        }
    }

    fn record_json(install_prefix: &Path) -> String {
        format!(
            r#"{{"install_layout":"cargo-home","install_prefix":"{}","version":"0.11.3"}}"#,
            install_prefix.display()
        )
    }

    fn install(executable: &str, record: Result<InstallRecord, RecordError>) -> Install {
        Install {
            executable: PathBuf::from(executable),
            record,
            cargo_bin: Some(PathBuf::from(CARGO_BIN)),
            windows: false,
            self_upgrade: true,
            version: "0.11.3".to_string(),
        }
    }

    fn owned() -> Install {
        install(
            "/Users/x/.cargo/bin/lis",
            Ok(record("/Users/x/.cargo", "0.11.3")),
        )
    }

    #[test]
    fn a_homebrew_copy_is_sent_to_brew() {
        let subject = install(
            "/opt/homebrew/Cellar/lisette/0.11.1/bin/lis",
            Err(RecordError::Missing),
        );
        assert_eq!(decide(&subject), Action::Brew);
    }

    #[test]
    fn a_linuxbrew_copy_is_sent_to_brew() {
        let subject = install(
            "/home/linuxbrew/.linuxbrew/Cellar/lisette/0.11.1/bin/lis",
            Err(RecordError::Missing),
        );
        assert_eq!(decide(&subject), Action::Brew);
    }

    #[cfg(unix)]
    #[test]
    fn a_homebrew_copy_reached_through_a_relative_symlink_is_sent_to_brew() {
        use std::os::unix::fs as unix_fs;

        let directory = tempfile::tempdir().expect("a temp dir");
        let cellar = directory.path().join("Cellar/lisette/0.11.1/bin");
        fs::create_dir_all(&cellar).expect("a cellar dir");
        fs::write(cellar.join("lis"), "").expect("a cellar binary");
        let bin = directory.path().join("bin");
        fs::create_dir(&bin).expect("a bin dir");
        unix_fs::symlink("../Cellar/lisette/0.11.1/bin/lis", bin.join("lis")).expect("a symlink");

        let executable = fs::canonicalize(bin.join("lis")).expect("the symlink resolves");
        let mut subject = install("/unused", Err(RecordError::Missing));
        subject.executable = executable;
        assert_eq!(decide(&subject), Action::Brew);
    }

    #[test]
    fn a_mise_copy_is_sent_to_mise() {
        let subject = install(
            "/Users/x/.local/share/mise/installs/lisette/0.11.3/lis",
            Err(RecordError::Missing),
        );
        assert_eq!(decide(&subject), Action::Mise);
    }

    #[test]
    fn a_custom_mise_data_dir_is_still_sent_to_mise() {
        let subject = install(
            "/opt/mise-data/installs/lisette/0.11.3/lis",
            Err(RecordError::Missing),
        );
        assert_eq!(decide(&subject), Action::Mise);
    }

    #[test]
    fn a_package_manager_copy_outranks_a_record_that_would_match_it() {
        let subject = install(
            "/opt/homebrew/Cellar/lisette/0.11.3/bin/lis",
            Ok(record("/opt/homebrew/Cellar/lisette/0.11.3", "0.11.3")),
        );
        assert_eq!(decide(&subject), Action::Brew);
    }

    #[test]
    fn a_cargo_home_copy_without_a_record_names_both_routes() {
        let subject = install("/Users/x/.cargo/bin/lis", Err(RecordError::Missing));
        assert_eq!(
            decide(&subject),
            Action::NoRecord {
                under_cargo_home: true
            }
        );
        assert!(missing_record_hint(false, true).contains("cargo install lisette"));
        assert!(missing_record_hint(false, true).contains(INSTALLER_URL));
    }

    #[test]
    fn a_copy_outside_cargo_home_without_a_record_gets_the_generic_message() {
        let subject = install("/usr/local/bin/lis", Err(RecordError::Missing));
        assert_eq!(
            decide(&subject),
            Action::NoRecord {
                under_cargo_home: false
            }
        );
        assert!(!missing_record_hint(false, false).contains("cargo install lisette"));
    }

    #[test]
    fn an_unreadable_record_does_not_fall_through_to_the_missing_row() {
        let subject = install("/Users/x/.cargo/bin/lis", Err(RecordError::Unreadable));
        assert_eq!(decide(&subject), Action::UnreadableRecord);
    }

    #[test]
    fn a_record_describing_another_copy_names_the_recorded_path() {
        let subject = install(
            "/Users/x/.cargo/bin/lis",
            Ok(record("/tmp/lis-install-demo", "0.11.3")),
        );
        assert_eq!(
            decide(&subject),
            Action::WrongPath {
                recorded: PathBuf::from("/tmp/lis-install-demo/bin/lis"),
            }
        );
    }

    #[test]
    fn a_record_recording_another_version_refuses_to_upgrade() {
        let subject = install(
            "/Users/x/.cargo/bin/lis",
            Ok(record("/Users/x/.cargo", "0.11.0")),
        );
        assert_eq!(
            decide(&subject),
            Action::WrongVersion {
                recorded: "0.11.0".to_string(),
            }
        );
    }

    #[test]
    fn a_layout_the_installer_never_records_is_not_a_match() {
        let mut recorded = record("/Users/x/.cargo", "0.11.3");
        recorded.layout = "flat".to_string();
        let subject = install("/Users/x/.cargo/bin/lis", Ok(recorded));
        assert_eq!(
            decide(&subject),
            Action::WrongPath {
                recorded: PathBuf::from("/Users/x/.cargo/bin/lis"),
            }
        );
    }

    #[test]
    fn a_record_for_this_executable_runs_the_installer() {
        assert_eq!(
            decide(&owned()),
            Action::RunInstaller {
                install_prefix: PathBuf::from("/Users/x/.cargo"),
            }
        );
    }

    #[test]
    fn a_record_for_a_custom_prefix_carries_that_prefix() {
        let subject = install("/opt/lisette/bin/lis", Ok(record("/opt/lisette", "0.11.3")));
        assert_eq!(
            decide(&subject),
            Action::RunInstaller {
                install_prefix: PathBuf::from("/opt/lisette"),
            }
        );
    }

    #[test]
    fn windows_prints_the_installer_command_instead_of_running_it() {
        let mut subject = install(
            "/c/Users/x/.cargo/bin/lis.exe",
            Ok(windows_record("/c/Users/x/.cargo", "0.11.3")),
        );
        subject.windows = true;
        assert_eq!(
            decide(&subject),
            Action::WindowsCommand {
                install_prefix: PathBuf::from("/c/Users/x/.cargo"),
            }
        );
    }

    #[test]
    fn a_windows_copy_without_a_record_is_never_told_to_run_the_installer() {
        let mut subject = install("/c/Users/x/.cargo/bin/lis.exe", Err(RecordError::Missing));
        subject.windows = true;
        subject.cargo_bin = Some(PathBuf::from("/c/Users/x/.cargo/bin"));
        assert_eq!(
            decide(&subject),
            Action::NoRecord {
                under_cargo_home: true
            }
        );
    }

    #[test]
    fn a_windows_record_that_fails_a_check_gets_the_ownership_message() {
        let mut subject = install(
            "/c/Users/x/.cargo/bin/lis.exe",
            Ok(windows_record("/c/Users/x/.cargo", "0.11.0")),
        );
        subject.windows = true;
        assert_eq!(
            decide(&subject),
            Action::WrongVersion {
                recorded: "0.11.0".to_string(),
            }
        );
    }

    #[test]
    fn windows_derives_the_recorded_path_with_an_exe_suffix() {
        let mut subject = install(
            "/c/Users/x/.cargo/bin/lis",
            Ok(windows_record("/c/Users/x/.cargo", "0.11.3")),
        );
        subject.windows = true;
        assert_eq!(
            decide(&subject),
            Action::WrongPath {
                recorded: PathBuf::from("/c/Users/x/.cargo/bin/lis.exe"),
            }
        );
    }

    #[test]
    fn without_the_feature_a_matching_record_gets_the_packager_message() {
        let mut subject = owned();
        subject.self_upgrade = false;
        assert_eq!(decide(&subject), Action::Packager);
    }

    #[test]
    fn without_the_feature_the_ownership_rows_are_unchanged() {
        let rows = [
            (
                "/opt/homebrew/Cellar/lisette/0.11.1/bin/lis",
                Err(RecordError::Missing),
                Action::Brew,
            ),
            (
                "/Users/x/.local/share/mise/installs/lisette/0.11.3/lis",
                Err(RecordError::Missing),
                Action::Mise,
            ),
            (
                "/Users/x/.cargo/bin/lis",
                Err(RecordError::Missing),
                Action::NoRecord {
                    under_cargo_home: true,
                },
            ),
            (
                "/Users/x/.cargo/bin/lis",
                Err(RecordError::Unreadable),
                Action::UnreadableRecord,
            ),
            (
                "/Users/x/.cargo/bin/lis",
                Ok(record("/tmp/lis-install-demo", "0.11.3")),
                Action::WrongPath {
                    recorded: PathBuf::from("/tmp/lis-install-demo/bin/lis"),
                },
            ),
            (
                "/Users/x/.cargo/bin/lis",
                Ok(record("/Users/x/.cargo", "0.11.0")),
                Action::WrongVersion {
                    recorded: "0.11.0".to_string(),
                },
            ),
        ];
        for (executable, recorded, expected) in rows {
            let mut subject = install(executable, recorded);
            subject.self_upgrade = false;
            assert_eq!(decide(&subject), expected, "for {executable}");
        }
    }

    #[test]
    fn a_full_record_parses() {
        let text = r#"{"binaries":["lis"],"binary_aliases":{},"install_layout":"cargo-home","install_prefix":"/opt/lisette","modify_path":false,"provider":{"source":"cargo-dist","version":"0.31.0"},"version":"0.11.3"}"#;
        let parsed = parse_record(text, false).expect("a full record parses");
        assert_eq!(parsed.layout, CARGO_HOME_LAYOUT);
        assert_eq!(parsed.install_prefix, PathBuf::from("/opt/lisette"));
        assert_eq!(parsed.version, "0.11.3");
        assert_eq!(
            parsed.canonical_executable,
            PathBuf::from("/opt/lisette/bin/lis")
        );
        assert_eq!(
            parse_record(text, true)
                .expect("a full record parses")
                .canonical_executable,
            PathBuf::from("/opt/lisette/bin/lis.exe")
        );
    }

    #[test]
    fn a_truncated_or_incomplete_record_is_unreadable() {
        for text in [
            r#"{"install_layout":"cargo-home","install_prefix":"/opt/lisette""#,
            r#"{"install_layout":"cargo-home","install_prefix":"/opt/lisette"}"#,
            r#"{"install_layout":"cargo-home","version":"0.11.3"}"#,
            "",
            "not json at all",
        ] {
            assert_eq!(
                parse_record(text, false),
                Err(RecordError::Unreadable),
                "for {text}"
            );
        }
    }

    #[test]
    fn a_powershell_literal_expands_nothing() {
        assert_eq!(
            powershell_literal(Path::new(r"D:\dev$env:TEMP\li`s")),
            r"'D:\dev$env:TEMP\li`s'"
        );
        assert_eq!(
            powershell_literal(Path::new("C:\\o'brien\\lisette")),
            "'C:\\o''brien\\lisette'"
        );
    }

    #[test]
    fn a_record_that_is_not_there_is_missing_rather_than_unreadable() {
        let directory = tempfile::tempdir().expect("a temp dir");
        assert_eq!(
            load_record(&directory.path().join(RECORD_FILE), false),
            Err(RecordError::Missing)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_prefix_reached_through_a_symlink_still_matches() {
        use std::os::unix::fs as unix_fs;

        let directory = tempfile::tempdir().expect("a temp dir");
        let real = directory.path().join("real");
        fs::create_dir_all(real.join("bin")).expect("a prefix dir");
        fs::write(real.join("bin").join("lis"), "").expect("an installed binary");
        let link = directory.path().join("link");
        unix_fs::symlink(&real, &link).expect("a symlink");

        let parsed = parse_record(&record_json(&link), false).expect("the record parses");
        let executable =
            fs::canonicalize(real.join("bin").join("lis")).expect("the binary resolves");

        let mut subject = install("/unused", Ok(parsed));
        subject.executable = executable;
        assert_eq!(
            decide(&subject),
            Action::RunInstaller {
                install_prefix: link,
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_bin_dir_is_not_a_second_copy() {
        use std::os::unix::fs as unix_fs;

        let directory = tempfile::tempdir().expect("a temp dir");
        let prefix = directory.path().join("prefix");
        let elsewhere = directory.path().join("elsewhere");
        fs::create_dir(&prefix).expect("a prefix dir");
        fs::create_dir(&elsewhere).expect("a target dir");
        fs::write(elsewhere.join("lis"), "").expect("an installed binary");
        unix_fs::symlink(&elsewhere, prefix.join("bin")).expect("a symlink");

        let parsed = parse_record(&record_json(&prefix), false).expect("the record parses");
        let executable = fs::canonicalize(elsewhere.join("lis")).expect("the binary resolves");

        let mut subject = install("/unused", Ok(parsed));
        subject.executable = executable;
        assert_eq!(
            decide(&subject),
            Action::RunInstaller {
                install_prefix: prefix,
            }
        );
    }

    #[test]
    fn a_record_pointing_at_a_deleted_install_keeps_its_recorded_path() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let gone = directory.path().join("gone");

        let parsed = parse_record(&record_json(&gone), false).expect("the record parses");
        assert_eq!(parsed.canonical_executable, gone.join("bin").join("lis"));

        let mut subject = install("/Users/x/.cargo/bin/lis", Ok(parsed));
        subject.executable = PathBuf::from("/Users/x/.cargo/bin/lis");
        assert_eq!(
            decide(&subject),
            Action::WrongPath {
                recorded: gone.join("bin").join("lis"),
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_tmp_prefix_matches_a_private_tmp_executable() {
        let directory = tempfile::Builder::new()
            .prefix("lis-upgrade")
            .tempdir_in("/tmp")
            .expect("a temp dir under /tmp");
        fs::create_dir(directory.path().join("bin")).expect("a bin dir");
        fs::write(directory.path().join("bin").join("lis"), "").expect("an installed binary");

        let parsed =
            parse_record(&record_json(directory.path()), false).expect("the record parses");
        let executable = fs::canonicalize(directory.path().join("bin").join("lis"))
            .expect("the binary resolves");
        assert!(executable.starts_with("/private/tmp"));

        let mut subject = install("/unused", Ok(parsed));
        subject.executable = executable;
        assert_eq!(
            decide(&subject),
            Action::RunInstaller {
                install_prefix: directory.path().to_path_buf(),
            }
        );
    }

    #[test]
    fn the_record_directory_follows_xdg_config_home_on_unix() {
        assert_eq!(
            record_directory(
                Some(Path::new("/Users/x/config")),
                None,
                Some(Path::new("/Users/x")),
                false
            ),
            Some(PathBuf::from("/Users/x/config/lisette"))
        );
    }

    #[test]
    fn the_record_directory_falls_back_to_the_home_config_dir_on_unix() {
        assert_eq!(
            record_directory(None, None, Some(Path::new("/Users/x")), false),
            Some(PathBuf::from("/Users/x/.config/lisette"))
        );
    }

    #[test]
    fn the_record_directory_reads_local_app_data_on_windows() {
        assert_eq!(
            record_directory(
                None,
                Some(Path::new("/c/Users/x/AppData/Local")),
                Some(Path::new("/c/Users/x")),
                true
            ),
            Some(PathBuf::from("/c/Users/x/AppData/Local/lisette"))
        );
    }

    #[test]
    fn the_record_directory_prefers_xdg_config_home_on_windows() {
        assert_eq!(
            record_directory(
                Some(Path::new("/c/config")),
                Some(Path::new("/c/Users/x/AppData/Local")),
                Some(Path::new("/c/Users/x")),
                true
            ),
            Some(PathBuf::from("/c/config/lisette"))
        );
    }

    #[test]
    fn the_cargo_bin_dir_follows_cargo_home_then_the_home_dir() {
        assert_eq!(
            cargo_bin_directory(Some(Path::new("/opt/cargo")), Some(Path::new("/Users/x"))),
            Some(PathBuf::from("/opt/cargo/bin"))
        );
        assert_eq!(
            cargo_bin_directory(None, Some(Path::new("/Users/x"))),
            Some(PathBuf::from(CARGO_BIN))
        );
        assert_eq!(cargo_bin_directory(None, None), None);
    }

    #[cfg(feature = "self-upgrade")]
    #[test]
    fn the_installer_command_pins_the_recorded_prefix() {
        for prefix in ["/Users/x/.cargo", "/opt/lisette"] {
            let command = installer_shell_command(Path::new(prefix));
            let variables: Vec<_> = command.get_envs().collect();
            assert_eq!(command.get_program(), OsStr::new("sh"));
            assert!(
                variables.contains(&(OsStr::new("LISETTE_INSTALL_DIR"), Some(OsStr::new(prefix)))),
                "for {prefix}"
            );
            assert!(
                variables.contains(&(OsStr::new("LISETTE_NO_MODIFY_PATH"), Some(OsStr::new("1")))),
                "for {prefix}"
            );
        }
    }

    #[cfg(feature = "self-upgrade")]
    #[test]
    fn the_download_command_makes_curl_fail_on_an_error_status() {
        let command = installer_fetch_command();
        assert_eq!(command.get_program(), OsStr::new("curl"));
        let arguments: Vec<_> = command.get_args().collect();
        assert_eq!(
            arguments,
            vec![OsStr::new("-fsSL"), OsStr::new(INSTALLER_URL)]
        );
    }

    #[cfg(feature = "self-upgrade")]
    #[test]
    fn a_failed_or_empty_download_never_becomes_a_script() {
        assert!(installer_script(false, b"#!/bin/sh\n".to_vec()).is_err());
        assert!(installer_script(true, Vec::new()).is_err());
        assert_eq!(
            installer_script(true, b"#!/bin/sh\n".to_vec()),
            Ok(b"#!/bin/sh\n".to_vec())
        );
    }
}
