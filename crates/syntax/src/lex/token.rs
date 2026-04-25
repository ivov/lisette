use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Token<'source> {
    pub kind: TokenKind,
    pub text: &'source str,
    pub byte_offset: u32,
    pub byte_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenKind {
    Integer,
    Imaginary,
    String,
    RawString,
    FormatStringStart,
    FormatStringText,
    FormatStringInterpolationStart,
    FormatStringInterpolationEnd,
    FormatStringEnd,
    Char,
    Boolean,
    Float,
    Identifier,
    Comment,
    DocComment,
    Semicolon,
    LeftParen,
    RightParen,
    LeftSquareBracket,
    RightSquareBracket,
    LeftCurlyBrace,
    RightCurlyBrace,
    LeftAngleBracket,
    RightAngleBracket,
    Arrow,
    ArrowDouble,
    Equal,
    EqualDouble,
    NotEqual,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Colon,
    Pipe,
    PipeDouble,
    Pipeline,
    Ampersand,
    AmpersandDouble,
    Plus,
    Minus,
    Star,
    Slash,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    Caret,
    Percent,
    Bang,
    QuestionMark,
    Dot,
    Comma,
    Hash,
    DotDot,
    DotDotEqual,
    Backtick,
    Function,
    Let,
    If,
    Else,
    Match,
    Enum,
    Struct,
    Type,
    Interface,
    Impl,
    Const,
    Var,
    Return,
    Defer,
    Import,
    Mut,
    Pub,
    For,
    In_,
    While,
    Loop,
    Break,
    Continue,
    Select,
    Task,
    Try,
    Recover,
    As,
    Directive,
    EOF,
    Placeholder,
    Error,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TokenKind::*;
        let s = match self {
            Integer => "integer",
            Imaginary => "imaginary",
            String => "string",
            RawString => "raw string",
            FormatStringStart => "format string",
            FormatStringText => "format string",
            FormatStringInterpolationStart => "`{`",
            FormatStringInterpolationEnd => "`}`",
            FormatStringEnd => "format string",
            Char => "rune",
            Boolean => "boolean",
            Float => "float",
            Identifier => "identifier",
            Comment => "comment",
            DocComment => "doc comment",
            Semicolon => "`;`",
            LeftParen => "`(`",
            RightParen => "`)`",
            LeftSquareBracket => "`[`",
            RightSquareBracket => "`]`",
            LeftCurlyBrace => "`{`",
            RightCurlyBrace => "`}`",
            LeftAngleBracket => "`<`",
            RightAngleBracket => "`>`",
            Arrow => "`->`",
            ArrowDouble => "`=>`",
            Equal => "`=`",
            EqualDouble => "`==`",
            NotEqual => "`!=`",
            GreaterThanOrEqual => "`>=`",
            LessThanOrEqual => "`<=`",
            Colon => "`:`",
            Pipe => "`|`",
            PipeDouble => "`||`",
            Pipeline => "`|>`",
            Ampersand => "`&`",
            AmpersandDouble => "`&&`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            PlusEqual => "`+=`",
            MinusEqual => "`-=`",
            StarEqual => "`*=`",
            SlashEqual => "`/=`",
            PercentEqual => "`%=`",
            Caret => "`^`",
            Percent => "`%`",
            Bang => "`!`",
            QuestionMark => "`?`",
            Dot => "`.`",
            Comma => "`,`",
            Hash => "`#`",
            DotDot => "`..`",
            DotDotEqual => "`..=`",
            Backtick => "`` ` ``",
            Function => "`fn`",
            Let => "`let`",
            If => "`if`",
            Else => "`else`",
            Match => "`match`",
            Enum => "`enum`",
            Struct => "`struct`",
            Type => "`type`",
            Interface => "`interface`",
            Impl => "`impl`",
            Const => "`const`",
            Var => "`var`",
            Return => "`return`",
            Defer => "`defer`",
            Import => "`import`",
            Mut => "`mut`",
            Pub => "`pub`",
            For => "`for`",
            In_ => "`in`",
            While => "`while`",
            Loop => "`loop`",
            Break => "`break`",
            Continue => "`continue`",
            Select => "`select`",
            Task => "`task`",
            Try => "`try`",
            Recover => "`recover`",
            As => "`as`",
            Directive => "directive",
            EOF => "end of file",
            Placeholder => "placeholder",
            Error => "error",
        };
        write!(f, "{}", s)
    }
}

