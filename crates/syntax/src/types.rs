use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use ecow::EcoString;

/// Extract the unqualified name from a dot-qualified identifier.
///
/// `"prelude.Option"` → `"Option"`, `"**nominal.int"` → `"int"`, `"foo"` → `"foo"`
pub fn unqualified_name(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// type param name -> type variable
pub type SubstitutionMap = HashMap<EcoString, Type>;

pub fn substitute(ty: &Type, map: &HashMap<EcoString, Type>) -> Type {
    if map.is_empty() {
        return ty.clone();
    }
    match ty {
        Type::Parameter(name) => map.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Constructor {
            id,
            params,
            underlying_ty: underlying,
        } => Type::Constructor {
            id: id.clone(),
            params: params.iter().map(|p| substitute(p, map)).collect(),
            underlying_ty: underlying.as_ref().map(|u| Box::new(substitute(u, map))),
        },
        Type::Function {
            params,
            param_mutability,
            bounds,
            return_type,
        } => Type::Function {
            params: params.iter().map(|p| substitute(p, map)).collect(),
            param_mutability: param_mutability.clone(),
            bounds: bounds
                .iter()
                .map(|b| Bound {
                    param_name: b.param_name.clone(),
                    generic: substitute(&b.generic, map),
                    ty: substitute(&b.ty, map),
                })
                .collect(),
            return_type: Box::new(substitute(return_type, map)),
        },
        Type::Variable(_) | Type::Error => ty.clone(),
        Type::Forall { vars, body } => {
            let has_overlap = map.keys().any(|k| vars.contains(k));
            let substituted_body = if has_overlap {
                let filtered_map: HashMap<EcoString, Type> = map
                    .iter()
                    .filter(|(k, _)| !vars.contains(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                substitute(body, &filtered_map)
            } else {
                substitute(body, map)
            };
            Type::Forall {
                vars: vars.clone(),
                body: Box::new(substituted_body),
            }
        }
        Type::Tuple(elements) => Type::Tuple(elements.iter().map(|e| substitute(e, map)).collect()),
        Type::Never => ty.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bound {
    pub param_name: EcoString,
    pub generic: Type,
    pub ty: Type,
}

#[derive(Clone)]
pub enum TypeVariableState {
    Unbound { id: i32, hint: Option<EcoString> },
    Link(Type),
}

impl TypeVariableState {
    pub fn is_unbound(&self) -> bool {
        matches!(self, TypeVariableState::Unbound { .. })
    }
}

impl std::fmt::Debug for TypeVariableState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeVariableState::Unbound { id, hint } => match hint {
                Some(name) => write!(f, "{}", name),
                None => write!(f, "{}", id),
            },
            TypeVariableState::Link(ty) => write!(f, "{:?}", ty),
        }
    }
}

impl PartialEq for TypeVariableState {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                TypeVariableState::Unbound { id: id1, .. },
                TypeVariableState::Unbound { id: id2, .. },
            ) => id1 == id2,
            (TypeVariableState::Link(ty1), TypeVariableState::Link(ty2)) => ty1 == ty2,
            _ => false,
        }
    }
}

#[derive(Clone)]
pub enum Type {
    Constructor {
        id: EcoString,
        params: Vec<Type>,
        underlying_ty: Option<Box<Type>>,
    },

    Function {
        params: Vec<Type>,
        param_mutability: Vec<bool>,
        bounds: Vec<Bound>,
        return_type: Box<Type>,
    },

    Variable(Rc<RefCell<TypeVariableState>>),

    Forall {
        vars: Vec<EcoString>,
        body: Box<Type>,
    },

    Parameter(EcoString),

    Never,

    Tuple(Vec<Type>),

    /// Poison type returned after an error has been reported.
    /// Unifies with everything silently, preventing cascading diagnostics.
    Error,
}

