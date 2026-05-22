use std::path::{Component, Path};

/// Returns path relative to the cwd as a forward-slash string.
/// Returns None if the cwd is unknown or the path lies outside it.
pub fn relative_to_cwd(path: &Path) -> Option<String> {
    relative_to_cwd_with(path, std::env::current_dir().ok().as_deref())
}

pub fn relative_to_cwd_with(path: &Path, cwd: Option<&Path>) -> Option<String> {
    let cwd = cwd?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    if let (Ok(base), Ok(target)) = (cwd.canonicalize(), absolute.canonicalize()) {
        return relativize(target.strip_prefix(&base).ok()?);
    }
    relativize(absolute.strip_prefix(cwd).ok()?)
}

fn relativize(rel: &Path) -> Option<String> {
    let mut segments = Vec::new();
    for component in rel.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => segments.push(segment.to_str()?),
            _ => return None,
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    #[test]
    fn plain_path_inside_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        assert_eq!(
            relative_to_cwd_with(&cwd.join("src/main.lis"), Some(cwd)),
            Some("src/main.lis".to_string())
        );
    }

    #[test]
    fn file_at_cwd_root() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        assert_eq!(
            relative_to_cwd_with(&cwd.join("main.lis"), Some(cwd)),
            Some("main.lis".to_string())
        );
    }

    #[test]
    fn strips_leading_dot_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        assert_eq!(
            relative_to_cwd_with(&cwd.join("./src/main.lis"), Some(cwd)),
            Some("src/main.lis".to_string())
        );
    }

    #[test]
    fn strips_mid_path_dot() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path();
        assert_eq!(
            relative_to_cwd_with(&cwd.join("src/./main.lis"), Some(cwd)),
            Some("src/main.lis".to_string())
        );
    }

    #[test]
    fn path_outside_cwd_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        assert_eq!(
            relative_to_cwd_with(&other.path().join("main.lis"), Some(tmp.path())),
            None
        );
    }

    #[test]
    fn unknown_cwd_returns_none() {
        assert_eq!(
            relative_to_cwd_with(Path::new("/any/path/main.lis"), None),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn absolute_path_under_symlinked_cwd_strips() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        stdfs::create_dir_all(real.join("src")).unwrap();
        stdfs::write(real.join("src/main.lis"), "").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert_eq!(
            relative_to_cwd_with(&real.join("src/main.lis"), Some(&link)),
            Some("src/main.lis".to_string())
        );
    }
}