impl TokenKind {
    pub fn from_keyword(s: &str) -> Option<Self> {
        use TokenKind::*;

        match s {
            "fn" => Some(Function),
            "let" => Some(Let),
            "if" => Some(If),
            "else" => Some(Else),
            "match" => Some(Match),
            "enum" => Some(Enum),
            "struct" => Some(Struct),
            "type" => Some(Type),
            "interface" => Some(Interface),
            "impl" => Some(Impl),
            "const" => Some(Const),
            "var" => Some(Var),
            "return" => Some(Return),
            "defer" => Some(Defer),
            "import" => Some(Import),
            "mut" => Some(Mut),
            "pub" => Some(Pub),
            "for" => Some(For),
            "in" => Some(In_),
            "while" => Some(While),
            "loop" => Some(Loop),
            "break" => Some(Break),
            "continue" => Some(Continue),
            "select" => Some(Select),
            "task" => Some(Task),
            "try" => Some(Try),
            "recover" => Some(Recover),
            "as" => Some(As),
            _ => None,
        }
    }

    pub fn is_keyword(&self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            Function
                | Let
                | If
                | Else
                | Match
                | Enum
                | Struct
                | Type
                | Interface
                | Impl
                | Const
                | Var
                | Return
                | Defer
                | Import
                | Mut
                | Pub
                | For
                | In_
                | While
                | Loop
                | Break
                | Continue
                | Select
                | Task
                | Try
                | Recover
                | As
        )
    }

    pub fn from_three_char_symbol(c1: char, c2: char, c3: char) -> Option<Self> {
        match (c1, c2, c3) {
            ('.', '.', '=') => Some(TokenKind::DotDotEqual),
            _ => None,
        }
    }

    pub fn from_two_char_symbol(c1: char, c2: char) -> Option<Self> {
        use TokenKind::*;

        match (c1, c2) {
            ('-', '>') => Some(Arrow),
            ('=', '>') => Some(ArrowDouble),
            ('=', '=') => Some(EqualDouble),
            ('!', '=') => Some(NotEqual),
            ('>', '=') => Some(GreaterThanOrEqual),
            ('<', '=') => Some(LessThanOrEqual),
            ('|', '|') => Some(PipeDouble),
            ('|', '>') => Some(Pipeline),
            ('&', '&') => Some(AmpersandDouble),
            ('.', '.') => Some(DotDot),
            ('+', '=') => Some(PlusEqual),
            ('-', '=') => Some(MinusEqual),
            ('*', '=') => Some(StarEqual),
            ('/', '=') => Some(SlashEqual),
            ('%', '=') => Some(PercentEqual),
            _ => None,
        }
    }

    pub fn from_one_char_symbol(c: char) -> Option<Self> {
        use TokenKind::*;

        match c {
            '(' => Some(LeftParen),
            ')' => Some(RightParen),
            '[' => Some(LeftSquareBracket),
            ']' => Some(RightSquareBracket),
            '{' => Some(LeftCurlyBrace),
            '}' => Some(RightCurlyBrace),
            '<' => Some(LeftAngleBracket),
            '>' => Some(RightAngleBracket),
            '=' => Some(Equal),
            ':' => Some(Colon),
            '|' => Some(Pipe),
            '&' => Some(Ampersand),
            '+' => Some(Plus),
            '-' => Some(Minus),
            '*' => Some(Star),
            '^' => Some(Caret),
            '%' => Some(Percent),
            '!' => Some(Bang),
            '?' => Some(QuestionMark),
            '.' => Some(Dot),
            ',' => Some(Comma),
            '#' => Some(Hash),
            _ => None,
        }
    }
}
