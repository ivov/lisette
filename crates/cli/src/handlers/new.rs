use std::fs;
use std::path::Path;

use crate::cli_error;

pub fn new_project(name: &str) -> i32 {
    let project_dir = Path::new(name);

    let project_name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);

    if let Err(msg) = deps::validate_project_name(project_name) {
        cli_error!(
            "Invalid project name",
            msg,
            "Choose a different project name"
        );
        return 1;
    }

    if is_go_stdlib_package(project_name) {
        cli_error!(
            "Invalid project name",
            format!(
                "`{}` conflicts with a Go standard library package",
                project_name
            ),
            "Choose a different project name"
        );
        return 1;
    }

    if project_dir.exists() {
        cli_error!(
            "Failed to create project",
            format!("Directory `{}` already exists", name),
            "Choose a different project name"
        );
        return 1;
    }

    if let Err(e) = fs::create_dir(project_dir) {
        cli_error!(
            "Failed to create project",
            format!("Failed to create directory `{}`: {}", name, e),
            "Check directory permissions"
        );
        return 1;
    }

    let src_dir = project_dir.join("src");
    if let Err(e) = fs::create_dir(&src_dir) {
        cli_error!(
            "Failed to create project",
            format!("Failed to create `src` directory: {}", e),
            "Check directory permissions"
        );
        return 1;
    }

    let toml_content = format!(
        r#"[project]
name = "{}"
version = "0.1.0"
"#,
        project_name
    );
    if let Err(e) = fs::write(project_dir.join("lisette.toml"), toml_content) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `lisette.toml`: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    let main_lis_content = r#"import "go:fmt"

fn main() {
  fmt.Println("Hello world!")
}
"#;
    if let Err(e) = fs::write(src_dir.join("main.lis"), main_lis_content) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `src/main.lis`: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    let readme_content = format!(
        r#"# {}

```bash
lis build
go run -C target .
```
"#,
        project_name
    );
    if let Err(e) = fs::write(project_dir.join("README.md"), readme_content) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `README.md`: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    if let Err(e) = fs::write(project_dir.join("AGENTS.md"), crate::agents_md::AGENTS_MD) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `AGENTS.md`: {}", e),
            "Check file permissions"
        );
        return 1;
    }

    let gitignore_content = "target/\n";
    if let Err(e) = fs::write(project_dir.join(".gitignore"), gitignore_content) {
        cli_error!(
            "Failed to create project",
            format!("Failed to write `.gitignore`: {}", e),
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
        eprintln!("  ✓ Created {} project", project_name.bright_magenta());
    } else {
        eprintln!("  ✓ Created `{}` project", project_name);
    }

    0
}

fn is_go_stdlib_package(name: &str) -> bool {
    stdlib::get_go_stdlib_packages()
        .iter()
        .any(|pkg| pkg.split('/').next() == Some(name))
}
