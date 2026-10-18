//! Collections with private keys: callers can update values without invalidating
//! a lookup index or the ordering of a compact table.

use std::{borrow::Borrow, hash::Hash, mem, ops::Index, slice, vec};

use rustc_hash::FxHashMap;
use std::{collections::hash_map, sync::Arc};

#[derive(Debug, Clone, Copy)]
struct Slot(u32);

const _: () = assert!(size_of::<Slot>() == 4);

/// An associative index over densely packed records. Hash buckets contain only
/// keys and slots, so growing the index does not move large compiler records.
#[derive(Debug, Clone)]
pub struct DenseMap<K, V> {
    index: FxHashMap<K, Slot>,
    entries: Vec<(K, V)>,
}

impl<K, V> Default for DenseMap<K, V> {
    fn default() -> Self {
        Self {
            index: FxHashMap::default(),
            entries: Vec::new(),
        }
    }
}

impl<K: Eq + Hash + Clone, V> DenseMap<K, V> {
    pub fn insert_if_absent(&mut self, key: K, value: V) {
        if !self.index.contains_key(&key) {
            self.insert(key, value);
        }
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(slot) = self.index.get(&key) {
            return Some(mem::replace(&mut self.entries[slot.0 as usize].1, value));
        }
        self.index.insert(
            key.clone(),
            Slot(u32::try_from(self.entries.len()).expect("too many indexed records")),
        );
        self.entries.push((key, value));
        None
    }

    pub fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.index
            .get(key)
            .map(|slot| &self.entries[slot.0 as usize].1)
    }

    pub fn get_mut<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        self.index
            .get(key)
            .map(|slot| &mut self.entries[slot.0 as usize].1)
    }

    pub fn contains_key<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.index.contains_key(key)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        let mut slot = 0;
        while slot < self.entries.len() {
            let (key, value) = &mut self.entries[slot];
            if keep(key, value) {
                slot += 1;
                continue;
            }
            let removed = self.entries.swap_remove(slot);
            self.index.remove(&removed.0);
            if let Some((moved, _)) = self.entries.get(slot) {
                *self.index.get_mut(moved).expect("moved record is indexed") = Slot(slot as u32);
            }
            // Restore the index before dropping the removed value or invoking
            // the predicate again, including when either unwinds.
            drop(removed);
        }
    }
}

impl<K, V> DenseMap<K, V> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn iter(&self) -> Entries<'_, K, V> {
        Entries(self.entries.iter())
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, value)| value)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, value)| value)
    }
}

impl<K: Eq + Hash + Clone, V> Extend<(K, V)> for DenseMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, entries: I) {
        let entries = entries.into_iter();
        self.entries.reserve(entries.size_hint().0);
        self.index.reserve(entries.size_hint().0);
        for (key, value) in entries {
            self.insert(key, value);
        }
    }
}

impl<K: Eq + Hash + Clone, V> FromIterator<(K, V)> for DenseMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Self {
        let mut map = Self::default();
        map.extend(entries);
        map
    }
}

impl<K: Eq + Hash + Clone + Borrow<Q>, V, Q: Eq + Hash + ?Sized> Index<&Q> for DenseMap<K, V> {
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key).expect("key must be present")
    }
}

impl<K, V> IntoIterator for DenseMap<K, V> {
    type Item = (K, V);
    type IntoIter = vec::IntoIter<(K, V)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a DenseMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Entries<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Entries<'a, K, V>(slice::Iter<'a, (K, V)>);

impl<'a, K, V> Iterator for Entries<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, value)| (key, value))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl<K, V> ExactSizeIterator for Entries<'_, K, V> {}

/// A compact lexical-frame dictionary. Replacement retains one entry per key;
/// reverse lookup favors recently introduced locals without a hash allocation.
#[derive(Debug, Clone)]
pub struct FlatMap<K, V>(Vec<(K, V)>);

impl<K, V> Default for FlatMap<K, V> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<K: Eq, V> FlatMap<K, V> {
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some(existing) = self.get_mut(&key) {
            return Some(mem::replace(existing, value));
        }
        self.0.push((key, value));
        None
    }
    pub fn insert_if_absent(&mut self, key: K, value: V) {
        if self.get(&key).is_none() {
            self.0.push((key, value));
        }
    }
    pub fn get<Q: Eq + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.0
            .iter()
            .rev()
            .find(|(candidate, _)| candidate.borrow() == key)
            .map(|(_, value)| value)
    }
    pub fn get_mut<Q: Eq + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        self.0
            .iter_mut()
            .rev()
            .find(|(candidate, _)| <K as Borrow<Q>>::borrow(candidate) == key)
            .map(|(_, value)| value)
    }
    pub fn iter(&self) -> Entries<'_, K, V> {
        Entries(self.0.iter())
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.0.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().map(|(_, value)| value)
    }
}

