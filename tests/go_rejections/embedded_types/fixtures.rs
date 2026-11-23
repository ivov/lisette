use super::scenario::*;

fn string_ret() -> Signature {
    Signature {
        parameters: vec![],
        return_type: MemberType::Basic(BasicType::String),
    }
}

/// A public, value-receiver, string-returning (identity) method.
pub fn smethod(base: &str) -> Method {
    Method {
        name: cased(base, Visibility::Public),
        receiver: Receiver::Value,
        signature: string_ret(),
        visibility: Visibility::Public,
    }
}

/// A public interface method returning the given basic type.
pub fn imethod(base: &str, return_type: BasicType) -> Method {
    Method {
        name: cased(base, Visibility::Public),
        receiver: Receiver::Value,
        signature: Signature {
            parameters: vec![],
            return_type: MemberType::Basic(return_type),
        },
        visibility: Visibility::Public,
    }
}

pub fn vembed(target: NodeId) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Value,
        storage: Storage::Plain,
        type_args: vec![],
    }
}

pub fn pembed(target: NodeId) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Pointer,
        storage: Storage::Plain,
        type_args: vec![],
    }
}

pub fn struct_node(
    id: NodeId,
    embeds: Vec<Embed>,
    fields: Vec<Field>,
    methods: Vec<Method>,
) -> Node {
    Node {
        id,
        name: format!("N{id}"),
        type_params: vec![],
        kind: NodeKind::Struct {
            fields,
            embeds,
            methods,
        },
        origin: Origin::Native,
    }
}

pub fn iface_node(id: NodeId, embeds: Vec<Embed>, methods: Vec<Method>) -> Node {
    Node {
        id,
        name: format!("N{id}"),
        type_params: vec![],
        kind: NodeKind::Interface { methods, embeds },
        origin: Origin::Native,
    }
}

fn sel(root: NodeId, member: &str) -> Question {
    Question::Selector {
        root,
        member: member.into(),
        kind: SelKind::Method,
    }
}

/// `N0` has a direct method `M`; question `N0.M`. Resolves at depth 0 in both
/// languages: a MATCH today, and the simplest case that can actually be run.
pub fn direct_method() -> Scenario {
    Scenario {
        name: "direct_method".into(),
        seed: 0,
        nodes: vec![struct_node(0, vec![], vec![], vec![smethod("m")])],
        questions: vec![sel(0, "M")],
    }
}

/// `N1` embeds `N0` (value); question `N1.M`. Go promotes `M` at depth 1; Lisette
/// does not yet, so INCOMPLETE.
pub fn value_embed_method() -> Scenario {
    Scenario {
        name: "value_embed_method".into(),
        seed: 0,
        nodes: vec![
            struct_node(0, vec![], vec![], vec![smethod("m")]),
            struct_node(1, vec![vembed(0)], vec![], vec![]),
        ],
        questions: vec![sel(1, "M")],
    }
}

/// `N1` embeds `*N0` (pointer); question `N1.M`. Go promotes `M` at depth 1 with an
/// indirection; Lisette does not yet, so INCOMPLETE.
pub fn pointer_embed_method() -> Scenario {
    Scenario {
        name: "pointer_embed_method".into(),
        seed: 0,
        nodes: vec![
            struct_node(0, vec![], vec![], vec![smethod("m")]),
            struct_node(1, vec![pembed(0)], vec![], vec![]),
        ],
        questions: vec![sel(1, "M")],
    }
}

/// Diamond: `N3` embeds `N1` and `N2`, both embedding `N0`. `N0.M` is reachable
/// at depth 2 via two subobjects, so `N3.M` is AMBIGUOUS in Go. Lisette does not
/// promote yet, so INCOMPLETE, and must never silently pick one path.
pub fn diamond() -> Scenario {
    Scenario {
        name: "diamond".into(),
        seed: 0,
        nodes: vec![
            struct_node(0, vec![], vec![], vec![smethod("m")]),
            struct_node(1, vec![vembed(0)], vec![], vec![]),
            struct_node(2, vec![vembed(0)], vec![], vec![]),
            struct_node(3, vec![vembed(1), vembed(2)], vec![], vec![]),
        ],
        questions: vec![sel(3, "M")],
    }
}

/// `N1` declares `speak` directly and so satisfies interface `N0`: a MATCH.
pub fn interface_direct_satisfaction() -> Scenario {
    Scenario {
        name: "interface_direct_satisfaction".into(),
        seed: 0,
        nodes: vec![
            iface_node(0, vec![], vec![imethod("speak", BasicType::String)]),
            struct_node(1, vec![], vec![], vec![smethod("speak")]),
        ],
        questions: vec![Question::Satisfies {
            type_id: 1,
            interface: 0,
        }],
    }
}

/// `N2` embeds `N1`, which declares `speak`, so Go has `N2` satisfy interface
/// `N0` via promotion. Lisette does not promote yet, so INCOMPLETE.
pub fn interface_promoted_satisfaction() -> Scenario {
    Scenario {
        name: "interface_promoted_satisfaction".into(),
        seed: 0,
        nodes: vec![
            iface_node(0, vec![], vec![imethod("speak", BasicType::String)]),
            struct_node(1, vec![], vec![], vec![smethod("speak")]),
            struct_node(2, vec![vembed(1)], vec![], vec![]),
        ],
        questions: vec![Question::Satisfies {
            type_id: 2,
            interface: 0,
        }],
    }
}

/// Two embedded interfaces declare `M` with different signatures, so the
/// composed interface `N2` is a duplicate-method error in Go. `N3` declares
/// `M() string`, matching only `N1`, so Go has `N3` not satisfy `N2`. Lisette
/// rejects `N2` at registration, agreeing.
pub fn interface_conflict() -> Scenario {
    Scenario {
        name: "interface_conflict".into(),
        seed: 0,
        nodes: vec![
            iface_node(0, vec![], vec![imethod("m", BasicType::Int)]),
            iface_node(1, vec![], vec![imethod("m", BasicType::String)]),
            iface_node(2, vec![vembed(0), vembed(1)], vec![]),
            struct_node(3, vec![], vec![], vec![smethod("m")]),
        ],
        questions: vec![Question::Satisfies {
            type_id: 3,
            interface: 2,
        }],
    }
}

/// Every fixture, for tests that want to sweep all of them.
pub fn all() -> Vec<Scenario> {
    vec![
        direct_method(),
        value_embed_method(),
        pointer_embed_method(),
        diamond(),
        interface_direct_satisfaction(),
        interface_promoted_satisfaction(),
    ]
}