impl std::fmt::Debug for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Constructor { id, params, .. } => f
                .debug_struct("Constructor")
                .field("id", id)
                .field("params", params)
                .finish(),
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                let mut s = f.debug_struct("Function");
                s.field("params", params);
                if param_mutability.iter().any(|m| *m) {
                    s.field("param_mutability", param_mutability);
                }
                s.field("bounds", bounds)
                    .field("return_type", return_type)
                    .finish()
            }
            Type::Variable(type_var) => f
                .debug_tuple("Variable")
                .field(&*type_var.borrow())
                .finish(),
            Type::Forall { vars, body } => f
                .debug_struct("Forall")
                .field("vars", vars)
                .field("body", body)
                .finish(),
            Type::Parameter(name) => f.debug_tuple("Parameter").field(name).finish(),
            Type::Never => write!(f, "Never"),
            Type::Tuple(elements) => f.debug_tuple("Tuple").field(elements).finish(),
            Type::Error => write!(f, "Error"),
        }
    }
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Type::Constructor {
                    id: id1,
                    params: params1,
                    ..
                },
                Type::Constructor {
                    id: id2,
                    params: params2,
                    ..
                },
            ) => id1 == id2 && params1 == params2,
            (
                Type::Function {
                    params: p1,
                    param_mutability: m1,
                    bounds: b1,
                    return_type: r1,
                },
                Type::Function {
                    params: p2,
                    param_mutability: m2,
                    bounds: b2,
                    return_type: r2,
                },
            ) => p1 == p2 && m1 == m2 && b1 == b2 && r1 == r2,
            (Type::Variable(v1), Type::Variable(v2)) => {
                Rc::ptr_eq(v1, v2) || *v1.borrow() == *v2.borrow()
            }
            (
                Type::Forall {
                    vars: vars1,
                    body: body1,
                },
                Type::Forall {
                    vars: vars2,
                    body: body2,
                },
            ) => vars1 == vars2 && body1 == body2,
            (Type::Parameter(name1), Type::Parameter(name2)) => name1 == name2,
            (Type::Never, Type::Never) => true,
            (Type::Tuple(elems1), Type::Tuple(elems2)) => elems1 == elems2,
            _ => false,
        }
    }
}

thread_local! {
    static INTERNED_INT: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_STRING: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_BOOL: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_UNIT: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_FLOAT64: OnceCell<Type> = const { OnceCell::new() };
    static INTERNED_RUNE: OnceCell<Type> = const { OnceCell::new() };
}

impl Type {
    pub fn int() -> Type {
        INTERNED_INT.with(|cell| cell.get_or_init(|| Self::nominal("int")).clone())
    }

    pub fn string() -> Type {
        INTERNED_STRING.with(|cell| cell.get_or_init(|| Self::nominal("string")).clone())
    }

    pub fn bool() -> Type {
        INTERNED_BOOL.with(|cell| cell.get_or_init(|| Self::nominal("bool")).clone())
    }

    pub fn unit() -> Type {
        INTERNED_UNIT.with(|cell| cell.get_or_init(|| Self::nominal("Unit")).clone())
    }

    pub fn float64() -> Type {
        INTERNED_FLOAT64.with(|cell| cell.get_or_init(|| Self::nominal("float64")).clone())
    }

    pub fn rune() -> Type {
        INTERNED_RUNE.with(|cell| cell.get_or_init(|| Self::nominal("rune")).clone())
    }
}

impl Type {
    const UNINFERRED_ID: i32 = -1;
    const IGNORED_ID: i32 = -333;

    pub fn nominal(name: &str) -> Self {
        Self::Constructor {
            id: format!("**nominal.{}", name).into(),
            params: vec![],
            underlying_ty: None,
        }
    }

    pub fn uninferred() -> Self {
        Self::Variable(Rc::new(RefCell::new(TypeVariableState::Unbound {
            id: Self::UNINFERRED_ID,
            hint: None,
        })))
    }

    pub fn ignored() -> Self {
        Self::Variable(Rc::new(RefCell::new(TypeVariableState::Unbound {
            id: Self::IGNORED_ID,
            hint: None,
        })))
    }

    pub fn get_type_params(&self) -> Option<&[Type]> {
        match self {
            Type::Constructor { params, .. } => Some(params),
            _ => None,
        }
    }
}

const ARITHMETIC_TYPES: &[&str] = &[
    "byte",
    "complex128",
    "complex64",
    "float32",
    "float64",
    "int",
    "int16",
    "int32",
    "int64",
    "int8",
    "rune",
    "uint",
    "uint16",
    "uint32",
    "uint64",
    "uint8",
];

const ORDERED_TYPES: &[&str] = &[
    "byte", "float32", "float64", "int", "int16", "int32", "int64", "int8", "rune", "uint",
    "uint16", "uint32", "uint64", "uint8",
];

const UNSIGNED_INT_TYPES: &[&str] = &["byte", "uint", "uint8", "uint16", "uint32", "uint64"];

impl Type {
    pub fn get_function_ret(&self) -> Option<&Type> {
        match self {
            Type::Function { return_type, .. } => Some(return_type),
            _ => None,
        }
    }

