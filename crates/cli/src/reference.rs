use std::fs;
use std::io;
use std::path::Path;

/// The language reference, in the order the site lists it.
pub const CHAPTERS: &[(&str, &str)] = &[
    ("lexemes.md", include_str!("../reference/lexemes.md")),
    ("types.md", include_str!("../reference/types.md")),
    ("bindings.md", include_str!("../reference/bindings.md")),
    ("operators.md", include_str!("../reference/operators.md")),
    (
        "control-flow.md",
        include_str!("../reference/control-flow.md"),
    ),
    ("structs.md", include_str!("../reference/structs.md")),
    ("enums.md", include_str!("../reference/enums.md")),
    ("references.md", include_str!("../reference/references.md")),
    (
        "pattern-matching.md",
        include_str!("../reference/pattern-matching.md"),
    ),
    ("attributes.md", include_str!("../reference/attributes.md")),
    ("functions.md", include_str!("../reference/functions.md")),
    ("methods.md", include_str!("../reference/methods.md")),
    ("interfaces.md", include_str!("../reference/interfaces.md")),
    ("failures.md", include_str!("../reference/failures.md")),
    (
        "concurrency.md",
        include_str!("../reference/concurrency.md"),
    ),
];

pub fn write_to(project_dir: &Path) -> io::Result<()> {
    let docs_dir = project_dir.join("target").join(".lisette").join("docs");
    let stamp = docs_dir.join(concat!(".stamp-", env!("CARGO_PKG_VERSION")));
    if stamp.exists() {
        return Ok(());
    }

    match fs::remove_dir_all(&docs_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::create_dir_all(&docs_dir)?;
    for (name, body) in CHAPTERS {
        fs::write(docs_dir.join(name), body)?;
    }
    fs::write(stamp, "")
}
