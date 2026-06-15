//! Method-set keys for Go unexported (sealing) methods, keyed by bindgen's
//! `#[go(unexported, "<identity>")]` so structural satisfaction enforces the seal.

use ecow::EcoString;

/// `#` and `:` cannot appear in a Lisette identifier, so this key is unforgeable.
const UNEXPORTED_PREFIX: &str = "#unexported:";

pub fn unexported_key(id: &str) -> EcoString {
    format!("{UNEXPORTED_PREFIX}{id}").into()
}

pub fn is_unexported_key(key: &str) -> bool {
    key.starts_with(UNEXPORTED_PREFIX)
}
