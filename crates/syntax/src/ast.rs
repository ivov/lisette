use ecow::EcoString;

use crate::program::{CallKind, DotAccessKind, ReceiverCoercion};
use crate::types::Type;

const CHILDREN_INLINE_CAP: usize = 4;

pub struct Children<'a> {
    inline: [Option<&'a Expression>; CHILDREN_INLINE_CAP],
    inline_len: usize,
    heap: Vec<&'a Expression>,
}

impl<'a> Children<'a> {
    pub fn new() -> Self {
        Children {
            inline: [None; CHILDREN_INLINE_CAP],
            inline_len: 0,
            heap: Vec::new(),
        }
    }

    pub fn push(&mut self, expression: &'a Expression) {
        if self.heap.is_empty() && self.inline_len < CHILDREN_INLINE_CAP {
            self.inline[self.inline_len] = Some(expression);
            self.inline_len += 1;
        } else {
            self.heap.push(expression);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a Expression> + '_ {
        self.inline[..self.inline_len]
            .iter()
            .filter_map(|slot| *slot)
            .chain(self.heap.iter().copied())
    }
}

impl Default for Children<'_> {
    fn default() -> Self {
        Children::new()
    }
}

impl<'a> Extend<&'a Expression> for Children<'a> {
    fn extend<T: IntoIterator<Item = &'a Expression>>(&mut self, iter: T) {
        for expression in iter {
            self.push(expression);
        }
    }
}

impl<'a> FromIterator<&'a Expression> for Children<'a> {
    fn from_iter<T: IntoIterator<Item = &'a Expression>>(iter: T) -> Self {
        let mut children = Children::new();
        children.extend(iter);
        children
    }
}

pub struct ChildrenIntoIter<'a> {
    inline: [Option<&'a Expression>; CHILDREN_INLINE_CAP],
    pos: usize,
    inline_len: usize,
    heap: std::vec::IntoIter<&'a Expression>,
}

impl<'a> Iterator for ChildrenIntoIter<'a> {
    type Item = &'a Expression;

    fn next(&mut self) -> Option<&'a Expression> {
        if self.pos < self.inline_len {
            let item = self.inline[self.pos];
            self.pos += 1;
            item
        } else {
            self.heap.next()
        }
    }
}

impl<'a> IntoIterator for Children<'a> {
    type Item = &'a Expression;
    type IntoIter = ChildrenIntoIter<'a>;

    fn into_iter(self) -> ChildrenIntoIter<'a> {
        ChildrenIntoIter {
            inline: self.inline,
            pos: 0,
            inline_len: self.inline_len,
            heap: self.heap.into_iter(),
        }
    }
}

macro_rules! children {
    () => { Children::new() };
    ($($expression:expr),+ $(,)?) => {{
        let mut __children = Children::new();
        $( __children.push($expression); )+
        __children
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadCodeCause {
    Return,
    Break,
    Continue,
    DivergingIf,
    DivergingMatch,
    InfiniteLoop,
    DivergingCall,
}

#[derive(Clone, PartialEq)]
pub struct Binding {
    pub pattern: Pattern,
    pub annotation: Option<Annotation>,
    pub typed_pattern: Option<TypedPattern>,
    pub ty: Type,
    pub mutable: bool,
}

impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Binding");
        s.field("pattern", &self.pattern);
        s.field("annotation", &self.annotation);
        s.field("typed_pattern", &self.typed_pattern);
        s.field("ty", &self.ty);
        if self.mutable {
            s.field("mutable", &self.mutable);
        }
        s.finish()
    }
}

pub type BindingId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Let { mutable: bool },
    Parameter { mutable: bool },
    MatchArm,
    IfLet,
    WhileLet,
}

impl BindingKind {
    pub fn is_mutable(&self) -> bool {
        matches!(
            self,
            BindingKind::Let { mutable: true } | BindingKind::Parameter { mutable: true }
        )
    }

    pub fn is_param(&self) -> bool {
        matches!(self, BindingKind::Parameter { .. })
    }

    pub fn is_match_arm(&self) -> bool {
        matches!(self, BindingKind::MatchArm)
    }

    pub fn is_pattern_position(&self) -> bool {
        matches!(
            self,
            BindingKind::MatchArm | BindingKind::IfLet | BindingKind::WhileLet
        )
    }
}

#[derive(Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expression>>,
    pub typed_pattern: Option<TypedPattern>,
    pub expression: Box<Expression>,
}

impl MatchArm {
    pub fn has_guard(&self) -> bool {
        self.guard.is_some()
    }
}

