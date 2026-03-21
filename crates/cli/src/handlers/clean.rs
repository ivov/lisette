use std::fs;
use std::path::Path;

use crate::cli_error;

pub fn clean(path: Option<String>) -> i32 {
    let project_root = path.unwrap_or_else(|| ".".to_string());
    let target_dir = Path::new(&project_root).join("target");

    if !target_dir.exists() {
        eprintln!();
        eprintln!("  ✓ No target directory to clean");
        return 0;
    }

    match fs::remove_dir_all(target_dir) {
        Ok(_) => {
            eprintln!();
            eprintln!("  ✓ Cleaned target directory");
            0
        }
        Err(e) => {
            cli_error!(
                "Failed to clean",
                format!("Failed to remove `target` directory: {}", e),
                "Check directory permissions"
            );
            1
        }
    }
}
