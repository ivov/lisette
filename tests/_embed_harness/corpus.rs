//! Hand-written test cases the generator cannot produce: cyclic graphs, the
//! conflicting-interface case, and inputs that must be rejected with a specific
//! `embed_*` error. Some rejections are expected to be lifted later, and their
//! tests will start passing when that happens.

use super::fixtures::{iface_node, smethod, struct_node, vembed};
use super::scenario::*;

fn pembed(target: NodeId) -> Embed {
    Embed {
        target,
        edge: EdgeKind::Pointer,
        storage: Storage::Plain,
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

/// Every scenario that goes through the full differential.
pub fn differential_scenarios() -> Vec<Scenario> {
    let mut scenarios = super::fixtures::all();
    scenarios.push(super::fixtures::interface_conflict());
    scenarios.push(pointer_cycle());
    scenarios.push(pointer_diamond());
    scenarios.push(mixed_embedding());
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
            name: "generic",
            source: "struct Box<T> {\n  pub value: T,\n}\nstruct Outer {\n  embed Box<int>,\n}\n",
            code: "infer.embed_generic",
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
