use rustc_hash::FxHashMap as HashMap;
use rustc_hash::FxHashSet as HashSet;

#[derive(Default)]
pub(crate) struct PackageState {
    escape_remap: HashMap<String, String>,
    generic_renames: HashMap<String, String>,
    go_const_bindings: HashSet<String>,
    /// Go names the package's own items declare at package scope.
    declared_names: HashSet<String>,
    /// Qualifiers the package's imports bind in the Go package block.
    import_qualifiers: HashSet<String>,
}

impl PackageState {
    pub(crate) fn record_escape_remap(
        &mut self,
        lisette_name: impl Into<String>,
        go_name: impl Into<String>,
    ) {
        self.escape_remap
            .insert(lisette_name.into(), go_name.into());
    }

    pub(crate) fn escape_remap(&self, lisette_name: &str) -> Option<&str> {
        self.escape_remap.get(lisette_name).map(String::as_str)
    }

    pub(crate) fn record_generic_rename(
        &mut self,
        source_name: impl Into<String>,
        go_name: impl Into<String>,
    ) {
        self.generic_renames
            .insert(source_name.into(), go_name.into());
    }

    pub(crate) fn generic_rename(&self, source_name: &str) -> Option<&str> {
        self.generic_renames.get(source_name).map(String::as_str)
    }

    pub(crate) fn record_package_block_names(
        &mut self,
        declared_names: HashSet<String>,
        import_qualifiers: HashSet<String>,
    ) {
        self.declared_names = declared_names;
        self.import_qualifiers = import_qualifiers;
    }

    pub(crate) fn declares_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name)
    }

    /// Names declared in the Go package block, which no local may take.
    pub(crate) fn is_package_block_name(&self, go_name: &str) -> bool {
        self.declared_names.contains(go_name) || self.import_qualifiers.contains(go_name)
    }

    pub(crate) fn is_import_qualifier(&self, go_name: &str) -> bool {
        self.import_qualifiers.contains(go_name)
    }

    pub(crate) fn package_block_names(&self) -> impl Iterator<Item = &String> {
        self.declared_names.iter().chain(&self.import_qualifiers)
    }

    pub(crate) fn extend_go_const_bindings(&mut self, names: impl IntoIterator<Item = String>) {
        self.go_const_bindings.extend(names);
    }

    pub(crate) fn is_go_const_binding(&self, symbol: &str) -> bool {
        self.go_const_bindings.contains(symbol)
    }
}
