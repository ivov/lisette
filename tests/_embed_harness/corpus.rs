//! Hand-written test cases the generator cannot produce: cyclic graphs, the
//! conflicting-interface case, and inputs that must be rejected with a specific
//! `embed_*` error. Some rejections are expected to be lifted later, and their
//! tests will start passing when that happens.

use super::fixtures::{iface_node, imethod, smethod, struct_node, vembed};
use super::scenario::*;

fn pembed(target: NodeId) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Pointer,
        storage: Storage::Plain,
        type_args: vec![],
    }
}

fn sel(root: NodeId, member: &str) -> Question {
    Question::Selector {
        root,
        member: member.into(),
        kind: SelKind::Method,
    }
}

/// `N0` embeds `*N1` and `N1` embeds `N0` by value; the pointer edge breaks the
/// cycle, so Go accepts the mutually-recursive types. A DAG cannot express this.
pub fn pointer_cycle() -> Scenario {
    Scenario {
        name: "pointer_cycle".into(),
        seed: 0,
        nodes: vec![
            struct_node(0, vec![pembed(1)], vec![], vec![smethod("a")]),
            struct_node(1, vec![vembed(0)], vec![], vec![smethod("b")]),
        ],
        questions: vec![sel(0, "A"), sel(0, "B"), sel(1, "B"), sel(1, "A")],
    }
}

/// A three-node diamond where the shared base is reached by two pointer edges.
pub fn pointer_diamond() -> Scenario {
    Scenario {
        name: "pointer_diamond".into(),
        seed: 0,
        nodes: vec![
            struct_node(0, vec![], vec![], vec![smethod("m")]),
            struct_node(1, vec![pembed(0)], vec![], vec![]),
            struct_node(2, vec![pembed(0)], vec![], vec![]),
            struct_node(3, vec![vembed(1), vembed(2)], vec![], vec![]),
        ],
        questions: vec![sel(3, "M")],
    }
}

/// `N2` embeds interface `N0` and struct `N1`, which also satisfies `N0`.
pub fn mixed_embedding() -> Scenario {
    Scenario {
        name: "mixed_embedding".into(),
        seed: 0,
        nodes: vec![
            iface_node(0, vec![], vec![smethod("speak")]),
            struct_node(1, vec![], vec![], vec![smethod("speak")]),
            struct_node(2, vec![vembed(0), vembed(1)], vec![], vec![]),
        ],
        questions: vec![
            sel(2, "Speak"),
            Question::Satisfies {
                type_id: 2,
                interface: 0,
            },
        ],
    }
}

/// `struct Box<T> { Value: T }` with `fn Tag(self) -> string`: a generic base
/// whose field is the parameter and whose method is monomorphic. The random
/// generator stays monomorphic, so generic shapes are written here.
fn generic_box(id: NodeId) -> Node {
    Node {
        id,
        name: format!("N{id}"),
        type_params: vec!["T".into()],
        kind: NodeKind::Struct {
            fields: vec![Field {
                name: cased("value", Visibility::Public),
                member_type: MemberType::TypeParam("T".into()),
                visibility: Visibility::Public,
            }],
            embeds: vec![],
            methods: vec![smethod("tag")],
        },
        origin: Origin::Native,
    }
}

fn vembed_arg(target: NodeId, arg: MemberType) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Value,
        storage: Storage::Plain,
        type_args: vec![arg],
    }
}

fn pembed_arg(target: NodeId, arg: MemberType) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Pointer,
        storage: Storage::Plain,
        type_args: vec![arg],
    }
}

/// `N1` embeds `Box<int>` by value. Go promotes the method `Tag` and the field
/// `Value` (instantiated to `int`) at depth 1; Lisette must agree.
pub fn generic_embed_promotes() -> Scenario {
    Scenario {
        name: "generic_embed_promotes".into(),
        seed: 0,
        nodes: vec![
            generic_box(0),
            struct_node(
                1,
                vec![vembed_arg(0, MemberType::Basic(BasicType::Int))],
                vec![],
                vec![],
            ),
        ],
        questions: vec![
            sel(1, "Tag"),
            Question::Selector {
                root: 1,
                member: "Value".into(),
                kind: SelKind::Field,
            },
        ],
    }
}

/// `N1` embeds `*Box<int>`. The method still promotes through the pointer edge.
pub fn generic_embed_pointer() -> Scenario {
    Scenario {
        name: "generic_embed_pointer".into(),
        seed: 0,
        nodes: vec![
            generic_box(0),
            struct_node(
                1,
                vec![pembed_arg(0, MemberType::Basic(BasicType::Int))],
                vec![],
                vec![],
            ),
        ],
        questions: vec![sel(1, "Tag")],
    }
}

