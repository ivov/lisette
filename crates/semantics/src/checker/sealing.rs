//! Method-set keys for Go sealing methods, from bindgen's `#[go(unexported, "<identity>")]`.

use ecow::EcoString;

/// `#` and `:` cannot appear in a Lisette identifier, so this key is unforgeable.
const UNEXPORTED_PREFIX: &str = "#unexported:";

pub(crate) fn unexported_key(id: &str) -> EcoString {
    format!("{UNEXPORTED_PREFIX}{id}").into()
}

pub(crate) fn is_unexported_key(key: &str) -> bool {
    key.starts_with(UNEXPORTED_PREFIX)
}