    pub fn has_name(&self, name: &str) -> bool {
        match self {
            Type::Constructor { id, .. } => unqualified_name(id) == name,
            _ => false,
        }
    }

    pub fn get_qualified_id(&self) -> Option<&str> {
        match self {
            Type::Constructor { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }

    pub fn get_underlying(&self) -> Option<&Type> {
        match self {
            Type::Constructor {
                underlying_ty: underlying,
                ..
            } => underlying.as_deref(),
            _ => None,
        }
    }

    pub fn is_result(&self) -> bool {
        self.has_name("Result")
    }

    pub fn is_option(&self) -> bool {
        self.has_name("Option")
    }

    pub fn is_partial(&self) -> bool {
        self.has_name("Partial")
    }

    pub fn is_unit(&self) -> bool {
        matches!(self.resolve(), Type::Constructor { ref id, .. } if id.as_ref() == "**nominal.Unit")
    }

    pub fn tuple_arity(&self) -> Option<usize> {
        match self {
            Type::Tuple(elements) => Some(elements.len()),
            _ => None,
        }
    }

    pub fn is_tuple(&self) -> bool {
        matches!(self, Type::Tuple(_))
    }

    pub fn is_ref(&self) -> bool {
        self.has_name("Ref")
    }

    pub fn is_receiver_placeholder(&self) -> bool {
        self.has_name("__receiver__")
    }

    pub fn is_unknown(&self) -> bool {
        self.has_name("Unknown")
    }

    pub fn is_receiver(&self) -> bool {
        self.has_name("Receiver")
    }

    pub fn is_ignored(&self) -> bool {
        match self {
            Type::Variable(var) => {
                matches!(&*var.borrow(), TypeVariableState::Unbound { id, .. } if *id == Self::IGNORED_ID)
            }
            _ => false,
        }
    }

    pub fn is_variadic(&self) -> Option<Type> {
        let args = self.get_function_params()?;
        let last = args.last()?;

        if last.get_name()? == "VarArgs" {
            return last.inner();
        }

        None
    }

    pub fn is_string(&self) -> bool {
        self.has_name("string")
    }

    pub fn is_slice_of(&self, element_name: &str) -> bool {
        match self {
            Type::Constructor { id, params, .. } => {
                if unqualified_name(id) != "Slice" || params.len() != 1 {
                    return false;
                }
                params[0].resolve().has_name(element_name)
            }
            _ => false,
        }
    }

    pub fn is_byte_slice(&self) -> bool {
        self.is_slice_of("byte") || self.is_slice_of("uint8")
    }

    pub fn is_rune_slice(&self) -> bool {
        self.is_slice_of("rune")
    }

    pub fn is_byte_or_rune_slice(&self) -> bool {
        self.is_byte_slice() || self.is_rune_slice()
    }

    pub fn has_byte_or_rune_slice_underlying(&self) -> bool {
        if self.is_byte_or_rune_slice() {
            return true;
        }
        match self {
            Type::Constructor { underlying_ty, .. } => underlying_ty
                .as_deref()
                .is_some_and(|u| u.has_byte_or_rune_slice_underlying()),
            _ => false,
        }
    }

    pub fn is_boolean(&self) -> bool {
        self.has_name("bool")
    }

    pub fn is_rune(&self) -> bool {
        self.has_name("rune")
    }

    pub fn is_float64(&self) -> bool {
        self.has_name("float64")
    }

    pub fn is_float32(&self) -> bool {
        self.has_name("float32")
    }

    pub fn is_float(&self) -> bool {
        self.is_float64() || self.is_float32()
    }

    pub fn is_variable(&self) -> bool {
        matches!(self, Type::Variable(_))
    }

    pub fn is_unbound_variable(&self) -> bool {
        matches!(self, Type::Variable(cell) if cell.borrow().is_unbound())
    }

    pub fn is_numeric(&self) -> bool {
        match self {
            Type::Constructor { id, .. } => ARITHMETIC_TYPES.contains(&unqualified_name(id)),
            _ => false,
        }
    }

    pub fn is_ordered(&self) -> bool {
        match self {
            Type::Constructor { id, .. } => ORDERED_TYPES.contains(&unqualified_name(id)),
            _ => false,
        }
    }

    pub fn is_complex(&self) -> bool {
        match self {
            Type::Constructor { id, .. } => {
                matches!(unqualified_name(id), "complex128" | "complex64")
            }
            _ => false,
        }
    }

    pub fn is_unsigned_int(&self) -> bool {
        match self {
            Type::Constructor { id, .. } => UNSIGNED_INT_TYPES.contains(&unqualified_name(id)),
            _ => false,
        }
    }

    pub fn is_never(&self) -> bool {
        matches!(self.shallow_resolve(), Type::Never)
    }

    pub fn is_error(&self) -> bool {
        matches!(self.shallow_resolve(), Type::Error)
    }

    pub fn has_unbound_variables(&self) -> bool {
        match self {
            Type::Variable(type_var) => match &*type_var.borrow() {
                TypeVariableState::Unbound { hint, .. } => hint.is_some(),
                TypeVariableState::Link(ty) => ty.has_unbound_variables(),
            },
            Type::Constructor { params, .. } => params.iter().any(|p| p.has_unbound_variables()),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.has_unbound_variables())
                    || return_type.has_unbound_variables()
            }
            Type::Forall { body, .. } => body.has_unbound_variables(),
            Type::Tuple(elements) => elements.iter().any(|e| e.has_unbound_variables()),
            Type::Parameter(_) | Type::Never | Type::Error => false,
        }
    }