/// `N2` embeds `Box<int>` and satisfies interface `N0` through the promoted
/// `Tag`.
pub fn generic_embed_satisfies() -> Scenario {
    Scenario {
        name: "generic_embed_satisfies".into(),
        seed: 0,
        nodes: vec![
            iface_node(0, vec![], vec![imethod("tag", BasicType::String)]),
            generic_box(1),
            struct_node(
                2,
                vec![vembed_arg(1, MemberType::Basic(BasicType::Int))],
                vec![],
                vec![],
            ),
        ],
        questions: vec![Question::Satisfies {
            type_id: 2,
            interface: 0,
        }],
    }
}

/// `image.Point` as an imported node: a flat stdlib struct (`X`/`Y`, value `String`).
fn image_point() -> Node {
    Node {
        id: 0,
        name: "Point".into(),
        type_params: vec![],
        kind: NodeKind::Struct {
            fields: vec![
                Field {
                    name: "X".into(),
                    member_type: MemberType::Basic(BasicType::Int),
                    visibility: Visibility::Public,
                },
                Field {
                    name: "Y".into(),
                    member_type: MemberType::Basic(BasicType::Int),
                    visibility: Visibility::Public,
                },
            ],
            embeds: vec![],
            methods: vec![Method {
                name: "String".into(),
                receiver: Receiver::Value,
                signature: Signature {
                    parameters: vec![],
                    return_type: MemberType::Basic(BasicType::String),
                },
                visibility: Visibility::Public,
            }],
        },
        origin: Origin::Imported {
            pkg: "image".into(),
        },
    }
}

/// Fixture for the `Origin::Imported` renderer test; not yet in the differential.
pub fn imported_struct_embed() -> Scenario {
    Scenario {
        name: "imported_struct_embed".into(),
        seed: 0,
        nodes: vec![
            image_point(),
            struct_node(1, vec![vembed(0)], vec![], vec![]),
        ],
        questions: vec![
            Question::Selector {
                root: 1,
                member: "X".into(),
                kind: SelKind::Field,
            },
            sel(1, "String"),
        ],
    }
}

/// Every scenario that goes through the full differential.
pub fn differential_scenarios() -> Vec<Scenario> {
    let mut scenarios = super::fixtures::all();
    scenarios.push(super::fixtures::interface_conflict());
    scenarios.push(pointer_cycle());
    scenarios.push(pointer_diamond());
    scenarios.push(mixed_embedding());
    scenarios.push(generic_embed_promotes());
    scenarios.push(generic_embed_pointer());
    scenarios.push(generic_embed_satisfies());
    scenarios
}

pub struct RejectCase {
    pub name: &'static str,
    pub source: &'static str,
    pub code: &'static str,
    /// `false` for a rejection a later phase is expected to lift.
    pub permanent: bool,
}

pub fn reject_cases() -> Vec<RejectCase> {
    vec![
        RejectCase {
            name: "pointer_to_interface",
            source: "interface I {\n  fn m(self) -> int\n}\nstruct Outer {\n  embed Ref<I>,\n}\n",
            code: "infer.embed_pointer_to_interface",
            permanent: true,
        },
        RejectCase {
            name: "nested_ref",
            source: "struct Base {\n  pub x: int,\n}\nstruct Outer {\n  embed Ref<Ref<Base>>,\n}\n",
            code: "infer.embed_nested_ref",
            permanent: true,
        },
        RejectCase {
            name: "option_target",
            source: "struct Base {\n  pub x: int,\n}\nstruct Outer {\n  embed Option<Base>,\n}\n",
            code: "infer.embed_option_target",
            permanent: false,
        },
        RejectCase {
            name: "defined_type",
            source: "struct MyInt(int)\nstruct Outer {\n  embed MyInt,\n}\n",
            code: "infer.embed_defined_type",
            permanent: false,
        },
        RejectCase {
            name: "no_surface",
            source: "struct Empty {}\nstruct Outer {\n  embed Empty,\n}\n",
            code: "infer.embed_no_surface",
            permanent: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::lisette_answer::declarations_register_cleanly;

    #[test]
    fn some_reject_cases_are_deferred() {
        let deferred = reject_cases().iter().filter(|case| !case.permanent).count();
        assert!(deferred > 0, "expected some deferred reject cases");
    }

    #[test]
    fn corpus_scenarios_validate() {
        for scenario in differential_scenarios() {
            scenario
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", scenario.name));
        }
    }

    #[test]
    fn pointer_cycle_decls_register() {
        declarations_register_cleanly(&pointer_cycle())
            .unwrap_or_else(|codes| panic!("pointer_cycle decls rejected: {codes:?}"));
    }

    #[test]
    fn reject_cases_raise_their_codes() {
        for case in reject_cases() {
            let codes = crate::_embed_harness::lisette_answer::check_codes(case.source)
                .unwrap_or_else(|| panic!("{}: expected a rejection, but it compiled", case.name));
            assert!(
                codes.iter().any(|c| c == case.code),
                "{}: expected `{}`, got {codes:?}",
                case.name,
                case.code
            );
        }
    }
}
