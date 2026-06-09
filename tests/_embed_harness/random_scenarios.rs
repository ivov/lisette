//! Generates random graphs from a seed (same seed, same graph, so any failure
//! reproduces). Embeds always point to an earlier type, which keeps every graph
//! acyclic and therefore valid Go; cycles and other special shapes are written
//! by hand in `corpus.rs` instead.

use super::scenario::*;

/// splitmix64: one `u64` of state, reproducible from the seed alone.
pub struct Random {
    state: u64,
}

impl Random {
    pub fn new(seed: u64) -> Self {
        Random { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (returns 0 for `n == 0`, never panics).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    pub fn one_in(&mut self, n: u64) -> bool {
        self.next_u64().is_multiple_of(n)
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let index = self.below(items.len());
        &items[index]
    }
}

/// Generate a scenario from a seed. Deterministic: same seed, same scenario.
pub fn generate(seed: u64) -> Scenario {
    let mut random = Random::new(seed);
    let node_count = 2 + random.below(6); // 2..=7

    let mut nodes: Vec<Node> = Vec::with_capacity(node_count);
    for id in 0..node_count {
        let kind = generate_kind(&mut random, id, &nodes);
        nodes.push(Node {
            id,
            name: format!("N{id}"),
            type_params: vec![],
            kind,
            origin: Origin::Native,
        });
    }

    let questions = generate_questions(&mut random, &nodes);
    let scenario = Scenario {
        name: format!("generate_{seed}"),
        seed,
        nodes,
        questions,
    };
    debug_assert!(
        scenario.validate().is_ok(),
        "generated invalid scenario: {seed}"
    );
    scenario
}

fn generate_kind(random: &mut Random, id: usize, existing: &[Node]) -> NodeKind {
    match random.below(10) {
        0..=5 => NodeKind::Struct {
            embeds: generate_struct_embeds(random, id, existing),
            fields: generate_fields(random),
            methods: generate_methods(random),
        },
        6..=7 => NodeKind::Interface {
            embeds: generate_interface_embeds(random, id, existing),
            methods: generate_interface_methods(random),
        },
        _ => NodeKind::NamedBasic {
            underlying: *random.pick(&[
                BasicType::Int,
                BasicType::Float,
                BasicType::String,
                BasicType::Bool,
            ]),
            methods: generate_methods(random),
        },
    }
}

fn generate_struct_embeds(random: &mut Random, id: usize, existing: &[Node]) -> Vec<Embed> {
    let candidates: Vec<usize> = (0..id).filter(|&j| embeddable(&existing[j])).collect();
    if candidates.is_empty() {
        return vec![];
    }
    let mut embeds = Vec::new();
    let mut used = Vec::new();
    for _ in 0..random.below(3) {
        let target = *random.pick(&candidates);
        if used.contains(&target) {
            continue;
        }
        used.push(target);
        let edge = if existing[target].kind.is_interface() || random.one_in(2) {
            EdgeKind::Value
        } else {
            EdgeKind::Pointer
        };
        embeds.push(Embed {
            target,
            edge,
            storage: Storage::Plain,
            type_args: vec![],
        });
    }
    embeds
}

fn generate_interface_embeds(random: &mut Random, id: usize, existing: &[Node]) -> Vec<Embed> {
    let interfaces: Vec<usize> = (0..id)
        .filter(|&j| existing[j].kind.is_interface() && embeddable(&existing[j]))
        .collect();
    if interfaces.is_empty() {
        return vec![];
    }
    let mut embeds = Vec::new();
    let mut used = Vec::new();
    for _ in 0..random.below(3) {
        let target = *random.pick(&interfaces);
        if used.contains(&target) {
            continue;
        }
        used.push(target);
        embeds.push(Embed {
            target,
            edge: EdgeKind::Value,
            storage: Storage::Plain,
            type_args: vec![],
        });
    }
    embeds
}

/// Whether this node can be embedded today without a declaration rejection.
fn embeddable(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Struct {
            fields, methods, ..
        } => !fields.is_empty() || !methods.is_empty(),
        NodeKind::Interface { methods, .. } => !methods.is_empty(),
        NodeKind::NamedBasic { .. } => false,
    }
}

/// Identity methods named from a small pool, so distinct nodes can share a name
/// (the source of diamonds and ambiguity when both feed a common descendant).
fn generate_methods(random: &mut Random) -> Vec<Method> {
    let mut methods = Vec::new();
    let mut used = Vec::new();
    for _ in 0..random.below(3) {
        let name = method_name(random);
        if used.contains(&name) {
            continue;
        }
        used.push(name.clone());
        methods.push(Method {
            name,
            receiver: if random.one_in(2) {
                Receiver::Pointer
            } else {
                Receiver::Value
            },
            signature: Signature {
                parameters: vec![],
                return_type: MemberType::Basic(BasicType::String),
            },
            visibility: Visibility::Public,
        });
    }
    methods
}

fn generate_interface_methods(random: &mut Random) -> Vec<Method> {
    let mut methods = Vec::new();
    let mut used = Vec::new();
    for _ in 0..(1 + random.below(2)) {
        let name = method_name(random);
        if used.contains(&name) {
            continue;
        }
        used.push(name.clone());
        methods.push(Method {
            name,
            receiver: Receiver::Value,
            signature: Signature {
                parameters: vec![],
                return_type: MemberType::Basic(BasicType::String),
            },
            visibility: Visibility::Public,
        });
    }
    methods
}

fn generate_fields(random: &mut Random) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut used = Vec::new();
    for _ in 0..random.below(2) {
        let name = field_name(random);
        if used.contains(&name) {
            continue;
        }
        used.push(name.clone());
        let basic = *random.pick(&[BasicType::Int, BasicType::String, BasicType::Bool]);
        let member_type = match random.below(3) {
            0 => MemberType::Ref(Box::new(MemberType::Basic(basic))),
            1 => MemberType::Slice(Box::new(MemberType::Basic(basic))),
            _ => MemberType::Basic(basic),
        };
        fields.push(Field {
            name,
            member_type,
            visibility: Visibility::Public,
        });
    }
    fields
}