    pub fn remove_found_type_names(&self, names: &mut HashSet<EcoString>) {
        if names.is_empty() {
            return;
        }

        match self {
            Type::Constructor { id, params, .. } => {
                names.remove(unqualified_name(id));
                for param in params {
                    param.remove_found_type_names(names);
                }
            }
            Type::Function {
                params,
                return_type,
                bounds,
                ..
            } => {
                for param in params {
                    param.remove_found_type_names(names);
                }
                return_type.remove_found_type_names(names);
                for bound in bounds {
                    bound.generic.remove_found_type_names(names);
                    bound.ty.remove_found_type_names(names);
                }
            }
            Type::Forall { body, .. } => {
                body.remove_found_type_names(names);
            }
            Type::Variable(type_var) => {
                if let TypeVariableState::Link(ty) = &*type_var.borrow() {
                    ty.remove_found_type_names(names);
                }
            }
            Type::Parameter(name) => {
                names.remove(name);
            }
            Type::Tuple(elements) => {
                for element in elements {
                    element.remove_found_type_names(names);
                }
            }
            Type::Never | Type::Error => {}
        }
    }
}

impl Type {
    pub fn get_name(&self) -> Option<&str> {
        match self {
            Type::Constructor { id, params, .. } => {
                let name = unqualified_name(id);
                if name == "Ref" {
                    return params.first().and_then(|inner| inner.get_name());
                }
                if let Some(module_path) = id.strip_prefix("@import/") {
                    let path = module_path.strip_prefix("go:").unwrap_or(module_path);
                    return path.rsplit('/').next();
                }
                Some(name)
            }
            _ => None,
        }
    }

    pub fn wraps(&self, name: &str, inner: &Type) -> bool {
        self.get_name().is_some_and(|n| n == name)
            && self
                .get_type_params()
                .and_then(|p| p.first())
                .is_some_and(|first| *first == *inner)
    }

    pub fn get_function_params(&self) -> Option<&[Type]> {
        match self {
            Type::Function { params, .. } => Some(params),
            Type::Constructor {
                underlying_ty: Some(inner),
                ..
            } => inner.get_function_params(),
            _ => None,
        }
    }

    pub fn param_count(&self) -> usize {
        match self {
            Type::Function { params, .. } => params.len(),
            _ => 0,
        }
    }

    pub fn get_param_mutability(&self) -> &[bool] {
        match self {
            Type::Function {
                param_mutability, ..
            } => param_mutability,
            _ => &[],
        }
    }

    pub fn with_replaced_first_param(&self, new_first: &Type) -> Type {
        match self {
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                if params.is_empty() {
                    return self.clone();
                }
                let mut new_params = params.clone();
                new_params[0] = new_first.clone();
                Type::Function {
                    params: new_params,
                    param_mutability: param_mutability.clone(),
                    bounds: bounds.clone(),
                    return_type: return_type.clone(),
                }
            }
            Type::Forall { vars, body } => Type::Forall {
                vars: vars.clone(),
                body: Box::new(body.with_replaced_first_param(new_first)),
            },
            _ => self.clone(),
        }
    }

    pub fn get_bounds(&self) -> &[Bound] {
        match self {
            Type::Function { bounds, .. } => bounds,
            Type::Forall { body, .. } => body.get_bounds(),
            _ => &[],
        }
    }

    pub fn get_qualified_name(&self) -> EcoString {
        match self.strip_refs() {
            Type::Constructor { id, .. } => id,
            _ => panic!("called get_qualified_name on {:#?}", self),
        }
    }

    pub fn inner(&self) -> Option<Type> {
        self.get_type_params()
            .and_then(|args| args.first().cloned())
    }

    pub fn ok_type(&self) -> Type {
        debug_assert!(
            self.is_result() || self.is_option() || self.is_partial(),
            "ok_type called on non-Result/Option/Partial type"
        );
        self.inner()
            .expect("Result/Option/Partial should have inner type")
    }

    pub fn err_type(&self) -> Type {
        debug_assert!(
            self.is_result() || self.is_partial(),
            "err_type called on non-Result/Partial type"
        );
        self.get_type_params()
            .and_then(|args| args.get(1).cloned())
            .expect("Result/Partial should have error type")
    }
}

