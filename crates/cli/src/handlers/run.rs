use std::path::Path;
use std::process::Command;

use crate::cli_error;
use crate::go_cli;
use crate::handlers::project::FileTarget;
use lisette::pipeline::ProjectKind;

fn exec_binary(output_path: &Path, args: &[String], heading: &str) -> i32 {
    match Command::new(output_path).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            cli_error!(
                heading,
                format!("Failed to execute compiled binary: {}", e),
                "Check that the binary was produced and is executable"
            );
            1
        }
    }
}

pub fn run(
    target: Option<String>,
    args: Vec<String>,
    sourcemap: bool,
    go_flags: Vec<String>,
) -> i32 {
    let target = target.unwrap_or_else(|| ".".to_string());
    let target_path = Path::new(&target);

    if !target_path.exists() {
        cli_error!(
            "Nothing to run",
            format!("Path `{}` does not exist", target),
            "Check the path and try again"
        );
        return 1;
    }

    if !target_path.is_file() {
        return run_project(target_path, args, sourcemap, &go_flags);
    }

    match super::project::resolve_file_target(target_path) {
        FileTarget::ProjectEntry { root } => run_project(&root, args, sourcemap, &go_flags),
        FileTarget::ProjectPackage { root } => not_an_entrypoint(target_path, &root),
        FileTarget::Script { inside_project } => {
            run_script(&target, args, sourcemap, &go_flags, inside_project)
        }
    }
}

fn not_an_entrypoint(file_path: &Path, root: &Path) -> i32 {
    let file =
        lisette::fs::relative_to_cwd(file_path).unwrap_or_else(|| file_path.display().to_string());
    let project = super::project::project_label(root);
    let project_path = root.display();

    if root.join("src").join("main.lis").is_file() {
        cli_error!(
            "Nothing to run",
            format!(
                "`{}` is a package of project `{}`, not its `main`",
                file, project
            ),
            format!("Run `lis run {}` to run the project", project_path)
        );
    } else {
        cli_error!(
            "Nothing to run",
            format!(
                "`{}` belongs to `{}`, which is a library, as it has no `src/main.lis` entrypoint",
                file, project
            ),
            "If not meant to be a library, convert it to a binary by adding `src/main.lis`"
        );
    }
    1
}

fn run_project(
    project_path: &Path,
    args: Vec<String>,
    sourcemap: bool,
    go_flags: &[String],
) -> i32 {
    let project = match super::build::LockedProject::acquire(project_path) {
        Ok(project) => project,
        Err(code) => return code,
    };

    if project.kind == ProjectKind::Library {
        cli_error!(
            "Nothing to run",
            format!(
                "`{}` is a library, as it has no `src/main.lis` entrypoint",
                project.manifest.project.name
            ),
            "If not meant to be a library, convert it to a binary by adding `src/main.lis`"
        );
        return 1;
    }

    let heading = "Failed to run project";
    let target = stdlib::Target::host();

    if let Err(code) =
        super::build::build_locked(&project, super::build::BuildPurpose::Run { sourcemap })
    {
        return code;
    }

    let output_path = match super::build::link_project_binary(&project, go_flags, target, heading) {
        Ok(p) => p,
        Err(code) => return code,
    };

    exec_binary(&output_path, &args, heading)
}

fn run_script(
    file: &str,
    args: Vec<String>,
    sourcemap: bool,
    go_flags: &[String],
    inside_project: bool,
) -> i32 {
    let heading = "Failed to run script";
    let build = match super::script::prepare(Path::new(file), sourcemap, inside_project, heading) {
        Ok(build) => build,
        Err(code) => return code,
    };

    let output_path = build.dir.join(go_cli::run_binary_name(build.target));
    if let Err(e) = go_cli::build_binary(&build.dir, &output_path, build.target, go_flags) {
        cli_error!(heading, e.message, e.hint);
        return 1;
    }
    exec_binary(&output_path, &args, heading)
}
