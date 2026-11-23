use serde::{Deserialize, Serialize};

/// A node's identity: its index into [`Scenario::nodes`].
pub type NodeId = usize;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    /// 0 for hand-written corpus cases; a generated divergence reproduces from
    /// `name` + `seed`.
    pub seed: u64,
    pub nodes: Vec<Node>,
    pub questions: Vec<Question>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// `N{id}`; unique within the scenario by construction.
    pub name: String,
    /// Generic parameter names (e.g. `["A"]`); empty for a monomorphic node.
    /// A member may reference one via `MemberType::TypeParam`.
    #[serde(default)]
    pub type_params: Vec<String>,
    pub kind: NodeKind,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NodeKind {
    Struct {
        fields: Vec<Field>,
        embeds: Vec<Embed>,
        methods: Vec<Method>,
    },
    Interface {
        methods: Vec<Method>,
        /// Go requires embedded interface parents to be interfaces.
        embeds: Vec<Embed>,
    },
    /// A Lisette newtype (`struct Celsius(float64)` plus an `impl`); lowers to a
    /// named Go scalar with methods.
    NamedBasic {
        underlying: BasicType,
        methods: Vec<Method>,
    },
}

/// An anonymous embedded field. No visibility axis: Lisette rejects `pub embed`,
/// so the field's name and exportedness follow the target type's casing.
///
/// `edge` and `storage` are orthogonal so a nilable pointer edge
/// (`Option<Ref<T>>`) stays representable; folding them would force it to drop
/// its pointer-ness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embed {
    pub target: NodeId,
    /// `Value` => `embed T` / `T`; `Pointer` => `embed Ref<T>` / `*T`.
    pub edge: EdgeKind,
    /// Pairs with `edge`: `Option` with `Value`, `OptionPointer` with `Pointer`.
    pub storage: Storage,
    /// Type arguments when the target is generic (e.g. `[Basic(Int)]` for
    /// `embed Box<int>`); length matches the target's `type_params`.
    #[serde(default)]
    pub type_args: Vec<MemberType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Value,
    Pointer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Storage {
    Plain,
    /// `Option<T>`: a nilable value edge.
    Option,
    /// `Option<Ref<T>>`: a nilable pointer edge.
    OptionPointer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub member_type: MemberType,
    pub visibility: Visibility,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    pub receiver: Receiver,
    pub signature: Signature,
    pub visibility: Visibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Receiver {
    Value,
    Pointer,
}

/// Concrete methods return `string` (their own qualified name), so when the
/// generated code runs, its output names the declaration that answered. That is
/// how we confirm the right member was resolved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    pub parameters: Vec<MemberType>,
    pub return_type: MemberType,
}

/// A closed type universe so both renderers translate it exhaustively.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MemberType {
    Basic(BasicType),
    Node(NodeId),
    Ref(Box<MemberType>),
    Slice(Box<MemberType>),
    Option(Box<MemberType>),
    /// A reference to an enclosing node's generic parameter (e.g. `T`).
    TypeParam(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasicType {
    Int,
    Float,
    String,
    Bool,
    Byte,
    Rune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
}

/// A node declared in the generated source, or a reference to a real Go package type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Origin {
    Native,
    Imported { pkg: String },
}

impl Origin {
    pub fn pkg(&self) -> Option<&str> {
        match self {
            Origin::Native => None,
            Origin::Imported { pkg } => Some(pkg),
        }
    }

    /// The reference qualifier: the last path segment (`net/url` -> `url`).
    pub fn alias(&self) -> Option<&str> {
        self.pkg().map(|pkg| pkg.rsplit('/').next().unwrap_or(pkg))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Question {
    Selector {
        root: NodeId,
        member: String,
        kind: SelKind,
    },
    Satisfies {
        type_id: NodeId,
        interface: NodeId,
    },
}

/// How a selector question is spelled in Lisette: a render hint (Lisette cannot
/// uniformly take a method value), not an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelKind {
    Field,
    Method,
}

impl Scenario {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn node_name(&self, id: NodeId) -> &str {
        &self.nodes[id].name
    }