impl Type {
    pub fn unwrap_forall(&self) -> &Type {
        match self {
            Type::Forall { body, .. } => body.as_ref(),
            other => other,
        }
    }

    pub fn strip_refs(&self) -> Type {
        if self.is_ref() {
            return self.inner().expect("ref type must have inner").strip_refs();
        }

        self.clone()
    }

    pub fn with_receiver_placeholder(self) -> Type {
        match self {
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => {
                let mut new_params = vec![Type::nominal("__receiver__")];
                new_params.extend(params);

                let mut new_mutability = vec![false];
                new_mutability.extend(param_mutability);

                Type::Function {
                    params: new_params,
                    param_mutability: new_mutability,
                    bounds,
                    return_type,
                }
            }
            _ => unreachable!(
                "with_receiver_placeholder called on non-function type: {:?}",
                self
            ),
        }
    }

    pub fn remove_vars(types: &[&Type]) -> (Vec<Type>, Vec<EcoString>) {
        let mut vars = HashMap::default();
        let types = types
            .iter()
            .map(|v| Self::remove_vars_impl(v, &mut vars))
            .collect();

        (types, vars.into_values().collect())
    }

    fn remove_vars_impl(ty: &Type, vars: &mut HashMap<i32, EcoString>) -> Type {
        match ty {
            Type::Constructor {
                id: name,
                params: args,
                underlying_ty: underlying,
            } => Type::Constructor {
                id: name.clone(),
                params: args
                    .iter()
                    .map(|a| Self::remove_vars_impl(a, vars))
                    .collect(),
                underlying_ty: underlying
                    .as_ref()
                    .map(|u| Box::new(Self::remove_vars_impl(u, vars))),
            },

            Type::Function {
                params: args,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: args
                    .iter()
                    .map(|a| Self::remove_vars_impl(a, vars))
                    .collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds
                    .iter()
                    .map(|b| Bound {
                        param_name: b.param_name.clone(),
                        generic: Self::remove_vars_impl(&b.generic, vars),
                        ty: Self::remove_vars_impl(&b.ty, vars),
                    })
                    .collect(),
                return_type: Self::remove_vars_impl(return_type, vars).into(),
            },

            Type::Variable(type_var) => match &*type_var.borrow() {
                TypeVariableState::Unbound { id, hint } => match vars.get(id) {
                    Some(g) => Self::nominal(g),
                    None => {
                        let name: EcoString = hint.clone().unwrap_or_else(|| {
                            char::from_digit(
                                (vars.len() + 10)
                                    .try_into()
                                    .expect("type var count fits in u32"),
                                16,
                            )
                            .expect("type var index is valid hex digit")
                            .to_uppercase()
                            .to_string()
                            .into()
                        });

                        vars.insert(*id, name.clone());
                        Self::nominal(&name)
                    }
                },
                TypeVariableState::Link(ty) => Self::remove_vars_impl(ty, vars),
            },

            Type::Forall { body, .. } => Self::remove_vars_impl(body, vars),
            Type::Tuple(elements) => Type::Tuple(
                elements
                    .iter()
                    .map(|e| Self::remove_vars_impl(e, vars))
                    .collect(),
            ),
            Type::Parameter(name) => Type::Parameter(name.clone()),
            Type::Never | Type::Error => ty.clone(),
        }
    }