impl std::fmt::Debug for MatchArm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("MatchArm");
        s.field("pattern", &self.pattern);
        if self.guard.is_some() {
            s.field("guard", &self.guard);
        }
        s.field("expression", &self.expression);
        s.finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOrigin {
    Explicit,
    IfLet { else_span: Option<Span> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectArm {
    pub pattern: SelectArmPattern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectArmPattern {
    Receive {
        binding: Box<Pattern>,
        typed_pattern: Option<TypedPattern>,
        receive_expression: Box<Expression>,
        body: Box<Expression>,
    },
    Send {
        send_expression: Box<Expression>,
        body: Box<Expression>,
    },
    MatchReceive {
        receive_expression: Box<Expression>,
        arms: Vec<MatchArm>,
    },
    WildCard {
        body: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RestPattern {
    Absent,
    Discard(Span),
    Bind { name: EcoString, span: Span },
}

impl RestPattern {
    pub fn is_present(&self) -> bool {
        !matches!(self, RestPattern::Absent)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Literal {
        literal: Literal,
        ty: Type,
        span: Span,
    },
    Unit {
        ty: Type,
        span: Span,
    },
    EnumVariant {
        identifier: EcoString,
        fields: Vec<Self>,
        rest: bool,
        ty: Type,
        span: Span,
    },
    Struct {
        identifier: EcoString,
        fields: Vec<StructFieldPattern>,
        rest: bool,
        ty: Type,
        span: Span,
    },
    Tuple {
        elements: Vec<Self>,
        span: Span,
    },
    WildCard {
        span: Span,
    },
    Identifier {
        identifier: EcoString,
        span: Span,
    },
    Slice {
        prefix: Vec<Self>,
        rest: RestPattern,
        element_ty: Type,
        span: Span,
    },
    Or {
        patterns: Vec<Self>,
        span: Span,
    },
    AsBinding {
        pattern: Box<Self>,
        name: EcoString,
        span: Span,
    },
}

/// Dataless variant index of [`Pattern`], usable as an array index.
#[derive(Clone, Copy)]
pub enum PatternKind {
    Literal,
    Unit,
    EnumVariant,
    Struct,
    Tuple,
    WildCard,
    Identifier,
    Slice,
    Or,
    AsBinding,
}

impl PatternKind {
    // Relies on `AsBinding` staying the last variant.
    pub const COUNT: usize = PatternKind::AsBinding as usize + 1;
}

impl Pattern {
    pub fn kind(&self) -> PatternKind {
        match self {
            Pattern::Literal { .. } => PatternKind::Literal,
            Pattern::Unit { .. } => PatternKind::Unit,
            Pattern::EnumVariant { .. } => PatternKind::EnumVariant,
            Pattern::Struct { .. } => PatternKind::Struct,
            Pattern::Tuple { .. } => PatternKind::Tuple,
            Pattern::WildCard { .. } => PatternKind::WildCard,
            Pattern::Identifier { .. } => PatternKind::Identifier,
            Pattern::Slice { .. } => PatternKind::Slice,
            Pattern::Or { .. } => PatternKind::Or,
            Pattern::AsBinding { .. } => PatternKind::AsBinding,
        }
    }

    pub fn get_span(&self) -> Span {
        match self {
            Pattern::Identifier { span, .. } => *span,
            Pattern::Literal { span, .. } => *span,
            Pattern::EnumVariant { span, .. } => *span,
            Pattern::Struct { span, .. } => *span,
            Pattern::WildCard { span } => *span,
            Pattern::Unit { span, .. } => *span,
            Pattern::Tuple { span, .. } => *span,
            Pattern::Slice { span, .. } => *span,
            Pattern::Or { span, .. } => *span,
            Pattern::AsBinding { span, .. } => *span,
        }
    }

    pub fn get_type(&self) -> Option<Type> {
        match self {
            Pattern::Identifier { .. } => None,
            Pattern::Literal { ty, .. } => Some(ty.clone()),
            Pattern::EnumVariant { ty, .. } => Some(ty.clone()),
            Pattern::Struct { ty, .. } => Some(ty.clone()),
            Pattern::WildCard { .. } => None,
            Pattern::Unit { ty, .. } => Some(ty.clone()),
            Pattern::Tuple { .. } => None,
            Pattern::Slice { .. } => None,
            Pattern::Or { .. } => None,
            Pattern::AsBinding { pattern, .. } => pattern.get_type(),
        }
    }

    pub fn is_identifier(&self) -> bool {
        matches!(self, Pattern::Identifier { .. } | Pattern::AsBinding { .. })
    }

    pub fn get_identifier(&self) -> Option<EcoString> {
        match self {
            Pattern::Identifier { identifier, .. } => Some(identifier.clone()),
            Pattern::AsBinding { name, .. } => Some(name.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldPattern {
    pub name: EcoString,
    pub value: Pattern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedPattern {
    Wildcard,
    Literal(Literal),
    /// A qualified const pattern (e.g. `time.Friday`). A value comparison, not
    /// an enum constructor: `qualified_name` is the resolved constant, `value`
    /// is its known case-eligible literal when available.
    Const {
        qualified_name: EcoString,
        ty: Type,
        value: Option<Literal>,
    },
    EnumVariant {
        enum_name: EcoString,
        variant_name: EcoString,
        variant_fields: Vec<EnumFieldDefinition>,
        fields: Vec<TypedPattern>,
        type_args: Vec<Type>,
        field_types: Box<[Type]>,
    },
    EnumStructVariant {
        enum_name: EcoString,
        variant_name: EcoString,
        variant_fields: Vec<EnumFieldDefinition>,
        pattern_fields: Vec<(EcoString, TypedPattern)>,
        type_args: Vec<Type>,
    },
    Struct {
        struct_name: EcoString,
        struct_fields: Vec<StructFieldDefinition>,
        pattern_fields: Vec<(EcoString, TypedPattern)>,
        type_args: Vec<Type>,
    },
    Slice {
        prefix: Vec<TypedPattern>,
        has_rest: bool,
        element_type: Type,
    },
    Tuple {
        arity: usize,
        elements: Vec<TypedPattern>,
    },
    Or {
        alternatives: Vec<TypedPattern>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDefinition {
    pub name: EcoString,
    pub name_span: Span,
    pub generics: Vec<Generic>,
    pub params: Vec<Binding>,
    pub body: Box<Expression>,
    pub return_type: Type,
    pub annotation: Annotation,
    pub ty: Type,
}

#[derive(Clone, Copy)]
pub struct FunctionDefinitionView<'a> {
    pub name: &'a EcoString,
    pub name_span: Span,
    pub generics: &'a [Generic],
    pub params: &'a [Binding],
    pub body: &'a Expression,
    pub return_type: &'a Type,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantFields {
    Unit,
    Tuple(Vec<EnumFieldDefinition>),
    Struct(Vec<EnumFieldDefinition>),
}

impl VariantFields {
    pub fn is_empty(&self) -> bool {
        match self {
            VariantFields::Unit => true,
            VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields.is_empty(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            VariantFields::Unit => 0,
            VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields.len(),
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, EnumFieldDefinition> {
        match self {
            VariantFields::Unit => [].iter(),
            VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields.iter(),
        }
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, VariantFields::Struct(_))
    }
}

impl<'a> IntoIterator for &'a VariantFields {
    type Item = &'a EnumFieldDefinition;
    type IntoIter = std::slice::Iter<'a, EnumFieldDefinition>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub doc: Option<String>,
    pub name: EcoString,
    pub name_span: Span,
    pub fields: VariantFields,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumFieldDefinition {
    pub name: EcoString,
    pub name_span: Span,
    pub annotation: Annotation,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<AttributeArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AttributeArg {
    /// A flag option, e.g., `omitempty`, `skip`, `snake_case`
    Flag(String),
    /// A negated flag, e.g., `!omitempty`
    NegatedFlag(String),
    /// A quoted string, e.g., `"custom_name"` (name override)
    String(String),
    /// A raw backtick literal, e.g., `json:"name,string"`
    Raw(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StructKind {
    Record,
    Tuple,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldDefinition {
    pub doc: Option<String>,
    pub attributes: Vec<Attribute>,
    pub name: EcoString,
    pub name_span: Span,
    pub annotation: Annotation,
    pub visibility: Visibility,
    pub ty: Type,
    pub embedded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructFieldAssignment {
    pub name: EcoString,
    pub name_span: Span,
    pub value: Box<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructSpread {
    None,
    From(Box<Expression>),
    ZeroFill { span: Span },
}

impl StructSpread {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn is_some(&self) -> bool {
        !self.is_none()
    }

    pub fn span(&self) -> Option<Span> {
        match self {
            Self::None => None,
            Self::From(e) => Some(e.get_span()),
            Self::ZeroFill { span } => Some(*span),
        }
    }

    pub fn as_expression(&self) -> Option<&Expression> {
        match self {
            Self::From(e) => Some(e),
            Self::None | Self::ZeroFill { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Annotation {
    Constructor {
        name: EcoString,
        params: Vec<Self>,
        span: Span,
    },
    Function {
        params: Vec<Self>,
        return_type: Box<Self>,
        span: Span,
    },
    Tuple {
        elements: Vec<Self>,
        span: Span,
    },
    Unknown,
    Opaque {
        span: Span,
    },
}

impl Annotation {
    pub fn unit() -> Self {
        Self::Constructor {
            name: "Unit".into(),
            params: vec![],
            span: Span::dummy(),
        }
    }

    pub fn get_span(&self) -> Span {
        match self {
            Self::Constructor { span, .. } => *span,
            Self::Function { span, .. } => *span,
            Self::Tuple { span, .. } => *span,
            Self::Opaque { span } => *span,
            Self::Unknown => Span::dummy(),
        }
    }

    pub fn get_name(&self) -> Option<String> {
        match self {
            Self::Constructor { name, .. } => Some(name.to_string()),
            _ => None,
        }
    }

    pub fn is_unit(&self) -> bool {
        matches!(self, Self::Constructor { name, params, .. } if name == "Unit" && params.is_empty())
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub fn is_opaque(&self) -> bool {
        matches!(self, Self::Opaque { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Generic {
    pub name: EcoString,
    pub bounds: Vec<Annotation>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    pub file_id: u32,
    pub byte_offset: u32,
    pub byte_length: u32,
}

impl Span {
    pub fn new(file_id: u32, byte_offset: u32, byte_length: u32) -> Self {
        Span {
            file_id,
            byte_offset,
            byte_length,
        }
    }

    pub fn dummy() -> Self {
        Span {
            file_id: 0,
            byte_offset: 0,
            byte_length: 0,
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.byte_length == 0
    }

    pub fn end(&self) -> u32 {
        self.byte_offset + self.byte_length
    }

    pub fn merge(self, other: Span) -> Span {
        Span::new(
            self.file_id,
            self.byte_offset,
            other.end() - self.byte_offset,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Expression {
    Literal {
        literal: Literal,
        ty: Type,
        span: Span,
    },
    Function {
        doc: Option<String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        params: Vec<Binding>,
        return_annotation: Annotation,
        return_type: Type,
        visibility: Visibility,
        body: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Lambda {
        params: Vec<Binding>,
        return_annotation: Annotation,
        body: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Block {
        items: Vec<Expression>,
        ty: Type,
        span: Span,
    },
    Let {
        binding: Box<Binding>,
        value: Box<Expression>,
        mutable: bool,
        mut_span: Option<Span>,
        else_block: Option<Box<Expression>>,
        else_span: Option<Span>,
        typed_pattern: Option<TypedPattern>,
        ty: Type,
        span: Span,
    },
    Identifier {
        value: EcoString,
        ty: Type,
        span: Span,
        binding_id: Option<BindingId>,
        qualified: Option<EcoString>,
    },
    Call {
        expression: Box<Expression>,
        args: Vec<Expression>,
        spread: Box<Option<Expression>>,
        type_args: Vec<Annotation>,
        ty: Type,
        span: Span,
        call_kind: Option<CallKind>,
    },
    If {
        condition: Box<Expression>,
        consequence: Box<Expression>,
        alternative: Box<Expression>,
        ty: Type,
        span: Span,
    },
    IfLet {
        pattern: Pattern,
        scrutinee: Box<Expression>,
        consequence: Box<Expression>,
        alternative: Box<Expression>,
        typed_pattern: Option<TypedPattern>,
        else_span: Option<Span>,
        ty: Type,
        span: Span,
    },
    Match {
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
        origin: MatchOrigin,
        ty: Type,
        span: Span,
    },
    Tuple {
        elements: Vec<Expression>,
        ty: Type,
        span: Span,
    },
    StructCall {
        name: EcoString,
        field_assignments: Vec<StructFieldAssignment>,
        spread: StructSpread,
        ty: Type,
        span: Span,
    },
    DotAccess {
        expression: Box<Expression>,
        member: EcoString,
        ty: Type,
        span: Span,
        dot_access_kind: Option<DotAccessKind>,
        receiver_coercion: Option<ReceiverCoercion>,
    },
    Assignment {
        target: Box<Expression>,
        value: Box<Expression>,
        compound_operator: Option<BinaryOperator>,
        span: Span,
    },
    Return {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Propagate {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    TryBlock {
        items: Vec<Expression>,
        ty: Type,
        try_keyword_span: Span,
        span: Span,
    },
    RecoverBlock {
        items: Vec<Expression>,
        ty: Type,
        recover_keyword_span: Span,
        span: Span,
    },
    ImplBlock {
        annotation: Annotation,
        receiver_name: EcoString,
        methods: Vec<Expression>,
        generics: Vec<Generic>,
        ty: Type,
        span: Span,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Unary {
        operator: UnaryOperator,
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Paren {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Const {
        doc: Option<String>,
        identifier: EcoString,
        identifier_span: Span,
        annotation: Option<Annotation>,
        expression: Box<Expression>,
        visibility: Visibility,
        ty: Type,
        span: Span,
    },
    VariableDeclaration {
        doc: Option<String>,
        name: EcoString,
        name_span: Span,
        annotation: Annotation,
        visibility: Visibility,
        ty: Type,
        span: Span,
    },
    RawGo {
        text: String,
    },
    Loop {
        body: Box<Expression>,
        ty: Type,
        span: Span,
        needs_label: bool,
    },
    While {
        condition: Box<Expression>,
        body: Box<Expression>,
        span: Span,
        needs_label: bool,
    },
    WhileLet {
        pattern: Pattern,
        scrutinee: Box<Expression>,
        body: Box<Expression>,
        typed_pattern: Option<TypedPattern>,
        span: Span,
        needs_label: bool,
    },
    For {
        binding: Box<Binding>,
        iterable: Box<Expression>,
        body: Box<Expression>,
        span: Span,
        needs_label: bool,
        binding_id: Option<BindingId>,
    },
    Break {
        value: Option<Box<Expression>>,
        span: Span,
    },
    Continue {
        span: Span,
    },
    Enum {
        doc: Option<String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        variants: Vec<EnumVariant>,
        visibility: Visibility,
        span: Span,
    },
    Struct {
        doc: Option<String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        fields: Vec<StructFieldDefinition>,
        kind: StructKind,
        visibility: Visibility,
        span: Span,
    },
    TypeAlias {
        doc: Option<String>,
        attributes: Vec<Attribute>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        annotation: Annotation,
        ty: Type,
        visibility: Visibility,
        span: Span,
    },
    ModuleImport {
        name: EcoString,
        name_span: Span,
        alias: Option<ImportAlias>,
        span: Span,
    },
    Reference {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Interface {
        doc: Option<String>,
        name: EcoString,
        name_span: Span,
        generics: Vec<Generic>,
        parents: Vec<ParentInterface>,
        method_signatures: Vec<Expression>,
        visibility: Visibility,
        span: Span,
    },
    IndexedAccess {
        expression: Box<Expression>,
        index: Box<Expression>,
        ty: Type,
        span: Span,
        from_colon_syntax: bool,
    },
    Task {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Defer {
        expression: Box<Expression>,
        ty: Type,
        span: Span,
    },
    Select {
        arms: Vec<SelectArm>,
        ty: Type,
        span: Span,
    },
    Unit {
        ty: Type,
        span: Span,
    },
    Range {
        start: Option<Box<Expression>>,
        end: Option<Box<Expression>>,
        inclusive: bool,
        ty: Type,
        span: Span,
    },
    Cast {
        expression: Box<Expression>,
        target_type: Annotation,
        ty: Type,
        span: Span,
    },
    NoOp,
}

/// Dataless variant index of [`Expression`], usable as an array index.
#[derive(Clone, Copy)]
pub enum ExpressionKind {
    Literal,
    Function,
    Lambda,
    Block,
    Let,
    Identifier,
    Call,
    If,
    IfLet,
    Match,
    Tuple,
    StructCall,
    DotAccess,
    Assignment,
    Return,
    Propagate,
    TryBlock,
    RecoverBlock,
    ImplBlock,
    Binary,
    Unary,
    Paren,
    Const,
    VariableDeclaration,
    RawGo,
    Loop,
    While,
    WhileLet,
    For,
    Break,
    Continue,
    Enum,
    Struct,
    TypeAlias,
    ModuleImport,
    Reference,
    Interface,
    IndexedAccess,
    Task,
    Defer,
    Select,
    Unit,
    Range,
    Cast,
    NoOp,
}

impl ExpressionKind {
    // Relies on `NoOp` staying the last variant.
    pub const COUNT: usize = ExpressionKind::NoOp as usize + 1;
}

impl Expression {
    pub fn kind(&self) -> ExpressionKind {
        match self {
            Expression::Literal { .. } => ExpressionKind::Literal,
            Expression::Function { .. } => ExpressionKind::Function,
            Expression::Lambda { .. } => ExpressionKind::Lambda,
            Expression::Block { .. } => ExpressionKind::Block,
            Expression::Let { .. } => ExpressionKind::Let,
            Expression::Identifier { .. } => ExpressionKind::Identifier,
            Expression::Call { .. } => ExpressionKind::Call,
            Expression::If { .. } => ExpressionKind::If,
            Expression::IfLet { .. } => ExpressionKind::IfLet,
            Expression::Match { .. } => ExpressionKind::Match,
            Expression::Tuple { .. } => ExpressionKind::Tuple,
            Expression::StructCall { .. } => ExpressionKind::StructCall,
            Expression::DotAccess { .. } => ExpressionKind::DotAccess,
            Expression::Assignment { .. } => ExpressionKind::Assignment,
            Expression::Return { .. } => ExpressionKind::Return,
            Expression::Propagate { .. } => ExpressionKind::Propagate,
            Expression::TryBlock { .. } => ExpressionKind::TryBlock,
            Expression::RecoverBlock { .. } => ExpressionKind::RecoverBlock,
            Expression::ImplBlock { .. } => ExpressionKind::ImplBlock,
            Expression::Binary { .. } => ExpressionKind::Binary,
            Expression::Unary { .. } => ExpressionKind::Unary,
            Expression::Paren { .. } => ExpressionKind::Paren,
            Expression::Const { .. } => ExpressionKind::Const,
            Expression::VariableDeclaration { .. } => ExpressionKind::VariableDeclaration,
            Expression::RawGo { .. } => ExpressionKind::RawGo,
            Expression::Loop { .. } => ExpressionKind::Loop,
            Expression::While { .. } => ExpressionKind::While,
            Expression::WhileLet { .. } => ExpressionKind::WhileLet,
            Expression::For { .. } => ExpressionKind::For,
            Expression::Break { .. } => ExpressionKind::Break,
            Expression::Continue { .. } => ExpressionKind::Continue,
            Expression::Enum { .. } => ExpressionKind::Enum,
            Expression::Struct { .. } => ExpressionKind::Struct,
            Expression::TypeAlias { .. } => ExpressionKind::TypeAlias,
            Expression::ModuleImport { .. } => ExpressionKind::ModuleImport,
            Expression::Reference { .. } => ExpressionKind::Reference,
            Expression::Interface { .. } => ExpressionKind::Interface,
            Expression::IndexedAccess { .. } => ExpressionKind::IndexedAccess,
            Expression::Task { .. } => ExpressionKind::Task,
            Expression::Defer { .. } => ExpressionKind::Defer,
            Expression::Select { .. } => ExpressionKind::Select,
            Expression::Unit { .. } => ExpressionKind::Unit,
            Expression::Range { .. } => ExpressionKind::Range,
            Expression::Cast { .. } => ExpressionKind::Cast,
            Expression::NoOp => ExpressionKind::NoOp,
        }
    }

    pub fn is_noop(&self) -> bool {
        matches!(self, Expression::NoOp)
    }

    pub fn is_block(&self) -> bool {
        matches!(self, Expression::Block { .. })
    }

    pub fn is_range(&self) -> bool {
        matches!(self, Expression::Range { .. })
    }

    pub fn is_conditional(&self) -> bool {
        matches!(
            self,
            Expression::If { .. }
                | Expression::IfLet { .. }
                | Expression::Match {
                    origin: MatchOrigin::IfLet { .. },
                    ..
                }
        )
    }

    pub fn is_control_flow(&self) -> bool {
        matches!(
            self,
            Expression::If { .. }
                | Expression::Match { .. }
                | Expression::Select { .. }
                | Expression::For { .. }
                | Expression::While { .. }
                | Expression::WhileLet { .. }
                | Expression::Loop { .. }
        )
    }

    pub fn is_temp_producing(&self) -> bool {
        matches!(
            self.unwrap_parens(),
            Expression::If { .. }
                | Expression::IfLet { .. }
                | Expression::Match { .. }
                | Expression::Block { .. }
                | Expression::Loop { .. }
                | Expression::Select { .. }
                | Expression::TryBlock { .. }
                | Expression::RecoverBlock { .. }
        )
    }

    pub fn callee_name(&self) -> Option<String> {
        let Expression::Call { expression, .. } = self else {
            return None;
        };
        match expression.as_ref() {
            Expression::Identifier { value, .. } => Some(value.to_string()),
            Expression::DotAccess {
                expression: base,
                member,
                ..
            } => {
                if let Expression::Identifier { value, .. } = base.as_ref() {
                    Some(format!("{}.{}", value, member))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn to_function_signature(&self) -> FunctionDefinition {
        match self {
            Expression::Function {
                name,
                name_span,
                generics,
                params,
                return_annotation,
                return_type,
                ty,
                ..
            } => FunctionDefinition {
                name: name.clone(),
                name_span: *name_span,
                generics: generics.clone(),
                params: params.clone(),
                body: Box::new(Expression::NoOp),
                return_type: return_type.clone(),
                annotation: return_annotation.clone(),
                ty: ty.clone(),
            },
            _ => panic!("to_function_signature called on non-Function expression"),
        }
    }

    pub fn function_definition_view(&self) -> FunctionDefinitionView<'_> {
        match self {
            Expression::Function {
                name,
                name_span,
                generics,
                params,
                return_type,
                body,
                ..
            } => FunctionDefinitionView {
                name,
                name_span: *name_span,
                generics,
                params,
                body,
                return_type,
            },
            _ => panic!("function_definition_view called on non-Function expression"),
        }
    }

    pub fn as_option_constructor(&self) -> Option<std::result::Result<(), ()>> {
        let variant = match self {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            _ => None,
        }?;

        match variant {
            "Option.Some" | "Some" => Some(Ok(())),
            "Option.None" | "None" => Some(Err(())),
            _ => None,
        }
    }

    pub fn is_none_literal(&self) -> bool {
        matches!(self.as_option_constructor(), Some(Err(())))
    }

    pub fn as_result_constructor(&self) -> Option<std::result::Result<(), ()>> {
        let variant = match self {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            _ => None,
        }?;

        match variant {
            "Result.Ok" | "Ok" => Some(Ok(())),
            "Result.Err" | "Err" => Some(Err(())),
            _ => None,
        }
    }

    pub fn as_partial_constructor(&self) -> Option<&'static str> {
        let variant = match self {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            _ => None,
        }?;

        match variant {
            "Partial.Ok" => Some("Ok"),
            "Partial.Err" => Some("Err"),
            "Partial.Both" => Some("Both"),
            _ => None,
        }
    }

    pub fn get_type(&self) -> Type {
        match self {
            Self::Literal { ty, .. }
            | Self::Function { ty, .. }
            | Self::Lambda { ty, .. }
            | Self::Block { ty, .. }
            | Self::Let { ty, .. }
            | Self::Identifier { ty, .. }
            | Self::Call { ty, .. }
            | Self::If { ty, .. }
            | Self::IfLet { ty, .. }
            | Self::Match { ty, .. }
            | Self::Tuple { ty, .. }
            | Self::StructCall { ty, .. }
            | Self::DotAccess { ty, .. }
            | Self::Return { ty, .. }
            | Self::Propagate { ty, .. }
            | Self::TryBlock { ty, .. }
            | Self::RecoverBlock { ty, .. }
            | Self::Binary { ty, .. }
            | Self::Paren { ty, .. }
            | Self::Unary { ty, .. }
            | Self::Const { ty, .. }
            | Self::VariableDeclaration { ty, .. }
            | Self::Defer { ty, .. }
            | Self::Reference { ty, .. }
            | Self::IndexedAccess { ty, .. }
            | Self::Task { ty, .. }
            | Self::Select { ty, .. }
            | Self::Unit { ty, .. }
            | Self::Loop { ty, .. }
            | Self::Range { ty, .. }
            | Self::Cast { ty, .. } => ty.clone(),
            Self::Enum { .. }
            | Self::Struct { .. }
            | Self::Assignment { .. }
            | Self::ImplBlock { .. }
            | Self::TypeAlias { .. }
            | Self::ModuleImport { .. }
            | Self::Interface { .. }
            | Self::NoOp
            | Self::RawGo { .. }
            | Self::While { .. }
            | Self::WhileLet { .. }
            | Self::For { .. } => Type::ignored(),
            Self::Break { .. } | Self::Continue { .. } => Type::Never,
        }
    }

    pub fn get_span(&self) -> Span {
        match self {
            Self::Literal { span, .. }
            | Self::Function { span, .. }
            | Self::Lambda { span, .. }
            | Self::Block { span, .. }
            | Self::Let { span, .. }
            | Self::Identifier { span, .. }
            | Self::Call { span, .. }
            | Self::If { span, .. }
            | Self::IfLet { span, .. }
            | Self::Match { span, .. }
            | Self::Tuple { span, .. }
            | Self::Enum { span, .. }
            | Self::Struct { span, .. }
            | Self::StructCall { span, .. }
            | Self::DotAccess { span, .. }
            | Self::Assignment { span, .. }
            | Self::Return { span, .. }
            | Self::Propagate { span, .. }
            | Self::TryBlock { span, .. }
            | Self::RecoverBlock { span, .. }
            | Self::ImplBlock { span, .. }
            | Self::Binary { span, .. }
            | Self::Paren { span, .. }
            | Self::Unary { span, .. }
            | Self::Const { span, .. }
            | Self::VariableDeclaration { span, .. }
            | Self::Defer { span, .. }
            | Self::Reference { span, .. }
            | Self::IndexedAccess { span, .. }
            | Self::Task { span, .. }
            | Self::Select { span, .. }
            | Self::Loop { span, .. }
            | Self::TypeAlias { span, .. }
            | Self::ModuleImport { span, .. }
            | Self::Interface { span, .. }
            | Self::Unit { span, .. }
            | Self::While { span, .. }
            | Self::WhileLet { span, .. }
            | Self::For { span, .. }
            | Self::Break { span, .. }
            | Self::Continue { span, .. }
            | Self::Range { span, .. }
            | Self::Cast { span, .. } => *span,
            Self::NoOp | Self::RawGo { .. } => Span::dummy(),
        }
    }

    pub fn contains_break(&self) -> bool {
        match self {
            Expression::Break { .. } => true,

            Expression::Loop { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. } => false,

            Expression::Block { items, .. } => items.iter().any(Self::contains_break),

            Expression::TryBlock { items, .. } => items.iter().any(Self::contains_break),
            Expression::RecoverBlock { items, .. } => items.iter().any(Self::contains_break),

            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                condition.contains_break()
                    || consequence.contains_break()
                    || alternative.contains_break()
            }

            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                scrutinee.contains_break()
                    || consequence.contains_break()
                    || alternative.contains_break()
            }

            Expression::Match { subject, arms, .. } => {
                subject.contains_break() || arms.iter().any(|arm| arm.expression.contains_break())
            }

            Expression::Paren { expression, .. } => expression.contains_break(),

            Expression::Binary { left, right, .. } => {
                left.contains_break() || right.contains_break()
            }

            Expression::Unary { expression, .. } => expression.contains_break(),

            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                expression.contains_break()
                    || args.iter().any(Self::contains_break)
                    || spread.as_ref().as_ref().is_some_and(Self::contains_break)
            }

            Expression::Function { .. } | Expression::Lambda { .. } => false,

            Expression::Select { arms, .. } => arms.iter().any(|arm| match &arm.pattern {
                SelectArmPattern::Receive { body, .. } => body.contains_break(),
                SelectArmPattern::Send { body, .. } => body.contains_break(),
                SelectArmPattern::MatchReceive { arms, .. } => {
                    arms.iter().any(|a| a.expression.contains_break())
                }
                SelectArmPattern::WildCard { body } => body.contains_break(),
            }),

            Expression::Cast { expression, .. } => expression.contains_break(),

            Expression::Let {
                value, else_block, ..
            } => value.contains_break() || else_block.as_ref().is_some_and(|e| e.contains_break()),

            Expression::Assignment { value, .. } => value.contains_break(),

            _ => false,
        }
    }

    pub fn diverges(&self) -> Option<DeadCodeCause> {
        match self {
            Expression::Return { .. } => Some(DeadCodeCause::Return),
            Expression::Break { .. } => Some(DeadCodeCause::Break),
            Expression::Continue { .. } => Some(DeadCodeCause::Continue),

            Expression::If {
                consequence,
                alternative,
                ..
            } => {
                if consequence.diverges().is_some() && alternative.diverges().is_some() {
                    Some(DeadCodeCause::DivergingIf)
                } else {
                    None
                }
            }

            Expression::IfLet {
                consequence,
                alternative,
                ..
            } => {
                if consequence.diverges().is_some() && alternative.diverges().is_some() {
                    Some(DeadCodeCause::DivergingIf)
                } else {
                    None
                }
            }

            Expression::Match { arms, .. } => {
                if !arms.is_empty() && arms.iter().all(|arm| arm.expression.diverges().is_some()) {
                    Some(DeadCodeCause::DivergingMatch)
                } else {
                    None
                }
            }

            Expression::Block { items, .. } => {
                for item in items {
                    if let Some(cause) = item.diverges() {
                        return Some(cause);
                    }
                }
                None
            }

            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                for item in items {
                    if let Some(cause) = item.diverges() {
                        return Some(cause);
                    }
                }
                None
            }

            Expression::Paren { expression, .. } | Expression::Cast { expression, .. } => {
                expression.diverges()
            }

            Expression::Loop { body, .. } => {
                if !body.contains_break() {
                    Some(DeadCodeCause::InfiniteLoop)
                } else {
                    None
                }
            }

            Expression::Call { ty, .. } if ty.is_never() => Some(DeadCodeCause::DivergingCall),

            _ => None,
        }
    }

    /// Returns references to all direct child expressions.
    ///
    /// This is the single source of truth for expression tree recursion. Use this
    /// instead of writing per-variant match arms when you need to walk an expression tree.
    pub fn children(&self) -> Children<'_> {
        match self {
            Expression::Literal { literal, .. } => match literal {
                Literal::Slice(elements) => elements.iter().collect(),
                Literal::FormatString(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        FormatStringPart::Expression(e) => Some(e.as_ref()),
                        FormatStringPart::Text(_) => None,
                    })
                    .collect(),
                _ => Children::new(),
            },
            Expression::Function { body, .. } => children![body],
            Expression::Lambda { body, .. } => children![body],
            Expression::Block { items, .. } => items.iter().collect(),
            Expression::Let {
                value, else_block, ..
            } => {
                let mut c = children![value.as_ref()];
                if let Some(eb) = else_block {
                    c.push(eb);
                }
                c
            }
            Expression::Identifier { .. } => Children::new(),
            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                let mut c = children![expression.as_ref()];
                c.extend(args);
                if let Some(s) = spread.as_ref() {
                    c.push(s);
                }
                c
            }
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => children![condition, consequence, alternative],
            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => children![scrutinee, consequence, alternative],
            Expression::Match { subject, arms, .. } => {
                let mut c = children![subject.as_ref()];
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        c.push(guard);
                    }
                    c.push(&arm.expression);
                }
                c
            }
            Expression::Tuple { elements, .. } => elements.iter().collect(),
            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                let mut c: Children = field_assignments.iter().map(|f| f.value.as_ref()).collect();
                if let Some(s) = spread.as_expression() {
                    c.push(s);
                }
                c
            }
            Expression::DotAccess { expression, .. } => children![expression],
            Expression::Assignment { target, value, .. } => children![target, value],
            Expression::Return { expression, .. } => children![expression],
            Expression::Propagate { expression, .. } => children![expression],
            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                items.iter().collect()
            }
            Expression::ImplBlock { methods, .. } => methods.iter().collect(),
            Expression::Binary { left, right, .. } => children![left, right],
            Expression::Unary { expression, .. } => children![expression],
            Expression::Paren { expression, .. } => children![expression],
            Expression::Const { expression, .. } => children![expression],
            Expression::Loop { body, .. } => children![body],
            Expression::While {
                condition, body, ..
            } => children![condition, body],
            Expression::WhileLet {
                scrutinee, body, ..
            } => children![scrutinee, body],
            Expression::For { iterable, body, .. } => children![iterable, body],
            Expression::Break { value, .. } => value
                .as_ref()
                .map(|v| children![v.as_ref()])
                .unwrap_or_default(),
            Expression::Reference { expression, .. } => children![expression],
            Expression::IndexedAccess {
                expression, index, ..
            } => children![expression, index],
            Expression::Task { expression, .. } => children![expression],
            Expression::Defer { expression, .. } => children![expression],
            Expression::Select { arms, .. } => {
                let mut c = Children::new();
                for arm in arms {
                    match &arm.pattern {
                        SelectArmPattern::Receive {
                            receive_expression,
                            body,
                            ..
                        } => {
                            c.push(receive_expression.as_ref());
                            c.push(body.as_ref());
                        }
                        SelectArmPattern::Send {
                            send_expression,
                            body,
                        } => {
                            c.push(send_expression.as_ref());
                            c.push(body.as_ref());
                        }
                        SelectArmPattern::MatchReceive {
                            receive_expression,
                            arms: match_arms,
                        } => {
                            c.push(receive_expression.as_ref());
                            for ma in match_arms {
                                if let Some(guard) = &ma.guard {
                                    c.push(guard);
                                }
                                c.push(&ma.expression);
                            }
                        }
                        SelectArmPattern::WildCard { body } => {
                            c.push(body.as_ref());
                        }
                    }
                }
                c
            }
            Expression::Range { start, end, .. } => {
                let mut c = Children::new();
                if let Some(s) = start {
                    c.push(s.as_ref());
                }
                if let Some(e) = end {
                    c.push(e.as_ref());
                }
                c
            }
            Expression::Cast { expression, .. } => children![expression],
            Expression::Interface {
                method_signatures, ..
            } => method_signatures.iter().collect(),
            Expression::Unit { .. }
            | Expression::Continue { .. }
            | Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::ModuleImport { .. }
            | Expression::RawGo { .. }
            | Expression::NoOp => Children::new(),
        }
    }

    pub fn unwrap_parens(&self) -> &Expression {
        match self {
            Expression::Paren { expression, .. } => expression.unwrap_parens(),
            other => other,
        }
    }

    pub fn binding_id(&self) -> Option<BindingId> {
        match self.unwrap_parens() {
            Expression::Identifier { binding_id, .. } => *binding_id,
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<u64> {
        match self.unwrap_parens() {
            Expression::Literal {
                literal: Literal::Integer { value, .. },
                ..
            } => Some(*value),
            _ => None,
        }
    }

    /// Inner expression of an explicit `x.*` deref, or `None` for anything else.
    #[inline]
    pub fn deref_inner(&self) -> Option<&Expression> {
        match self {
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => Some(expression),
            _ => None,
        }
    }

    pub fn as_dotted_path(&self) -> Option<String> {
        match self {
            Expression::Identifier { value, .. } => Some(value.to_string()),
            Expression::DotAccess {
                expression, member, ..
            } => Some(format!("{}.{}", expression.as_dotted_path()?, member)),
            _ => None,
        }
    }

    pub fn root_identifier(&self) -> Option<&str> {
        match self {
            Expression::Identifier { value, .. } => Some(value),
            Expression::DotAccess { expression, .. } => expression.root_identifier(),
            _ => None,
        }
    }

    pub fn is_empty_collection(&self) -> bool {
        matches!(
            self,
            Expression::Literal {
                literal: Literal::Slice(elements),
                ..
            } if elements.is_empty()
        )
    }

    pub fn is_all_literals(&self) -> bool {
        match self.unwrap_parens() {
            Expression::Literal { literal, .. } => match literal {
                Literal::Slice(elements) => elements.iter().all(|e| e.is_all_literals()),
                Literal::FormatString(parts) => parts.iter().all(|p| match p {
                    FormatStringPart::Text(_) => true,
                    FormatStringPart::Expression(e) => e.is_all_literals(),
                }),
                _ => true,
            },
            Expression::Tuple { elements, .. } => elements.iter().all(|e| e.is_all_literals()),
            Expression::Unit { .. } => true,
            _ => false,
        }
    }

    pub fn get_var_name(&self) -> Option<String> {
        match self {
            Expression::Identifier { value, .. } => Some(value.to_string()),
            Expression::DotAccess { expression, .. } => expression.get_var_name(),
            Expression::Assignment { target, .. } => target.get_var_name(),
            Expression::IndexedAccess { expression, .. } => expression.get_var_name(),
            Expression::Paren { expression, .. } => expression.get_var_name(),
            Expression::Reference { expression, .. } => expression.get_var_name(),
            Expression::Unary {
                operator,
                expression,
                ..
            } => {
                if operator == &UnaryOperator::Deref {
                    expression.get_var_name()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn is_function(&self) -> bool {
        matches!(self, Expression::Function { .. })
    }

    pub fn set_public(self) -> Self {
        match self {
            Expression::Enum {
                doc,
                attributes,
                name,
                name_span,
                generics,
                variants,
                span,
                ..
            } => Expression::Enum {
                doc,
                attributes,
                name,
                name_span,
                generics,
                variants,
                visibility: Visibility::Public,
                span,
            },
            Expression::Struct {
                doc,
                attributes,
                name,
                name_span,
                generics,
                fields,
                kind,
                span,
                ..
            } => {
                let fields = if kind == StructKind::Tuple {
                    fields
                        .into_iter()
                        .map(|f| StructFieldDefinition {
                            visibility: Visibility::Public,
                            ..f
                        })
                        .collect()
                } else {
                    fields
                };
                Expression::Struct {
                    doc,
                    attributes,
                    name,
                    name_span,
                    generics,
                    fields,
                    kind,
                    visibility: Visibility::Public,
                    span,
                }
            }
            Expression::Function {
                doc,
                attributes,
                name,
                name_span,
                generics,
                params,
                return_annotation,
                return_type,
                body,
                ty,
                span,
                ..
            } => Expression::Function {
                doc,
                attributes,
                name,
                name_span,
                generics,
                params,
                return_annotation,
                return_type,
                visibility: Visibility::Public,
                body,
                ty,
                span,
            },
            Expression::Const {
                doc,
                identifier,
                identifier_span,
                annotation,
                expression,
                ty,
                span,
                ..
            } => Expression::Const {
                doc,
                identifier,
                identifier_span,
                annotation,
                expression,
                visibility: Visibility::Public,
                ty,
                span,
            },
            Expression::VariableDeclaration {
                doc,
                name,
                name_span,
                annotation,
                ty,
                span,
                ..
            } => Expression::VariableDeclaration {
                doc,
                name,
                name_span,
                annotation,
                visibility: Visibility::Public,
                ty,
                span,
            },
            Expression::TypeAlias {
                doc,
                attributes,
                name,
                name_span,
                generics,
                annotation,
                ty,
                span,
                ..
            } => Expression::TypeAlias {
                doc,
                attributes,
                name,
                name_span,
                generics,
                annotation,
                ty,
                visibility: Visibility::Public,
                span,
            },
            Expression::Interface {
                doc,
                name,
                name_span,
                generics,
                parents,
                method_signatures,
                span,
                ..
            } => Expression::Interface {
                doc,
                name,
                name_span,
                generics,
                parents,
                method_signatures,
                visibility: Visibility::Public,
                span,
            },
            expression => expression,
        }
    }

    pub fn has_else(&self) -> bool {
        match self {
            Self::Block { items, .. } if items.is_empty() => false,
            Self::Unit { .. } => false,
            Self::If { alternative, .. } | Self::IfLet { alternative, .. } => {
                alternative.has_else()
            }
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer {
        value: u64,
        text: Option<String>,
    },
    Float {
        value: f64,
        text: Option<String>,
    },
    /// Imaginary coefficient, e.g. `4i` stores `4.0`
    Imaginary(f64),
    Boolean(bool),
    String {
        value: String,
        raw: bool,
    },
    FormatString(Vec<FormatStringPart>),
    Char(String),
    Slice(Vec<Expression>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormatStringPart {
    Text(String),
    Expression(Box<Expression>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negative,
    Not,
    BitwiseNot,
    Deref,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOperator {
    Addition,
    Subtraction,
    Multiplication,
    Division,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAndNot,
    ShiftLeft,
    ShiftRight,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Remainder,
    Equal,
    NotEqual,
    And,
    Or,
    Pipeline,
}

impl BinaryOperator {
    /// The compound-assignment form (`+=`, `<<=`, ...) for operators that have
    /// one. Comparison, logical, and pipeline operators return `None`. Mirrors
    /// the compound-assignment tokens accepted by `parse_assignment`.
    pub fn compound_assignment_symbol(&self) -> Option<&'static str> {
        match self {
            BinaryOperator::Addition => Some("+="),
            BinaryOperator::Subtraction => Some("-="),
            BinaryOperator::Multiplication => Some("*="),
            BinaryOperator::Division => Some("/="),
            BinaryOperator::Remainder => Some("%="),
            BinaryOperator::BitwiseAnd => Some("&="),
            BinaryOperator::BitwiseOr => Some("|="),
            BinaryOperator::BitwiseXor => Some("^="),
            BinaryOperator::BitwiseAndNot => Some("&^="),
            BinaryOperator::ShiftLeft => Some("<<="),
            BinaryOperator::ShiftRight => Some(">>="),
            _ => None,
        }
    }
}

impl std::fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            BinaryOperator::Addition => "+",
            BinaryOperator::Subtraction => "-",
            BinaryOperator::Multiplication => "*",
            BinaryOperator::Division => "/",
            BinaryOperator::Remainder => "%",
            BinaryOperator::BitwiseAnd => "&",
            BinaryOperator::BitwiseOr => "|",
            BinaryOperator::BitwiseXor => "^",
            BinaryOperator::BitwiseAndNot => "&^",
            BinaryOperator::ShiftLeft => "<<",
            BinaryOperator::ShiftRight => ">>",
            BinaryOperator::Equal => "==",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::LessThan => "<",
            BinaryOperator::LessThanOrEqual => "<=",
            BinaryOperator::GreaterThan => ">",
            BinaryOperator::GreaterThanOrEqual => ">=",
            BinaryOperator::And => "&&",
            BinaryOperator::Or => "||",
            BinaryOperator::Pipeline => "|>",
        };
        write!(f, "{}", symbol)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParentInterface {
    pub annotation: Annotation,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub fn is_public(&self) -> bool {
        matches!(self, Visibility::Public)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportAlias {
    Named(EcoString, Span),
    Blank(Span),
}
