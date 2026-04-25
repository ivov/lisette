use std::fs::File;
use std::path::Path;

use fs2::FileExt;

use crate::{cli_error, error};

pub fn acquire_mutation_lock(target_dir: &Path) -> Result<File, i32> {
    let lock_path = target_dir.join(".lis-mutate.lock");
    let file = match File::create(&lock_path) {
        Ok(f) => f,
        Err(e) => {
            error!(
                "failed to create lock file",
                format!("Failed to create `{}`: {}", lock_path.display(), e)
            );
            return Err(1);
        }
    };

    if let Err(e) = file.try_lock_exclusive() {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            cli_error!(
                "Another manifest mutation is in progress",
                "A concurrent `lis add` or `lis sync` holds the project lock",
                "Wait for the other invocation to finish, then retry"
            );
        } else {
            error!("failed to acquire lock", format!("{}", e));
        }
        return Err(1);
    }

    Ok(file)
}