/// Read-mostly dictionaries share their storage until a value changes. Empty
/// dictionaries allocate nothing. Hash iteration order remains the same as the
/// producer's table because emission and diagnostic ordering depend on it.
#[derive(Debug, Clone)]
pub struct SharedMap<K, V>(Option<Arc<FxHashMap<K, V>>>);

impl<K, V> Default for SharedMap<K, V> {
    fn default() -> Self {
        Self(None)
    }
}

impl<K: Eq + Hash + Clone, V: Clone> SharedMap<K, V> {
    fn make_mut(&mut self) -> &mut FxHashMap<K, V> {
        Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(FxHashMap::default())))
    }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.make_mut().insert(key, value)
    }
    pub fn insert_if_absent(&mut self, key: K, value: V) {
        if !self.contains_key(&key) {
            self.make_mut().entry(key).or_insert(value);
        }
    }
    pub fn get_mut<Q: Eq + Hash + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        if !self.contains_key(key) {
            return None;
        }
        self.make_mut().get_mut(key)
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.0
            .iter_mut()
            .flat_map(|map| Arc::make_mut(map).values_mut())
    }
}

impl<K: Eq + Hash, V> SharedMap<K, V> {
    pub fn get<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.0.as_ref()?.get(key)
    }
    pub fn get_key_value<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
    {
        self.0.as_ref()?.get_key_value(key)
    }
    pub fn contains_key<Q: Eq + Hash + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.get(key).is_some()
    }
}

impl<K, V> SharedMap<K, V> {
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |map| map.len())
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn iter(&self) -> SharedEntries<'_, K, V> {
        SharedEntries(self.0.as_ref().map(|map| map.iter()))
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
    pub fn into_values(self) -> impl Iterator<Item = V>
    where
        K: Clone,
        V: Clone,
    {
        self.into_iter().map(|(_, value)| value)
    }
}

impl<K: Eq + Hash, V: PartialEq> PartialEq for SharedMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl<K: Eq + Hash, V> FromIterator<(K, V)> for SharedMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Self {
        let map: FxHashMap<_, _> = entries.into_iter().collect();
        if map.is_empty() {
            Self::default()
        } else {
            Self(Some(Arc::new(map)))
        }
    }
}

impl<K: Clone, V: Clone> IntoIterator for SharedMap<K, V> {
    type Item = (K, V);
    type IntoIter = hash_map::IntoIter<K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.0
            .map(Arc::unwrap_or_clone)
            .unwrap_or_default()
            .into_iter()
    }
}
impl<'a, K, V> IntoIterator for &'a SharedMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = SharedEntries<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<K: Eq + Hash + Borrow<Q>, V, Q: Eq + Hash + ?Sized> Index<&Q> for SharedMap<K, V> {
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key).expect("key must be present")
    }
}

pub struct SharedEntries<'a, K, V>(Option<hash_map::Iter<'a, K, V>>);
impl<'a, K, V> Iterator for SharedEntries<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.as_mut()?.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.as_ref().map_or((0, Some(0)), Iterator::size_hint)
    }
}
impl<K, V> ExactSizeIterator for SharedEntries<'_, K, V> {}

#[cfg(feature = "serde")]
mod serde_impl {
    use super::{Arc, FxHashMap, SharedMap};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::hash::Hash;

    impl<K: Serialize, V: Serialize> Serialize for SharedMap<K, V> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_map(self.iter())
        }
    }
    impl<'de, K: Deserialize<'de> + Eq + Hash, V: Deserialize<'de>> Deserialize<'de>
        for SharedMap<K, V>
    {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let map = FxHashMap::deserialize(deserializer)?;
            Ok(if map.is_empty() {
                SharedMap::default()
            } else {
                SharedMap(Some(Arc::new(map)))
            })
        }
    }
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<SharedMap<u32, u32>>() == 8);

#[cfg(test)]
mod tests;
