use super::NativeTypeKind;

#[derive(Clone, Copy)]
pub struct NativeOperation<'a> {
    pub kind: NativeTypeKind,
    pub method: &'a str,
}

impl<'a> NativeOperation<'a> {
    pub fn new(kind: NativeTypeKind, method: &'a str) -> Self {
        Self { kind, method }
    }
    pub fn mutates_receiver(self) -> bool {
        matches!(
            (self.kind, self.method),
            (NativeTypeKind::Map, "delete") | (NativeTypeKind::Slice, "copy_from")
        )
    }
    pub fn grows_slice(self) -> bool {
        self.kind == NativeTypeKind::Slice && matches!(self.method, "append" | "reserve")
    }
    pub fn returns_fresh_slice(self) -> bool {
        matches!(
            (self.kind, self.method),
            (
                NativeTypeKind::Slice,
                "append" | "reserve" | "clone" | "filter" | "map"
            ) | (NativeTypeKind::Array, "to_slice")
        )
    }

    pub fn read_only_without_callbacks(self) -> bool {
        let Self {
            kind: native_type,
            method,
        } = self;
        match native_type {
            NativeTypeKind::String => matches!(
                method,
                "length"
                    | "is_empty"
                    | "contains"
                    | "split"
                    | "starts_with"
                    | "ends_with"
                    | "byte_at"
                    | "rune_at"
                    | "bytes"
                    | "runes"
                    | "substring"
            ),
            NativeTypeKind::Array => matches!(method, "length" | "get" | "to_slice"),
            NativeTypeKind::Slice => matches!(
                method,
                "length"
                    | "is_empty"
                    | "capacity"
                    | "get"
                    | "append"
                    | "enumerate"
                    | "clone"
                    | "join"
            ),
            NativeTypeKind::Map => matches!(method, "length" | "is_empty" | "get" | "clone"),
            NativeTypeKind::Channel | NativeTypeKind::Sender | NativeTypeKind::Receiver => {
                matches!(method, "length" | "is_empty" | "capacity")
            }
            NativeTypeKind::EnumeratedSlice => method == "clone",
        }
    }
}

#[derive(Clone, Copy)]
pub enum NativeInlinePackage {
    None,
    Slices,
    Strings,
    Prelude,
}

#[derive(Clone, Copy)]
pub enum NativeInlineForm {
    Call(&'static str),

    IsEmpty,
    Receiver,

    Conversion(&'static str),

    Index,
}

pub struct NativeInlineRule {
    types: &'static [NativeTypeKind],
    method: &'static str,
    arity: RuleArity,
    pub form: NativeInlineForm,
    pub package: NativeInlinePackage,
}

#[derive(Clone, Copy)]
enum RuleArity {
    Exact(usize),
    Variadic,
}

impl RuleArity {
    fn accepts(self, actual: usize) -> bool {
        match self {
            Self::Exact(expected) => actual == expected,
            Self::Variadic => true,
        }
    }
}

impl NativeInlineRule {
    fn matches(&self, native_type: NativeTypeKind, method: &str, arity: usize) -> bool {
        self.method == method && self.types.contains(&native_type) && self.arity.accepts(arity)
    }
}

static INLINE_METHODS: &[NativeInlineRule] = &[
    NativeInlineRule {
        types: &[
            NativeTypeKind::Slice,
            NativeTypeKind::Map,
            NativeTypeKind::Channel,
            NativeTypeKind::Sender,
            NativeTypeKind::Receiver,
            NativeTypeKind::String,
            NativeTypeKind::Array,
        ],
        method: "length",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::Call("len"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[
            NativeTypeKind::Slice,
            NativeTypeKind::Channel,
            NativeTypeKind::Sender,
            NativeTypeKind::Receiver,
        ],
        method: "capacity",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::Call("cap"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[
            NativeTypeKind::Slice,
            NativeTypeKind::Map,
            NativeTypeKind::Channel,
            NativeTypeKind::Sender,
            NativeTypeKind::Receiver,
            NativeTypeKind::String,
        ],
        method: "is_empty",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::IsEmpty,
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "enumerate",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::Receiver,
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "bytes",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::Conversion("byte"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "runes",
        arity: RuleArity::Exact(0),
        form: NativeInlineForm::Conversion("rune"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Map],
        method: "delete",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("delete"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "copy_from",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("copy"),
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "contains",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("Contains"),
        package: NativeInlinePackage::Slices,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "contains",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("Contains"),
        package: NativeInlinePackage::Strings,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "split",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("Split"),
        package: NativeInlinePackage::Strings,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "starts_with",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("HasPrefix"),
        package: NativeInlinePackage::Strings,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "ends_with",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("HasSuffix"),
        package: NativeInlinePackage::Strings,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "byte_at",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Index,
        package: NativeInlinePackage::None,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::String],
        method: "rune_at",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("RuneAt"),
        package: NativeInlinePackage::Prelude,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "join",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("Join"),
        package: NativeInlinePackage::Strings,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "any",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("ContainsFunc"),
        package: NativeInlinePackage::Slices,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "reserve",
        arity: RuleArity::Exact(1),
        form: NativeInlineForm::Call("Grow"),
        package: NativeInlinePackage::Slices,
    },
    NativeInlineRule {
        types: &[NativeTypeKind::Slice],
        method: "append",
        arity: RuleArity::Variadic,
        form: NativeInlineForm::Call("append"),
        package: NativeInlinePackage::None,
    },
];

impl NativeOperation<'_> {
    pub fn inline_rule(self, arity: usize) -> Option<&'static NativeInlineRule> {
        INLINE_METHODS
            .iter()
            .find(|rule| rule.matches(self.kind, self.method, arity))
    }
    pub fn reads_array_in_place(self) -> bool {
        self.kind == NativeTypeKind::Array && matches!(self.method, "to_slice" | "get")
    }

    pub fn reads_elements_only(self, arity: usize) -> bool {
        matches!(
            (self.method, arity),
            ("length" | "is_empty" | "clone", 0) | ("get" | "contains" | "equals" | "join", 1)
        )
    }
    pub fn lowers_to_plain_call(self, arity: usize) -> bool {
        if matches!(self.method, "substring" | "equals" | "clone") || self.reads_array_in_place() {
            return true;
        }
        self.inline_rule(arity).is_none_or(|rule| {
            matches!(
                rule.method,
                "delete"
                    | "contains"
                    | "split"
                    | "starts_with"
                    | "ends_with"
                    | "rune_at"
                    | "join"
                    | "any"
            )
        })
    }
}