    /// How a node is referenced in source: `Name`, or `alias.Name` if imported.
    pub fn node_ref(&self, id: NodeId) -> String {
        let node = &self.nodes[id];
        match node.origin.alias() {
            Some(alias) => format!("{alias}.{}", node.name),
            None => node.name.clone(),
        }
    }

    /// Invariants the renderers rely on; a failure is a generator/corpus bug.
    pub fn validate(&self) -> Result<(), String> {
        for (index, node) in self.nodes.iter().enumerate() {
            if node.id != index {
                return Err(format!("node {} has id {}", index, node.id));
            }
            if let Some(pkg) = node.origin.pkg()
                && pkg.starts_with("go:")
            {
                return Err(format!(
                    "imported node `{}` has Lisette-prefixed pkg `{pkg}`; use the raw Go path",
                    node.name
                ));
            }
            for embed in node.kind.embeds() {
                self.check_node_ref(embed.target, "embed target")?;
                match (embed.edge, embed.storage) {
                    (_, Storage::Plain)
                    | (EdgeKind::Value, Storage::Option)
                    | (EdgeKind::Pointer, Storage::OptionPointer) => {}
                    (edge, storage) => {
                        return Err(format!(
                            "embed on `{}` pairs {edge:?} edge with {storage:?} storage",
                            node.name
                        ));
                    }
                }
                if node.kind.is_interface() && !self.nodes[embed.target].kind.is_interface() {
                    return Err(format!(
                        "interface `{}` embeds non-interface `{}`",
                        node.name,
                        self.node_name(embed.target)
                    ));
                }
                if embed.type_args.len() != self.nodes[embed.target].type_params.len() {
                    return Err(format!(
                        "embed on `{}` gives {} type args for target `{}` with {} params",
                        node.name,
                        embed.type_args.len(),
                        self.node_name(embed.target),
                        self.nodes[embed.target].type_params.len()
                    ));
                }
                for arg in &embed.type_args {
                    self.check_member_type(arg, &node.type_params)?;
                }
            }
            for field in node.kind.fields() {
                check_casing(&field.name, field.visibility, &node.name)?;
                self.check_member_type(&field.member_type, &node.type_params)?;
            }
            for method in node.kind.methods() {
                check_casing(&method.name, method.visibility, &node.name)?;
                for param in &method.signature.parameters {
                    self.check_member_type(param, &node.type_params)?;
                }
                self.check_member_type(&method.signature.return_type, &node.type_params)?;
            }
        }
        for question in &self.questions {
            match question {
                Question::Selector { root, .. } => self.check_node_ref(*root, "selector root")?,
                Question::Satisfies { type_id, interface } => {
                    self.check_node_ref(*type_id, "satisfies type")?;
                    self.check_node_ref(*interface, "satisfies interface")?;
                    if !self.nodes[*interface].kind.is_interface() {
                        return Err(format!(
                            "satisfies question targets non-interface `{}`",
                            self.node_name(*interface)
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_node_ref(&self, id: NodeId, what: &str) -> Result<(), String> {
        if id >= self.nodes.len() {
            return Err(format!("{what} references out-of-range node {id}"));
        }
        Ok(())
    }

    fn check_member_type(&self, member_type: &MemberType, scope: &[String]) -> Result<(), String> {
        match member_type {
            MemberType::Basic(_) => Ok(()),
            MemberType::TypeParam(name) => {
                if scope.iter().any(|param| param == name) {
                    Ok(())
                } else {
                    Err(format!("type param `{name}` is not in scope"))
                }
            }
            MemberType::Node(id) => self.check_node_ref(*id, "member type"),
            MemberType::Ref(inner) | MemberType::Slice(inner) | MemberType::Option(inner) => {
                self.check_member_type(inner, scope)
            }
        }
    }
}

impl NodeKind {
    pub fn embeds(&self) -> &[Embed] {
        match self {
            NodeKind::Struct { embeds, .. } | NodeKind::Interface { embeds, .. } => embeds,
            NodeKind::NamedBasic { .. } => &[],
        }
    }

    pub fn methods(&self) -> &[Method] {
        match self {
            NodeKind::Struct { methods, .. }
            | NodeKind::Interface { methods, .. }
            | NodeKind::NamedBasic { methods, .. } => methods,
        }
    }

    pub fn fields(&self) -> &[Field] {
        match self {
            NodeKind::Struct { fields, .. } => fields,
            _ => &[],
        }
    }

    pub fn is_interface(&self) -> bool {
        matches!(self, NodeKind::Interface { .. })
    }
}

impl BasicType {
    pub const ALL: [BasicType; 6] = [
        BasicType::Int,
        BasicType::Float,
        BasicType::String,
        BasicType::Bool,
        BasicType::Byte,
        BasicType::Rune,
    ];

    /// The Lisette spelling. Lisette names the 64-bit float `float64` (there is
    /// no `float` type); the others match Go.
    pub fn lisette(self) -> &'static str {
        match self {
            BasicType::Int => "int",
            BasicType::Float => "float64",
            BasicType::String => "string",
            BasicType::Bool => "bool",
            BasicType::Byte => "byte",
            BasicType::Rune => "rune",
        }
    }

    /// The Go spelling.
    pub fn go(self) -> &'static str {
        match self {
            BasicType::Int => "int",
            BasicType::Float => "float64",
            BasicType::String => "string",
            BasicType::Bool => "bool",
            BasicType::Byte => "byte",
            BasicType::Rune => "rune",
        }
    }
}

/// The shared casing accessor both renderers and the generator route through,
/// so an identifier can never export in one language and not the other. Public
/// => leading uppercase (exported in Go); Private => leading lowercase.
pub fn cased(base: &str, visibility: Visibility) -> String {
    let mut chars = base.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest: String = chars.collect();
    match visibility {
        Visibility::Public => format!("{}{}", first.to_uppercase(), rest),
        Visibility::Private => format!("{}{}", first.to_lowercase(), rest),
    }
}

fn check_casing(name: &str, visibility: Visibility, owner: &str) -> Result<(), String> {
    let Some(first) = name.chars().next() else {
        return Err(format!("empty member name on `{owner}`"));
    };
    let exported = first.is_uppercase();
    let want_exported = matches!(visibility, Visibility::Public);
    if exported != want_exported {
        return Err(format!(
            "member `{name}` on `{owner}` has casing inconsistent with its visibility"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches every variant.
    fn kitchen_sink() -> Scenario {
        let leaf = Node {
            id: 0,
            name: "N0".into(),
            type_params: vec![],
            kind: NodeKind::NamedBasic {
                underlying: BasicType::Float,
                methods: vec![Method {
                    name: cased("m0", Visibility::Public),
                    receiver: Receiver::Value,
                    signature: Signature {
                        parameters: vec![],
                        return_type: MemberType::Basic(BasicType::String),
                    },
                    visibility: Visibility::Public,
                }],
            },
            origin: Origin::Native,
        };

        let interface = Node {
            id: 1,
            name: "N1".into(),
            type_params: vec![],
            kind: NodeKind::Interface {
                methods: vec![Method {
                    name: cased("speak", Visibility::Public),
                    receiver: Receiver::Value,
                    signature: Signature {
                        parameters: vec![],
                        return_type: MemberType::Basic(BasicType::Int),
                    },
                    visibility: Visibility::Public,
                }],
                embeds: vec![],
            },
            origin: Origin::Imported { pkg: "fmt".into() },
        };

        let outer = Node {
            id: 2,
            name: "N2".into(),
            type_params: vec![],
            kind: NodeKind::Struct {
                fields: vec![
                    Field {
                        name: cased("count", Visibility::Public),
                        member_type: MemberType::Basic(BasicType::Int),
                        visibility: Visibility::Public,
                    },
                    Field {
                        name: cased("nested", Visibility::Private),
                        member_type: MemberType::Slice(Box::new(MemberType::Ref(Box::new(
                            MemberType::Option(Box::new(MemberType::Node(0))),
                        )))),
                        visibility: Visibility::Private,
                    },
                ],
                embeds: vec![
                    Embed {
                        target: 0,
                        edge: EdgeKind::Value,
                        storage: Storage::Plain,
                        type_args: vec![],
                    },
                    Embed {
                        target: 1,
                        edge: EdgeKind::Value,
                        storage: Storage::Option,
                        type_args: vec![],
                    },
                ],
                methods: vec![Method {
                    name: cased("own", Visibility::Private),
                    receiver: Receiver::Pointer,
                    signature: Signature {
                        parameters: vec![MemberType::Basic(BasicType::Byte)],
                        return_type: MemberType::Basic(BasicType::String),
                    },
                    visibility: Visibility::Private,
                }],
            },
            origin: Origin::Native,
        };

        let pointer_embedder = Node {
            id: 3,
            name: "N3".into(),
            type_params: vec![],
            kind: NodeKind::Struct {
                fields: vec![],
                embeds: vec![Embed {
                    target: 2,
                    edge: EdgeKind::Pointer,
                    storage: Storage::OptionPointer,
                    type_args: vec![],
                }],
                methods: vec![],
            },
            origin: Origin::Native,
        };

        Scenario {
            name: "kitchen_sink".into(),
            seed: 0,
            nodes: vec![leaf, interface, outer, pointer_embedder],
            questions: vec![
                Question::Selector {
                    root: 3,
                    member: "M0".into(),
                    kind: SelKind::Method,
                },
                Question::Satisfies {
                    type_id: 2,
                    interface: 1,
                },
            ],
        }
    }

    #[test]
    fn json_round_trips() {
        let scenario = kitchen_sink();
        let json = serde_json::to_string(&scenario).expect("serialize");
        let back: Scenario = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(scenario, back);
    }

    #[test]
    fn kitchen_sink_validates() {
        kitchen_sink()
            .validate()
            .expect("kitchen sink is a valid scenario");
    }

    #[test]
    fn validate_rejects_bad_id() {
        let mut scenario = kitchen_sink();
        scenario.nodes[1].id = 99;
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn validate_rejects_go_prefixed_imported_pkg() {
        let mut scenario = kitchen_sink();
        scenario.nodes[1].origin = Origin::Imported {
            pkg: "go:fmt".into(),
        };
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn validate_rejects_casing_visibility_mismatch() {
        let mut scenario = kitchen_sink();
        if let NodeKind::Struct { fields, .. } = &mut scenario.nodes[2].kind {
            fields[0].name = "count".into();
            fields[0].visibility = Visibility::Public;
        }
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn validate_rejects_interface_embedding_struct() {
        let mut scenario = kitchen_sink();
        if let NodeKind::Interface { embeds, .. } = &mut scenario.nodes[1].kind {
            embeds.push(Embed {
                target: 2,
                edge: EdgeKind::Value,
                storage: Storage::Plain,
                type_args: vec![],
            });
        }
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn validate_rejects_out_of_scope_type_param() {
        let mut scenario = kitchen_sink();
        if let NodeKind::Struct { fields, .. } = &mut scenario.nodes[2].kind {
            fields[0].member_type = MemberType::TypeParam("Z".into());
        }
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn validate_rejects_storage_edge_mismatch() {
        let mut scenario = kitchen_sink();
        if let NodeKind::Struct { embeds, .. } = &mut scenario.nodes[2].kind {
            embeds[0].edge = EdgeKind::Value;
            embeds[0].storage = Storage::OptionPointer;
        }
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn basic_type_spellings() {
        assert_eq!(BasicType::Float.lisette(), "float64");
        assert_eq!(BasicType::Float.go(), "float64");
        assert_eq!(BasicType::ALL.len(), 6);
    }
}
