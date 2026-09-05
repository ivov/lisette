use std::collections::HashMap;
use std::sync::LazyLock;

use crate::Target;
use crate::go_modules::TYPEDEF_BUNDLE;

type PackageTargets = Vec<(&'static str, &'static str)>;

#[derive(Default)]
struct TypedefIndex {
    common: HashMap<&'static str, &'static str>,
    overlays: HashMap<Target, HashMap<&'static str, &'static str>>,
    package_targets: HashMap<&'static str, PackageTargets>,
}

static TYPEDEF_INDEX: LazyLock<TypedefIndex> =
    LazyLock::new(|| TypedefIndex::parse(TYPEDEF_BUNDLE));

impl TypedefIndex {
    fn parse(bundle: &'static str) -> Self {
        let (metadata, mut contents) = bundle
            .split_once("\n\n")
            .expect("embedded typedef bundle has metadata");
        let mut sources = HashMap::new();
        while !contents.is_empty() {
            let (header, remainder) = contents
                .split_once('\n')
                .expect("embedded typedef has a header");
            let (filename, length) = header
                .split_once('\t')
                .expect("embedded typedef header has a byte length");
            let length = length
                .parse()
                .expect("embedded typedef length is an integer");
            let (source, remainder) = remainder.split_at(length);
            sources.insert(filename, source);
            contents = remainder;
        }

        let mut index = Self::default();
        for record in metadata.trim_end_matches('\r').lines() {
            let fields: Vec<_> = record.split('\t').collect();
            match fields.as_slice() {
                ["common", package, filename] => {
                    index.common.insert(*package, sources[*filename]);
                }
                ["overlay", operating_system, architecture, package, filename] => {
                    index
                        .overlays
                        .entry(Target::new(operating_system, architecture))
                        .or_default()
                        .insert(*package, sources[*filename]);
                }
                ["targets", package, targets @ ..] => {
                    let targets = targets.chunks_exact(2);
                    assert!(targets.remainder().is_empty(), "embedded targets are pairs");
                    index.package_targets.insert(
                        *package,
                        targets.map(|target| (target[0], target[1])).collect(),
                    );
                }
                _ => panic!("invalid embedded typedef metadata"),
            }
        }
        index
    }

    fn is_available_on(&self, package: &str, target: Target) -> bool {
        self.package_targets
            .get(package)
            .is_none_or(|targets| targets.contains(&(target.goos, target.goarch)))
    }
}

pub fn get_go_stdlib_typedef(package: &str, target: Target) -> Option<&'static str> {
    if !TYPEDEF_INDEX.is_available_on(package, target) {
        return None;
    }
    TYPEDEF_INDEX
        .overlays
        .get(&target)
        .and_then(|overlay| overlay.get(package))
        .or_else(|| TYPEDEF_INDEX.common.get(package))
        .copied()
}

pub fn get_go_stdlib_packages(target: Target) -> Vec<&'static str> {
    let mut packages: Vec<_> = TYPEDEF_INDEX
        .common
        .keys()
        .copied()
        .filter(|package| TYPEDEF_INDEX.is_available_on(package, target))
        .collect();
    if let Some(overlay) = TYPEDEF_INDEX.overlays.get(&target) {
        packages.extend(overlay.keys().copied());
    }
    packages.sort();
    packages
}

pub fn get_go_stdlib_package_targets(
    package: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    TYPEDEF_INDEX
        .package_targets
        .get(package)
        .map(Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_lengths_preserve_source_boundaries() {
        for bundle in [
            "common\tfirst\tfirst.d.lis\ncommon\tsecond\tsecond.d.lis\n\nfirst.d.lis\t5\nα\t\n\nsecond.d.lis\t1\nz",
            "common\tfirst\tfirst.d.lis\r\ncommon\tsecond\tsecond.d.lis\r\n\nfirst.d.lis\t5\nα\t\n\nsecond.d.lis\t1\nz",
        ] {
            let index = TypedefIndex::parse(bundle);
            assert_eq!(index.common["first"], "α\t\n\n");
            assert_eq!(index.common["second"], "z");
        }
    }

    #[test]
    fn availability_is_independent_of_shared_contents() {
        let index = TypedefIndex::parse(
            "common\tlimited\tlimited.d.lis\ntargets\tlimited\tlinux\tamd64\n\nlimited.d.lis\t1\nx",
        );
        assert!(index.is_available_on("limited", Target::new("linux", "amd64")));
        assert!(!index.is_available_on("limited", Target::new("windows", "amd64")));
        assert!(index.is_available_on("unknown", Target::new("freebsd", "amd64")));
    }
}
