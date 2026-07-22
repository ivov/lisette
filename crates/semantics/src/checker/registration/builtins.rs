use crate::checker::EnvResolve;
use syntax::types::{CompoundKind, SimpleKind, Symbol, Type};

use crate::checker::TaskState;
use crate::store::Store;

impl TaskState<'_> {
    fn builtin_qualified_name(&mut self, store: &Store, type_name: &str) -> Symbol {
        self.lookup_qualified_name(store, type_name)
            .map(Symbol::from)
            .unwrap_or_else(|| panic!("Builtin type {type_name} not found in store"))
    }

    pub(crate) fn type_unit(&self) -> Type {
        Type::unit()
    }

    pub(crate) fn type_never(&self) -> Type {
        Type::Never
    }

    pub(crate) fn type_int(&mut self) -> Type {
        Type::Simple(SimpleKind::Int)
    }

    pub(crate) fn type_float(&mut self) -> Type {
        Type::Simple(SimpleKind::Float64)
    }

    pub(crate) fn type_string(&mut self) -> Type {
        Type::Simple(SimpleKind::String)
    }

    pub(crate) fn type_char(&mut self) -> Type {
        Type::Simple(SimpleKind::Rune)
    }

    pub(crate) fn type_bool(&mut self) -> Type {
        Type::Simple(SimpleKind::Bool)
    }

    pub(crate) fn type_complex128(&mut self) -> Type {
        Type::Simple(SimpleKind::Complex128)
    }

    pub(crate) fn type_slice(&mut self, element_type: Type) -> Type {
        Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![element_type],
        }
    }

    pub(crate) fn type_array(&mut self, length: u64, element_type: Type) -> Type {
        Type::Array {
            length,
            element: Box::new(element_type),
        }
    }

    pub(crate) fn type_reference(&mut self, inner_type: Type) -> Type {
        Type::Compound {
            kind: CompoundKind::Ref,
            args: vec![inner_type],
        }
    }

    pub(crate) fn type_map(&mut self, key_type: Type, value_type: Type) -> Type {
        Type::Compound {
            kind: CompoundKind::Map,
            args: vec![key_type, value_type],
        }
    }

    pub(crate) fn type_result(&mut self, store: &Store, ok_type: Type, error_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "Result"),
            params: vec![ok_type, error_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_option(&mut self, store: &Store, some_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "Option"),
            params: vec![some_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_panic_value(&mut self, store: &Store) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "PanicValue"),
            params: vec![],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_range(&mut self, store: &Store, element_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "Range"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_range_inclusive(&mut self, store: &Store, element_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "RangeInclusive"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_range_from(&mut self, store: &Store, element_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "RangeFrom"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_range_to(&mut self, store: &Store, element_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "RangeTo"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub(crate) fn type_range_to_inclusive(&mut self, store: &Store, element_type: Type) -> Type {
        Type::Nominal {
            id: self.builtin_qualified_name(store, "RangeToInclusive"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    /// Checks if a type is a generic container (Option, Result) whose
    /// type parameter needs the expected type to flow through for
    /// codegen: Go interfaces (for method-set satisfaction) or
    /// Go-imported named types (for Go generic instantiation preserving
    /// alias names like `tea.Cmd` instead of collapsing to the
    /// underlying `func() Msg`).
    pub(crate) fn is_generic_container_with_interface(&self, store: &Store, ty: &Type) -> bool {
        let resolved = ty.resolve_in(&self.env);
        let Type::Nominal { id, params, .. } = &resolved else {
            return false;
        };

        if id != "prelude.Option" && id != "prelude.Result" {
            return false;
        }

        params.iter().any(|p| {
            if let Type::Nominal { id, .. } = p.resolve_in(&self.env) {
                store.get_interface(&id).is_some() || id.starts_with("go:")
            } else {
                false
            }
        })
    }
}
