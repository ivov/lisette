use syntax::types::{CompoundKind, SimpleKind, Type};

pub fn int_type() -> Type {
    Type::Simple(SimpleKind::Int)
}

pub fn int8_type() -> Type {
    Type::Simple(SimpleKind::Int8)
}

pub fn int16_type() -> Type {
    Type::Simple(SimpleKind::Int16)
}

pub fn float32_type() -> Type {
    Type::Simple(SimpleKind::Float32)
}

pub fn bool_type() -> Type {
    Type::Simple(SimpleKind::Bool)
}

pub fn string_type() -> Type {
    Type::Simple(SimpleKind::String)
}

pub fn float_type() -> Type {
    Type::Simple(SimpleKind::Float64)
}

pub fn rune_type() -> Type {
    Type::Simple(SimpleKind::Rune)
}

pub fn byte_type() -> Type {
    Type::Simple(SimpleKind::Byte)
}

pub fn unit_type() -> Type {
    Type::Simple(SimpleKind::Unit)
}

pub fn slice_type(inner: Type) -> Type {
    Type::Compound {
        kind: CompoundKind::Slice,
        args: vec![inner],
    }
}

pub fn ref_type(inner: Type) -> Type {
    Type::Compound {
        kind: CompoundKind::Ref,
        args: vec![inner],
    }
}

pub fn tuple_type(types: Vec<Type>) -> Type {
    Type::Tuple(types)
}

pub fn array_type(length: u64, element: Type) -> Type {
    Type::Array {
        length,
        element: Box::new(element),
    }
}

pub fn con_type(name: &str, args: Vec<Type>) -> Type {
    Type::Nominal {
        id: format!("_entry_.{}", name).into(),
        params: args,
    }
}

pub fn fun_type(args: Vec<Type>, ret: Type) -> Type {
    Type::function(
        args.into_iter()
            .map(|ty| syntax::types::FunctionParameter::new(ty, false))
            .collect(),
        vec![],
        Box::new(ret),
    )
}
