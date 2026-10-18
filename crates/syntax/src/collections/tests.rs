use super::*;

#[test]
fn dense_replacement_and_first_wins_insertion_keep_one_record_per_key() {
    let mut map = DenseMap::default();
    map.insert("entry".to_string(), 1);
    assert_eq!(map.insert("entry".to_string(), 2), Some(1));
    map.insert_if_absent("entry".to_string(), 3);
    map.insert_if_absent("other".to_string(), 4);
    *map.get_mut("other").unwrap() += 1;

    assert_eq!(map.len(), 2);
    assert_eq!(map.get("entry"), Some(&2));
    assert_eq!(map.get("other"), Some(&5));
    assert_eq!(map.get("missing"), None);
}

#[test]
fn dense_removal_preserves_sparse_id_lookup_and_allows_reinsertion() {
    let mut map: DenseMap<_, _> = [(1_u32, 10), (100_000, 20), (u32::MAX, 30)]
        .into_iter()
        .collect();

    map.retain(|id, value| {
        *value += 1;
        *id != 1
    });
    assert_eq!(map.get(&1), None);
    assert_eq!(map.get(&100_000), Some(&21));
    assert_eq!(map.get(&u32::MAX), Some(&31));

    *map.get_mut(&u32::MAX).unwrap() = 40;
    map.insert(1, 50);
    map.retain(|id, _| *id != 100_000);
    let mut entries: Vec<_> = map.into_iter().collect();
    entries.sort_unstable();
    assert_eq!(entries, [(1, 50), (u32::MAX, 40)]);
}

#[test]
fn flat_replacement_keeps_one_value_per_name() {
    let mut map = FlatMap::default();
    map.insert("value".to_string(), 1);
    map.insert("other".to_string(), 2);
    assert_eq!(map.insert("value".to_string(), 3), Some(1));
    map.insert_if_absent("value".to_string(), 4);
    *map.get_mut("other").unwrap() = 5;

    assert_eq!(map.get("value"), Some(&3));
    assert_eq!(map.get("missing"), None);
    let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    entries.sort_unstable();
    assert_eq!(entries, [("other", 5), ("value", 3)]);
}

#[test]
fn shared_insertions_do_not_change_an_existing_clone() {
    let original: SharedMap<_, _> = [("method".to_string(), 1)].into_iter().collect();
    let mut copy = original.clone();
    copy.insert_if_absent("method".to_string(), 2);
    assert_eq!(copy.get("method"), Some(&1));
    assert_eq!(copy.insert("method".to_string(), 3), Some(1));
    copy.insert_if_absent("new".to_string(), 4);

    assert_eq!(original.get("method"), Some(&1));
    assert_eq!(original.get("new"), None);
    assert_eq!(copy.get("method"), Some(&3));
    assert_eq!(copy.get("new"), Some(&4));
}

#[test]
fn shared_mutable_lookup_does_not_change_an_existing_clone() {
    let original: SharedMap<_, _> = [("method".to_string(), 1)].into_iter().collect();
    let mut copy = original.clone();
    assert_eq!(copy.get_mut("missing"), None);
    *copy.get_mut("method").unwrap() = 2;

    assert_eq!(original.get("method"), Some(&1));
    assert_eq!(copy.get("method"), Some(&2));
}

#[test]
fn shared_mutable_iteration_does_not_change_an_existing_clone() {
    let original: SharedMap<_, _> = [("first", 1), ("second", 2)].into_iter().collect();
    let mut copy = original.clone();
    for value in copy.values_mut() {
        *value += 10;
    }

    assert_eq!(original["first"], 1);
    assert_eq!(original["second"], 2);
    assert_eq!(copy["first"], 11);
    assert_eq!(copy["second"], 12);
}
