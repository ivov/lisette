use std::fs;
use std::path::Path;

use crate::cli_error;

const MAIN: &str = include_str!("learn/main.lis");
const PROPS: &str = include_str!("learn/models/props.lis");
const TASK: &str = include_str!("learn/models/task.lis");
const STORE: &str = include_str!("learn/store/store.lis");
const COMMANDS: &str = include_str!("learn/commands/commands.lis");
const DISPLAY: &str = include_str!("learn/display/display.lis");
const README: &str = include_str!("learn/README.md");

pub fn learn() -> i32 {
    let project_dir = Path::new("learn-lisette");

    if project_dir.exists() {
        cli_error!(
            "Failed to create project",
            "Directory `learn-lisette` already exists",
            "Remove it first or run from a different directory"
        );
        return 1;
    }

    let dirs = [
        "",
        "src",
        "src/models",
        "src/store",
        "src/commands",
        "src/display",
    ];

    for dir in &dirs {
        let path = if dir.is_empty() {
            project_dir.to_path_buf()
        } else {
            project_dir.join(dir)
        };
        if let Err(e) = fs::create_dir(&path) {
            cli_error!(
                "Failed to create project",
                format!("Failed to create directory `{}`: {}", path.display(), e),
                "Check directory permissions"
            );
            return 1;
        }
    }

    let files = [
        (
            "lisette.toml",
            "[project]\nname = \"learn-lisette\"\nversion = \"0.1.0\"\n",
        ),
        ("src/main.lis", MAIN),
        ("src/models/props.lis", PROPS),
        ("src/models/task.lis", TASK),
        ("src/store/store.lis", STORE),
        ("src/commands/commands.lis", COMMANDS),
        ("src/display/display.lis", DISPLAY),
        ("README.md", README),
        (".gitignore", "target/\ntasks.json\n"),
    ];

    for (path, content) in &files {
        if let Err(e) = fs::write(project_dir.join(path), content) {
            cli_error!(
                "Failed to create project",
                format!("Failed to write `{}`: {}", path, e),
                "Check file permissions"
            );
            return 1;
        }
    }

    if let Err(e) = fs::write(project_dir.join("AGENTS.md"), crate::agents_md::AGENTS_MD) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `AGENTS.md`: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    let _ = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(project_dir)
        .status();

    crate::go_cli::prewarm_module_cache();

    eprintln!();
    if crate::output::use_color() {
        use owo_colors::OwoColorize;
        eprintln!("  ✓ Created {} project", "learn-lisette".bright_magenta());
        eprintln!(
            "    cd {} and open in your editor to get started",
            "learn-lisette".bright_magenta()
        );
    } else {
        eprintln!("  ✓ Created `learn-lisette` project");
        eprintln!("    cd `learn-lisette` and open in your editor to get started");
    }

    0
}
