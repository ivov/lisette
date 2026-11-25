use crate::target::ir::bodies::{LoweredBlock, LoweredStatement};
use crate::target::ir::go_expression::{GoParameter, Position};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

#[derive(Clone, Default)]
pub(crate) struct Documentation(pub(crate) Vec<String>);

impl Documentation {
    pub(crate) fn of_text(text: &str) -> Self {
        Self(
            text.split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
                .collect(),
        )
    }
}

#[derive(Clone)]
pub(crate) struct TypeParameter {
    pub(crate) name: String,
    pub(crate) bounds: Vec<GoType>,
}

#[derive(Clone)]
pub(crate) struct FunctionSignature {
    pub(crate) receiver: Option<GoParameter>,
    pub(crate) name: String,
    pub(crate) generics: Vec<TypeParameter>,
    pub(crate) parameters: Vec<GoParameter>,
    pub(crate) result: GoType,
}

#[derive(Clone)]
pub(crate) struct FieldDeclaration {
    pub(crate) documentation: Documentation,
    pub(crate) name: Option<String>,
    pub(crate) ty: GoType,
    pub(crate) tag: Option<String>,
}

#[derive(Clone)]
pub(crate) struct InterfaceMethod {
    pub(crate) name: String,
    pub(crate) parameters: Vec<GoType>,
    pub(crate) result: GoType,
}

#[derive(Clone)]
pub(crate) enum DeclarationKind {
    Type {
        name: String,
        parameters: Vec<TypeParameter>,
        underlying: GoType,
    },
    Alias {
        name: String,
        parameters: Vec<TypeParameter>,
        underlying: GoType,
    },
    Struct {
        name: String,
        parameters: Vec<TypeParameter>,
        fields: Vec<FieldDeclaration>,
    },
    Interface {
        name: String,
        parameters: Vec<TypeParameter>,
        parents: Vec<GoType>,
        methods: Vec<InterfaceMethod>,
    },
    Constants(Vec<ConstantDeclaration>),
    Binding(LoweredStatement),
    Function {
        signature: FunctionSignature,
        body: LoweredBlock,
    },
}

#[derive(Clone)]
pub(crate) struct ConstantDeclaration {
    pub(crate) documentation: Documentation,
    pub(crate) name: String,
    pub(crate) ty: Option<GoType>,
    pub(crate) value: Option<GoExpression>,
}

#[derive(Clone)]
pub(crate) struct GoDeclaration {
    pub(crate) documentation: Documentation,
    pub(crate) position: Option<Position>,
    pub(crate) kind: DeclarationKind,
}

impl GoDeclaration {
    pub(crate) fn function(
        receiver: Option<GoParameter>,
        name: impl Into<String>,
        generics: Vec<TypeParameter>,
        parameters: Vec<GoParameter>,
        result: GoType,
        body: LoweredBlock,
    ) -> Self {
        Self::new(DeclarationKind::Function {
            signature: FunctionSignature {
                receiver,
                name: name.into(),
                generics,
                parameters,
                result,
            },
            body,
        })
    }

    pub(crate) fn new(kind: DeclarationKind) -> Self {
        Self {
            documentation: Documentation::default(),
            position: None,
            kind,
        }
    }
}
