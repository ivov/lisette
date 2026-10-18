use std::{collections::hash_map, iter::Flatten, mem};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::Usage;

/// Task-owned usage production. Each set contains only usages from its file.
/// Merging a previously unseen file transfers its allocation intact.
#[derive(Debug, Default)]
pub struct UsageFacts {
    files: HashMap<u32, HashSet<Usage>>,
}

impl UsageFacts {
    pub(super) fn insert(&mut self, usage: Usage) {
        self.files
            .entry(usage.usage_span.file_id)
            .or_default()
            .insert(usage);
    }

    pub(super) fn merge(&mut self, other: Self) {
        for (file, mut incoming) in other.files {
            match self.files.entry(file) {
                hash_map::Entry::Vacant(entry) => {
                    entry.insert(incoming);
                }
                hash_map::Entry::Occupied(mut entry) => {
                    // Keep the larger table's allocation when registration and inference
                    // both produced usages for this file.
                    if entry.get().len() < incoming.len() {
                        mem::swap(entry.get_mut(), &mut incoming);
                    }
                    entry.get_mut().extend(incoming);
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.files.values().map(HashSet::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Usage> {
        self.into_iter()
    }
    pub fn in_file(&self, file: u32) -> impl Iterator<Item = &Usage> {
        self.files.get(&file).into_iter().flatten()
    }

    pub fn freeze(self) -> UsageIndex {
        UsageIndex {
            files: self
                .files
                .into_iter()
                .map(|(file, usages)| {
                    let mut usages: Vec<_> = usages.into_iter().collect();
                    usages.sort_unstable_by_key(|usage| {
                        let at = usage.usage_span;
                        let definition = usage.definition_span;
                        (
                            at.byte_offset,
                            at.byte_length,
                            definition.file_id,
                            definition.byte_offset,
                            definition.byte_length,
                        )
                    });
                    (file, usages.into_boxed_slice())
                })
                .collect(),
        }
    }
}

impl<'a> IntoIterator for &'a UsageFacts {
    type Item = &'a Usage;
    type IntoIter = Flatten<hash_map::Values<'a, u32, HashSet<Usage>>>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.values().flatten()
    }
}

/// Immutable editor view. Entries in each file are unique full span pairs,
/// ordered by usage position and then definition position.
#[derive(Debug, Default)]
pub struct UsageIndex {
    files: HashMap<u32, Box<[Usage]>>,
}

impl UsageIndex {
    pub fn len(&self) -> usize {
        self.files.values().map(|usages| usages.len()).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Usage> {
        self.into_iter()
    }
    pub fn in_file(&self, file: u32) -> &[Usage] {
        self.files.get(&file).map_or(&[], Box::as_ref)
    }
}

impl<'a> IntoIterator for &'a UsageIndex {
    type Item = &'a Usage;
    type IntoIter = Flatten<hash_map::Values<'a, u32, Box<[Usage]>>>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.values().flatten()
    }
}

const _: () = assert!(size_of::<Usage>() == 24);

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Span;

    #[test]
    fn merging_overlapping_and_disjoint_files_preserves_unique_sorted_usages() {
        let usage = |file, offset| Usage {
            usage_span: Span::new(file, offset, 4),
            definition_span: Span::new(10, 0, 4),
        };
        let first = usage(1, 10);
        let second = usage(1, 20);
        let third = usage(1, 30);
        let other_file = usage(2, 10);

        // Exercise either side owning the larger overlapping file table.
        for reverse in [false, true] {
            let mut left = UsageFacts::default();
            left.insert(second);
            let mut right = UsageFacts::default();
            for item in [third, first, second, other_file] {
                right.insert(item);
            }
            if reverse {
                mem::swap(&mut left, &mut right);
            }
            left.merge(right);
            left.merge(UsageFacts::default());

            assert_eq!(left.len(), 4);
            let index = left.freeze();
            assert_eq!(index.len(), 4);
            assert_eq!(index.in_file(1), [first, second, third]);
            assert_eq!(index.in_file(2), [other_file]);
            assert!(index.in_file(3).is_empty());
        }
    }

    #[test]
    fn usage_identity_includes_both_complete_spans() {
        let mut facts = UsageFacts::default();
        let at = Span::new(1, 10, 4);
        for definition in [Span::new(2, 0, 4), Span::new(3, 0, 4), Span::new(2, 0, 5)] {
            facts.insert(Usage {
                usage_span: at,
                definition_span: definition,
            });
            facts.insert(Usage {
                usage_span: at,
                definition_span: definition,
            });
        }
        assert_eq!(facts.freeze().in_file(1).len(), 3);
    }
}