fn generate_questions(random: &mut Random, nodes: &[Node]) -> Vec<Question> {
    let roots: Vec<usize> = nodes
        .iter()
        .filter(|n| !n.kind.is_interface())
        .map(|n| n.id)
        .collect();
    let interfaces: Vec<usize> = nodes
        .iter()
        .filter(|n| n.kind.is_interface())
        .map(|n| n.id)
        .collect();

    let mut questions = Vec::new();
    if !roots.is_empty() {
        for _ in 0..(1 + random.below(3)) {
            let root = *random.pick(&roots);
            questions.push(Question::Selector {
                root,
                member: method_name(random),
                kind: SelKind::Method,
            });
            if random.one_in(2) {
                questions.push(Question::Selector {
                    root,
                    member: field_name(random),
                    kind: SelKind::Field,
                });
            }
        }
        for _ in 0..random.below(3) {
            if interfaces.is_empty() {
                break;
            }
            questions.push(Question::Satisfies {
                type_id: *random.pick(&roots),
                interface: *random.pick(&interfaces),
            });
        }
    }
    questions
}

fn method_name(random: &mut Random) -> String {
    cased(&format!("m{}", random.below(3)), Visibility::Public)
}

fn field_name(random: &mut Random) -> String {
    cased(&format!("f{}", random.below(2)), Visibility::Public)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        for seed in [0u64, 1, 42, 9999, u64::MAX] {
            assert_eq!(
                generate(seed),
                generate(seed),
                "seed {seed} produced different scenarios"
            );
        }
    }

    #[test]
    fn distinct_seeds_differ() {
        assert_ne!(generate(1), generate(2));
    }

    #[test]
    fn generated_scenarios_validate() {
        for seed in 0..500 {
            let scenario = generate(seed);
            scenario
                .validate()
                .unwrap_or_else(|e| panic!("seed {seed} invalid: {e}"));
        }
    }

    #[test]
    fn generated_scenarios_register_cleanly() {
        use crate::_embed_harness::lisette_answer::declarations_register_cleanly;
        for seed in 0..500 {
            declarations_register_cleanly(&generate(seed))
                .unwrap_or_else(|codes| panic!("seed {seed} has declaration errors: {codes:?}"));
        }
    }

    #[test]
    fn generated_scenarios_have_questions_mostly() {
        let with_questions = (0..200)
            .filter(|&s| !generate(s).questions.is_empty())
            .count();
        assert!(
            with_questions > 150,
            "too few generated scenarios had questions: {with_questions}"
        );
    }
}
