use syntax::types::Type;

pub fn int_type() -> Type {
    Type::int()
}

pub fn int8_type() -> Type {
    Type::nominal("int8")
}

pub fn int16_type() -> Type {
    Type::nominal("int16")
}

pub fn float32_type() -> Type {
    Type::nominal("float32")
}

pub fn bool_type() -> Type {
    Type::bool()
}

pub fn string_type() -> Type {
    Type::string()
}

pub fn float_type() -> Type {
    Type::float64()
}

pub fn rune_type() -> Type {
    Type::rune()
}

pub fn unit_type() -> Type {
    Type::unit()
}

pub fn slice_type(inner: Type) -> Type {
    Type::Constructor {
        id: "**nominal.Slice".into(),
        params: vec![inner],
        underlying_ty: None,
    }
}

pub fn ref_type(inner: Type) -> Type {
    Type::Constructor {
        id: "**nominal.Ref".into(),
        params: vec![inner],
        underlying_ty: None,
    }
}

pub fn tuple_type(types: Vec<Type>) -> Type {
    Type::Tuple(types)
}

pub fn con_type(name: &str, args: Vec<Type>) -> Type {
    Type::Constructor {
        id: format!("**nominal.{}", name).into(),
        params: args,
        underlying_ty: None,
    }
}

pub fn fun_type(args: Vec<Type>, ret: Type) -> Type {
    Type::Function {
        param_mutability: vec![false; args.len()],
        params: args,
        bounds: vec![],
        return_type: Box::new(ret),
    }
}
