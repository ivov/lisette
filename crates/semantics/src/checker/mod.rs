mod context;
pub mod freeze;
pub mod infer;
pub mod promotion;
pub(crate) mod registration;
mod resolution;
pub(crate) mod scopes;
mod sealing;
mod state;
mod type_env;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use crate::facts::{BindingIdAllocator, Facts};
use crate::store::Store;
use diagnostics::LocalSink;
use ecow::EcoString;
use registration::derived_attributes::EqualityAttributes;
use scopes::Scopes;
use syntax::ast::{Annotation, Expression, Generic, ImportAlias, Span};
use syntax::program::{
    Definition, DefinitionBody, FileImport, Methods, NativeTypeKind, Package,
    go_import_default_name,
};
use syntax::types::{Bound, SubstitutionMap, Symbol, Type, substitute};

pub(crate) use context::FileContext;
pub use infer::expressions::comparison::{check_never_comparable, check_not_comparable};
pub use registration::RegisteredPackage;
pub(crate) use registration::UnregisteredPackage;
pub use state::{Cursor, TaskState};
pub(crate) use state::{InferredFile, TaskOutput};
pub use type_env::{EnvResolve, TypeEnv, VarState};

impl TaskState {
    fn is_d_lis(&self, store: &Store) -> bool {
        let Some(file_id) = self.cursor.file_id() else {
            return false;
        };

        let Some(package) = store.get_package(self.cursor.package_id()) else {
            return false;
        };

        package.is_typedef(file_id)
    }

    fn is_lis(&self, store: &Store) -> bool {
        !self.is_d_lis(store)
    }

    fn current_package<'a>(&self, store: &'a Store) -> &'a Package {
        store
            .get_package(self.cursor.package_id())
            .expect("current package must exist in store")
    }

    fn current_package_mut<'a>(&self, store: &'a mut Store) -> &'a mut Package {
        store
            .get_package_mut(self.cursor.package_id())
            .expect("current package must exist in store")
    }

    fn qualify_name(&self, name: &str) -> Symbol {
        Symbol::from_parts(self.cursor.package_id(), name)
    }

    pub(crate) fn put_in_scope(&mut self, generics: &[Generic]) {
        for (index, generic) in generics.iter().enumerate() {
            self.scopes
                .insert_type_param(self.qualify_name(&generic.name), index);
        }
    }

    pub(crate) fn resolve_generic_bounds(
        &mut self,
        store: &Store,
        generics: &mut [Generic],
        span: &Span,
    ) {
        for generic in &mut *generics {
            if !generic.bounds_are_resolved() {
                generic.resolve_bounds_with(|bound| {
                    self.register_bound_annotation(store, bound, span)
                });
            }
        }
        self.record_resolved_generic_bounds(generics);
    }

    fn record_resolved_generic_bounds(&mut self, generics: &[Generic]) {
        for generic in generics {
            for bound in generic
                .resolved_bounds()
                .expect("generic bounds were resolved before recording")
            {
                self.record_generic_bound(&generic.name, bound.clone());
            }
        }
    }

    fn ensure_generic_bounds(
        &mut self,
        store: &Store,
        mut generics: Vec<Generic>,
        span: &Span,
    ) -> Vec<Generic> {
        self.resolve_generic_bounds(store, &mut generics, span);
        generics
    }

    fn record_generic_bound(&mut self, parameter: &str, bound: Type) {
        self.scopes.insert_trait_bound(parameter, bound);
    }

    fn parameter_satisfies_bound(&self, parameter: &str, target: infer::BuiltinBound) -> bool {
        let mut found = false;
        self.scopes.for_each_bound_on_param(parameter, |bound_ty| {
            if found {
                return;
            }
            if let Some(declared) = bound_ty
                .resolve_in(&self.env)
                .get_qualified_id()
                .and_then(infer::BuiltinBound::from_qualified_id)
                && declared.satisfies(target)
            {
                found = true;
            }
        });
        found
    }

    fn register_bound_annotation(
        &mut self,
        store: &Store,
        bound: &Annotation,
        span: &Span,
    ) -> Type {
        let resolved = self.convert_bound_to_type(store, bound, span);
        if self.is_lis(store) && store.contains_unknown(&resolved) {
            self.sink
                .push(diagnostics::infer::unknown_in_bound_position(
                    bound.get_span(),
                ));
        }
        resolved
    }
}

pub(crate) fn resolved_generic_bounds(generics: &[Generic]) -> Vec<Bound> {
    generics
        .iter()
        .flat_map(|generic| {
            generic
                .resolved_bounds()
                .expect("generic bounds must be resolved before use")
                .cloned()
                .map(|ty| Bound {
                    param_name: generic.name.clone(),
                    generic: Type::Parameter(generic.name.clone()),
                    ty,
                })
        })
        .collect()
}
