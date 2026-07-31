use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use format::format_source;
use lisette::fs::collect_lis_filepaths_recursive;
use rayon::prelude::*;

use crate::cli_error;

enum Outcome {
    Unchanged,
    Changed,
    Failed { reason: String, hint: &'static str },
}

pub fn format(path: Option<String>, check: bool) -> i32 {
    let target = path.unwrap_or_else(|| ".".to_string());
    let target_path = Path::new(&target);

    if !target_path.exists() {
        cli_error!(
            "Failed to format",
            format!("Path `{}` does not exist", target),
            "Check the path and try again"
        );
        return 1;
    }

    let filepaths: Vec<std::path::PathBuf> = if target_path.is_dir() {
        collect_lis_filepaths_recursive(target_path)
    } else {
        vec![target_path.to_path_buf()]
    };

    if filepaths.is_empty() {
        eprintln!();
        eprintln!("  ✓ No .lis files to format");
        return 0;
    }

    let start = Instant::now();

    let outcomes: Vec<Outcome> = filepaths
        .par_iter()
        .map(|file| {
            let source = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    return Outcome::Failed {
                        reason: format!("Failed to read `{}`: {}", file.display(), e),
                        hint: "Check file permissions",
                    };
                }
            };

            let formatted = match format_source(&source) {
                Ok(f) => f,
                Err(errors) => {
                    return Outcome::Failed {
                        reason: format!(
                            "Parse error in `{}`: {} error(s)",
                            file.display(),
                            errors.len()
                        ),
                        hint: "Fix syntax errors first",
                    };
                }
            };

            if source == formatted {
                return Outcome::Unchanged;
            }

            if check {
                return Outcome::Changed;
            }

            match fs::File::create(file) {
                Ok(mut f) => match f.write_all(formatted.as_bytes()) {
                    Ok(()) => Outcome::Changed,
                    Err(e) => Outcome::Failed {
                        reason: format!("Failed to write `{}`: {}", file.display(), e),
                        hint: "Check file permissions",
                    },
                },
                Err(e) => Outcome::Failed {
                    reason: format!("Failed to open `{}` for writing: {}", file.display(), e),
                    hint: "Check file permissions",
                },
            }
        })
        .collect();

    let mut changed_files: Vec<std::path::PathBuf> = Vec::new();
    let mut error_count = 0;

    for (file, outcome) in filepaths.iter().zip(outcomes) {
        match outcome {
            Outcome::Unchanged => {}
            Outcome::Changed => changed_files.push(file.clone()),
            Outcome::Failed { reason, hint } => {
                cli_error!("Failed to format", reason, hint);
                error_count += 1;
            }
        }
    }

    if error_count > 0 {
        return 1;
    }

    eprintln!();

    let time_display = crate::output::format_elapsed(start.elapsed());

    if check {
        let colored = crate::output::use_color();
        let render_path = |file: &std::path::Path| -> String {
            let s = file.display().to_string();
            if colored {
                use owo_colors::OwoColorize;
                format!("{}", s.bright_magenta())
            } else {
                format!("`{}`", s)
            }
        };
        match changed_files.len() {
            0 => {
                eprintln!("  ✓ No changes needed {}", time_display);
                return 0;
            }
            1 => {
                eprintln!(
                    "  ✖ 1 file needs formatting: {} {}",
                    render_path(&changed_files[0]),
                    time_display
                );
            }
            n => {
                eprintln!("  ✖ {} files need formatting: {}", n, time_display);
                for file in &changed_files {
                    eprintln!("    {}", render_path(file));
                }
            }
        }
        return 1;
    }

    match changed_files.len() {
        0 => eprintln!("  ✓ All files formatted {}", time_display),
        1 => eprintln!("  ✓ Formatted 1 file {}", time_display),
        n => eprintln!("  ✓ Formatted {} files {}", n, time_display),
    }

    0
}