    pub fn contains_type(&self, target: &Type) -> bool {
        if *self == *target {
            return true;
        }
        match self {
            Type::Constructor { params, .. } => params.iter().any(|p| p.contains_type(target)),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.contains_type(target)) || return_type.contains_type(target)
            }
            Type::Variable(var) => {
                if let TypeVariableState::Link(linked) = &*var.borrow() {
                    linked.contains_type(target)
                } else {
                    false
                }
            }
            Type::Forall { body, .. } => body.contains_type(target),
            Type::Tuple(elements) => elements.iter().any(|e| e.contains_type(target)),
            Type::Parameter(_) | Type::Never | Type::Error => false,
        }
    }

    /// Follow Variable::Link chains to the outermost non-variable type.
    /// Does NOT recurse into Constructor params, Function params, etc.
    /// Use this when you only need the outermost type (e.g. is_never, is_unknown, has_name).
    pub fn shallow_resolve(&self) -> Type {
        match self {
            Type::Variable(type_var) => {
                let state = type_var.borrow();
                match &*state {
                    TypeVariableState::Unbound { .. } => self.clone(),
                    TypeVariableState::Link(linked) => linked.shallow_resolve(),
                }
            }
            _ => self.clone(),
        }
    }

    pub fn resolve(&self) -> Type {
        match self {
            Type::Variable(type_var) => {
                let state = type_var.borrow();
                match &*state {
                    TypeVariableState::Unbound { .. } => self.clone(),
                    TypeVariableState::Link(linked) => {
                        let resolved = linked.resolve();
                        drop(state);
                        *type_var.borrow_mut() = TypeVariableState::Link(resolved.clone());
                        resolved
                    }
                }
            }
            Type::Constructor {
                id,
                params,
                underlying_ty: underlying,
            } => Type::Constructor {
                id: id.clone(),
                params: params.iter().map(|p| p.resolve()).collect(),
                underlying_ty: underlying.as_ref().map(|u| Box::new(u.resolve())),
            },
            Type::Function {
                params,
                param_mutability,
                bounds,
                return_type,
            } => Type::Function {
                params: params.iter().map(|p| p.resolve()).collect(),
                param_mutability: param_mutability.clone(),
                bounds: bounds
                    .iter()
                    .map(|b| Bound {
                        param_name: b.param_name.clone(),
                        generic: b.generic.resolve(),
                        ty: b.ty.resolve(),
                    })
                    .collect(),
                return_type: Box::new(return_type.resolve()),
            },
            Type::Forall { .. } => {
                unreachable!("Forall types are always instantiated before resolve")
            }
            Type::Tuple(elements) => Type::Tuple(elements.iter().map(|e| e.resolve()).collect()),
            Type::Parameter(_) | Type::Error => self.clone(),
            Type::Never => Type::Never,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericFamily {
    SignedInt,
    UnsignedInt,
    Float,
}

const SIGNED_INT_TYPES: &[&str] = &["int", "int8", "int16", "int32", "int64", "rune"];
const FLOAT_TYPES: &[&str] = &["float32", "float64"];

impl Type {
    pub fn underlying_numeric_type(&self) -> Option<Type> {
        self.underlying_numeric_type_recursive(&mut HashSet::default())
    }

    pub fn has_underlying_numeric_type(&self) -> bool {
        self.underlying_numeric_type().is_some()
    }

    fn underlying_numeric_type_recursive(&self, visited: &mut HashSet<EcoString>) -> Option<Type> {
        match self {
            Type::Constructor {
                id,
                underlying_ty: underlying,
                ..
            } => {
                if self.is_numeric() {
                    return Some(self.clone());
                }

                if !visited.insert(id.clone()) {
                    return None;
                }

                underlying
                    .as_ref()?
                    .underlying_numeric_type_recursive(visited)
            }
            _ => None,
        }
    }

    pub fn numeric_family(&self) -> Option<NumericFamily> {
        let name = match self {
            Type::Constructor { id, .. } => unqualified_name(id),
            _ => return None,
        };

        if SIGNED_INT_TYPES.contains(&name) {
            Some(NumericFamily::SignedInt)
        } else if UNSIGNED_INT_TYPES.contains(&name) {
            Some(NumericFamily::UnsignedInt)
        } else if FLOAT_TYPES.contains(&name) {
            Some(NumericFamily::Float)
        } else {
            None
        }
    }

    pub fn is_numeric_compatible_with(&self, other: &Type) -> bool {
        let self_underlying_ty = self.underlying_numeric_type();
        let other_underlying_ty = other.underlying_numeric_type();

        match (self_underlying_ty, other_underlying_ty) {
            (Some(s), Some(o)) => s.numeric_family() == o.numeric_family(),
            _ => false,
        }
    }

    pub fn is_aliased_numeric_type(&self) -> bool {
        match self {
            Type::Constructor { underlying_ty, .. } => {
                underlying_ty.is_some() && !self.is_numeric()
            }
            _ => false,
        }
    }
}
