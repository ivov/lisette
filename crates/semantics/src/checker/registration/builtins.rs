use syntax::EcoString;
use syntax::types::Type;

use crate::checker::Checker;

impl Checker<'_, '_> {
    fn builtin_qualified_name(&mut self, type_name: &str) -> EcoString {
        self.lookup_qualified_name(type_name)
            .map(EcoString::from)
            .unwrap_or_else(|| panic!("Builtin type {type_name} not found in store"))
    }

    fn builtin_type(&mut self, type_name: &str) -> Type {
        if let Some(ty) = self.builtins.get(type_name) {
            return ty.clone();
        }

        let qualified_name = self.builtin_qualified_name(type_name);

        let ty = self
            .store
            .get_type(&qualified_name)
            .unwrap_or_else(|| panic!("Builtin type {type_name} not found in store"));

        let body = match &ty {
            Type::Forall { body, .. } => body.as_ref().clone(),
            _ => ty.clone(),
        };

        self.builtins.insert(type_name.to_string(), body.clone());

        body
    }

    pub fn type_unit(&self) -> Type {
        Type::unit()
    }

    pub fn type_never(&self) -> Type {
        Type::Never
    }

    pub fn type_int(&mut self) -> Type {
        self.builtin_type("int")
    }

    pub fn type_float(&mut self) -> Type {
        self.builtin_type("float64")
    }

    pub fn type_string(&mut self) -> Type {
        self.builtin_type("string")
    }

    pub fn type_char(&mut self) -> Type {
        self.builtin_type("rune")
    }

    pub fn type_bool(&mut self) -> Type {
        self.builtin_type("bool")
    }

    pub fn type_complex128(&mut self) -> Type {
        self.builtin_type("complex128")
    }

    pub fn type_unknown(&mut self) -> Type {
        self.builtin_type("Unknown")
    }

    pub fn type_slice(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Slice"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub fn type_reference(&mut self, inner_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Ref"),
            params: vec![inner_type],
            underlying_ty: None,
        }
    }

    pub fn type_map(&mut self, key_type: Type, value_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Map"),
            params: vec![key_type, value_type],
            underlying_ty: None,
        }
    }

    pub fn type_result(&mut self, ok_type: Type, error_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Result"),
            params: vec![ok_type, error_type],
            underlying_ty: None,
        }
    }

    pub fn type_option(&mut self, some_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Option"),
            params: vec![some_type],
            underlying_ty: None,
        }
    }

    pub fn type_panic_value(&mut self) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("PanicValue"),
            params: vec![],
            underlying_ty: None,
        }
    }

    pub fn type_range(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("Range"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub fn type_range_inclusive(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("RangeInclusive"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub fn type_range_from(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("RangeFrom"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub fn type_range_to(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("RangeTo"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    pub fn type_range_to_inclusive(&mut self, element_type: Type) -> Type {
        Type::Constructor {
            id: self.builtin_qualified_name("RangeToInclusive"),
            params: vec![element_type],
            underlying_ty: None,
        }
    }

    /// Checks if a type is a generic container (Option, Result) with interface type parameters.
    /// Used to determine when to use the expected type for codegen instead of the inferred type.
    pub fn is_generic_container_with_interface(&self, ty: &Type) -> bool {
        let Type::Constructor { id, params, .. } = ty.resolve() else {
            return false;
        };

        if id != "prelude.Option" && id != "prelude.Result" {
            return false;
        }

        params.iter().any(|p| {
            if let Type::Constructor { id, .. } = p.resolve() {
                self.store.get_interface(&id).is_some()
            } else {
                false
            }
        })
    }

    pub fn has_interface_type_param(&self, ty: &Type) -> bool {
        let Type::Constructor { params, .. } = ty.resolve() else {
            return false;
        };

        params.iter().any(|p| {
            if let Type::Constructor { id, .. } = p.resolve() {
                self.store.get_interface(&id).is_some()
            } else {
                false
            }
        })
    }
}
