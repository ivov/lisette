//! Turns a graph into Lisette source, in three forms: the declarations alone;
//! the declarations plus one function per question (so Lisette's answer can be read
//! from compiler errors); and a runnable `main`. Names and structure match the
//! Go renderer.

use std::collections::BTreeSet;

use super::PrintedQuestion;
use super::scenario::*;

/// Byte ranges of the question functions, aligned 1:1 with `scenario.questions`, so a
/// diagnostic can be attributed to the question that produced it.
pub enum QuestionSpans {
    Selector {
        range: (usize, usize),
    },
    Satisfies {
        value: (usize, usize),
        pointer: (usize, usize),
    },
}

pub struct RenderedQuestions {
    pub source: String,
    pub spans: Vec<QuestionSpans>,
}

/// Type declarations and `impl` blocks only.
pub fn render_lis_declarations(scenario: &Scenario) -> String {
    let mut out = String::new();
    push_declarations(scenario, &mut out);
    out
}

pub fn render_lis_questions(scenario: &Scenario) -> RenderedQuestions {
    let mut out = String::new();
    push_declarations(scenario, &mut out);

    let mut interfaces: BTreeSet<NodeId> = BTreeSet::new();
    for question in &scenario.questions {
        if let Question::Satisfies { interface, .. } = question {
            interfaces.insert(*interface);
        }
    }
    for interface in &interfaces {
        out.push_str(&format!(
            "fn {}(_v: {}) {{\n}}\n\n",
            sink_name(scenario.node_name(*interface)),
            scenario.node_name(*interface),
        ));
    }

    let mut spans = Vec::with_capacity(scenario.questions.len());
    for (i, question) in scenario.questions.iter().enumerate() {
        match question {
            Question::Selector { root, member, kind } => {
                let access = match kind {
                    SelKind::Method => format!("r.{member}()"),
                    SelKind::Field => format!("r.{member}"),
                };
                let start = out.len();
                out.push_str(&format!(
                    "fn __sel_{i}(r: Ref<{}>) {{\n  let _ = {access}\n}}\n\n",
                    scenario.node_name(*root),
                ));
                let end = out.len();
                spans.push(QuestionSpans::Selector {
                    range: (start, end),
                });
            }
            Question::Satisfies { type_id, interface } => {
                let sink = sink_name(scenario.node_name(*interface));
                let type_name = scenario.node_name(*type_id);

                let v_start = out.len();
                out.push_str(&format!(
                    "fn __sat_v_{i}(x: {type_name}) {{\n  {sink}(x)\n}}\n\n",
                ));
                let v_end = out.len();

                let p_start = out.len();
                out.push_str(&format!(
                    "fn __sat_p_{i}(x: Ref<{type_name}>) {{\n  {sink}(x)\n}}\n\n",
                ));
                let p_end = out.len();

                spans.push(QuestionSpans::Satisfies {
                    value: (v_start, v_end),
                    pointer: (p_start, p_end),
                });
            }
        }
    }

    RenderedQuestions { source: out, spans }
}

pub fn render_lis_run(scenario: &Scenario, printed: &[PrintedQuestion]) -> String {
    let mut out = String::new();
    out.push_str("import \"go:fmt\"\n\n");
    push_declarations(scenario, &mut out);

    out.push_str("fn main() {\n");
    for (i, question) in printed.iter().enumerate() {
        out.push_str(&format!(
            "  let r{i} = {}\n",
            lis_zero_construct(scenario, question.root)
        ));
        out.push_str(&format!("  fmt.Println(r{i}.{}())\n", question.member));
        out.push_str(&format!("  let f{i} = r{i}.{}\n", question.member));
        out.push_str(&format!("  fmt.Println(f{i}())\n"));
    }
    out.push_str("}\n");
    out
}

fn push_declarations(scenario: &Scenario, out: &mut String) {
    emit_lis_imports(scenario, out);
    for node in &scenario.nodes {
        render_node(scenario, node, out);
        out.push('\n');
    }
}

/// Renders only the `import "go:<pkg>"`; the stdlib typedef is auto-loaded.
fn emit_lis_imports(scenario: &Scenario, out: &mut String) {
    let mut pkgs: Vec<&str> = Vec::new();
    for node in &scenario.nodes {
        if let Some(pkg) = node.origin.pkg()
            && !pkgs.contains(&pkg)
        {
            pkgs.push(pkg);
        }
    }
    for pkg in &pkgs {
        out.push_str(&format!("import \"go:{pkg}\"\n"));
    }
    if !pkgs.is_empty() {
        out.push('\n');
    }
}

fn lis_generics(node: &Node) -> String {
    if node.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", node.type_params.join(", "))
    }
}

fn render_node(scenario: &Scenario, node: &Node, out: &mut String) {
    if node.origin.pkg().is_some() {
        return; // imported nodes are referenced, not defined
    }
    let generics = lis_generics(node);
    match &node.kind {
        NodeKind::Struct {
            fields,
            embeds,
            methods,
        } => {
            if embeds.is_empty() && fields.is_empty() {
                out.push_str(&format!("struct {}{generics} {{}}\n", node.name));
            } else {
                out.push_str(&format!("struct {}{generics} {{\n", node.name));
                for embed in embeds {
                    out.push_str(&format!("  embed {},\n", embed_type(scenario, embed)));
                }
                for field in fields {
                    let visibility = match field.visibility {
                        Visibility::Public => "pub ",
                        Visibility::Private => "",
                    };
                    out.push_str(&format!(
                        "  {visibility}{}: {},\n",
                        field.name,
                        lis_type(scenario, &field.member_type)
                    ));
                }
                out.push_str("}\n");
            }
            render_impl(scenario, node, methods, out);
        }
        NodeKind::Interface { methods, embeds } => {
            if embeds.is_empty() && methods.is_empty() {
                out.push_str(&format!("interface {}{generics} {{}}\n", node.name));
            } else {
                out.push_str(&format!("interface {}{generics} {{\n", node.name));
                for embed in embeds {
                    out.push_str(&format!(
                        "  embed {}\n",
                        lis_node_ref(scenario, embed.target, &embed.type_args)
                    ));
                }
                for method in methods {
                    out.push_str(&format!(
                        "  fn {}(self{}) -> {}\n",
                        method.name,
                        lis_parameters(scenario, &method.signature.parameters),
                        lis_type(scenario, &method.signature.return_type),
                    ));
                }
                out.push_str("}\n");
            }
        }
        NodeKind::NamedBasic {
            underlying,
            methods,
        } => {
            // A tuple struct, not `type N = T`: an alias cannot carry methods.
            out.push_str(&format!("struct {}({})\n", node.name, underlying.lisette()));
            render_impl(scenario, node, methods, out);
        }
    }
}

fn render_impl(scenario: &Scenario, node: &Node, methods: &[Method], out: &mut String) {
    if methods.is_empty() {
        return;
    }
    let owner = &node.name;
    let generics = lis_generics(node);
    out.push_str(&format!("impl{generics} {owner}{generics} {{\n"));
    for method in methods {
        let receiver = match method.receiver {
            Receiver::Value => "self".to_string(),
            Receiver::Pointer => format!("self: Ref<{owner}{generics}>"),
        };
        let extra = lis_parameters(scenario, &method.signature.parameters);
        match &method.signature.return_type {
            MemberType::Basic(BasicType::String) => {
                out.push_str(&format!(
                    "  fn {}({receiver}{extra}) -> string {{\n    \"{}.{}\"\n  }}\n",
                    method.name, owner, method.name,
                ));
            }
            other => panic!(
                "concrete method {}.{} must return string for identity (got {other:?})",
                owner, method.name
            ),
        }
    }
    out.push_str("}\n");
}

fn lis_parameters(scenario: &Scenario, parameters: &[MemberType]) -> String {
    if parameters.is_empty() {
        return String::new();
    }
    let rendered: Vec<String> = parameters
        .iter()
        .enumerate()
        .map(|(i, member_type)| format!("p{i}: {}", lis_type(scenario, member_type)))
        .collect();
    format!(", {}", rendered.join(", "))
}

fn embed_type(scenario: &Scenario, embed: &Embed) -> String {
    let target = lis_node_ref(scenario, embed.target, &embed.type_args);
    match (embed.edge, embed.storage) {
        (EdgeKind::Value, Storage::Plain) => target,
        (EdgeKind::Pointer, Storage::Plain) => format!("Ref<{target}>"),
        (EdgeKind::Value, Storage::Option) => format!("Option<{target}>"),
        (EdgeKind::Pointer, Storage::OptionPointer) => format!("Option<Ref<{target}>>"),
        (edge, storage) => panic!("invalid embed edge/storage pairing: {edge:?}/{storage:?}"),
    }
}

/// A node reference with optional type arguments: `Box` or `Box<int>`.
fn lis_node_ref(scenario: &Scenario, target: NodeId, type_args: &[MemberType]) -> String {
    let name = scenario.node_ref(target);
    if type_args.is_empty() {
        return name;
    }
    let args: Vec<String> = type_args.iter().map(|a| lis_type(scenario, a)).collect();
    format!("{name}<{}>", args.join(", "))
}

pub fn lis_type(scenario: &Scenario, member_type: &MemberType) -> String {
    match member_type {
        MemberType::Basic(basic) => basic.lisette().to_string(),
        MemberType::Node(id) => scenario.node_ref(*id),
        MemberType::Ref(inner) => format!("Ref<{}>", lis_type(scenario, inner)),
        MemberType::Slice(inner) => format!("Slice<{}>", lis_type(scenario, inner)),
        MemberType::Option(inner) => format!("Option<{}>", lis_type(scenario, inner)),
        MemberType::TypeParam(name) => name.clone(),
    }
}

fn lis_zero_construct(scenario: &Scenario, id: NodeId) -> String {
    let node = scenario.node(id);
    match &node.kind {
        NodeKind::Struct { .. } => format!("{} {{ .. }}", node.name),
        NodeKind::NamedBasic { underlying, .. } => {
            format!("{}({})", node.name, basic_zero_lit(*underlying))
        }
        NodeKind::Interface { .. } => {
            panic!(
                "cannot construct interface root `{}` for the build arm",
                node.name
            )
        }
    }
}

fn basic_zero_lit(basic: BasicType) -> &'static str {
    match basic {
        BasicType::Int | BasicType::Byte | BasicType::Rune => "0",
        BasicType::Float => "0.0",
        BasicType::String => "\"\"",
        BasicType::Bool => "false",
    }
}

fn sink_name(interface: &str) -> String {
    format!("__sink_{interface}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;
    use crate::_embed_harness::render_go::{GoMode, render_go};

    fn assert_snap(name: &str, value: String) {
        insta::with_settings!({ prepend_module_to_snapshot => false, omit_expression => true }, {
            insta::assert_snapshot!(name, value);
        });
    }

    #[test]
    fn declarations_diamond() {
        let scenario = fixtures::diamond();
        assert_snap(
            "lis_declarations_diamond",
            render_lis_declarations(&scenario),
        );
    }

    #[test]
    fn declarations_interface() {
        let scenario = fixtures::interface_direct_satisfaction();
        assert_snap(
            "lis_declarations_interface",
            render_lis_declarations(&scenario),
        );
    }

    #[test]
    fn questions_value_embed() {
        let scenario = fixtures::value_embed_method();
        assert_snap(
            "lis_questions_value_embed",
            render_lis_questions(&scenario).source,
        );
    }

    #[test]
    fn questions_satisfaction() {
        let scenario = fixtures::interface_promoted_satisfaction();
        assert_snap(
            "lis_questions_satisfaction",
            render_lis_questions(&scenario).source,
        );
    }

    #[test]
    fn run_direct() {
        let scenario = fixtures::direct_method();
        let printed = [PrintedQuestion {
            root: 0,
            member: "M".into(),
        }];
        assert_snap("lis_run_direct", render_lis_run(&scenario, &printed));
    }

    #[test]
    fn question_spans_align() {
        for scenario in fixtures::all() {
            let rendered = render_lis_questions(&scenario);
            assert_eq!(
                rendered.spans.len(),
                scenario.questions.len(),
                "{}: span count mismatch",
                scenario.name
            );
            for (question, span) in scenario.questions.iter().zip(&rendered.spans) {
                match (question, span) {
                    (Question::Selector { .. }, QuestionSpans::Selector { range }) => {
                        check_range(&rendered.source, *range, "__sel_");
                    }
                    (Question::Satisfies { .. }, QuestionSpans::Satisfies { value, pointer }) => {
                        check_range(&rendered.source, *value, "__sat_v_");
                        check_range(&rendered.source, *pointer, "__sat_p_");
                    }
                    _ => panic!("{}: question/span kind mismatch", scenario.name),
                }
            }
        }
    }

    fn check_range(source: &str, (start, end): (usize, usize), needle: &str) {
        assert!(start < end && end <= source.len(), "range out of bounds");
        assert!(
            source[start..end].contains(needle),
            "range does not contain `{needle}`"
        );
    }

    #[test]
    fn rendered_lisette_parses() {
        for scenario in fixtures::all() {
            let struct_root = scenario.nodes.iter().find(|n| !n.kind.is_interface());
            let printed: Vec<PrintedQuestion> = struct_root
                .map(|n| {
                    vec![PrintedQuestion {
                        root: n.id,
                        member: "M".into(),
                    }]
                })
                .unwrap_or_default();
            for (label, src) in [
                ("declarations", render_lis_declarations(&scenario)),
                ("questions", render_lis_questions(&scenario).source),
                ("run", render_lis_run(&scenario, &printed)),
            ] {
                let result = syntax::build_ast(&src, 0);
                assert!(
                    !result.failed(),
                    "{} ({label}) failed to parse: {:?}\n--- source ---\n{src}",
                    scenario.name,
                    result.errors
                );
            }
        }
    }

    #[test]
    fn multi_member_interface_parses() {
        let src = "interface N1 {\n  fn C(self) -> bool\n}\n\n\
                   interface N0 {\n  embed N1\n  fn A(self) -> int\n  fn B(self) -> string\n}\n";
        let result = syntax::build_ast(src, 0);
        assert!(
            !result.failed(),
            "multi-member interface parse: {:?}",
            result.errors
        );
    }

    #[test]
    fn generic_node_renders_in_both_languages() {
        let scenario = crate::_embed_harness::corpus::generic_embed_promotes();

        let lis = render_lis_declarations(&scenario);
        assert!(lis.contains("struct N0<T>"), "lisette header:\n{lis}");
        assert!(lis.contains("impl<T> N0<T>"), "lisette impl:\n{lis}");
        assert!(lis.contains("embed N0<int>"), "lisette embed:\n{lis}");

        let go = render_go(&scenario, GoMode::TypeDefs);
        assert!(go.contains("type N0[T any] struct"), "go header:\n{go}");
        assert!(go.contains("func (r N0[T]) Tag()"), "go receiver:\n{go}");
        assert!(go.contains("N0[int]"), "go embed:\n{go}");
    }

    #[test]
    fn imported_node_renders_as_package_reference() {
        let scenario = crate::_embed_harness::corpus::imported_struct_embed();

        let lis = render_lis_declarations(&scenario);
        assert!(
            lis.contains("import \"go:image\""),
            "lisette import:\n{lis}"
        );
        assert!(lis.contains("embed image.Point"), "lisette embed:\n{lis}");
        assert!(
            !lis.contains("struct Point"),
            "imported Point must not be defined:\n{lis}"
        );

        let go = render_go(&scenario, GoMode::TypeDefs);
        assert!(go.contains("import \"image\""), "go import:\n{go}");
        assert!(go.contains("\timage.Point\n"), "go anonymous field:\n{go}");
        assert!(
            !go.contains("type Point struct"),
            "imported Point must not be defined:\n{go}"
        );
    }

    #[test]
    fn generic_interface_parent_renders_type_args_in_both_languages() {
        let parent = Node {
            id: 0,
            name: "N0".into(),
            type_params: vec!["T".into()],
            kind: NodeKind::Interface {
                methods: vec![Method {
                    name: "Get".into(),
                    receiver: Receiver::Value,
                    signature: Signature {
                        parameters: vec![],
                        return_type: MemberType::TypeParam("T".into()),
                    },
                    visibility: Visibility::Public,
                }],
                embeds: vec![],
            },
            origin: Origin::Native,
        };
        let child = Node {
            id: 1,
            name: "N1".into(),
            type_params: vec![],
            kind: NodeKind::Interface {
                methods: vec![],
                embeds: vec![Embed {
                    target: 0,
                    edge: EdgeKind::Value,
                    storage: Storage::Plain,
                    type_args: vec![MemberType::Basic(BasicType::Int)],
                }],
            },
            origin: Origin::Native,
        };
        let scenario = Scenario {
            name: "generic_iface_parent".into(),
            seed: 0,
            nodes: vec![parent, child],
            questions: vec![],
        };
        scenario.validate().unwrap();

        let lis = render_lis_declarations(&scenario);
        assert!(lis.contains("embed N0<int>"), "lisette embed:\n{lis}");

        let go = render_go(&scenario, GoMode::TypeDefs);
        assert!(go.contains("N0[int]"), "go embed:\n{go}");
    }

    #[test]
    fn renderers_agree_on_node_names() {
        for scenario in fixtures::all() {
            let go = render_go(&scenario, GoMode::TypeDefs);
            let lis = render_lis_declarations(&scenario);
            for node in &scenario.nodes {
                assert!(
                    go.contains(&node.name) && lis.contains(&node.name),
                    "{}: renderers disagree on node `{}`",
                    scenario.name,
                    node.name
                );
            }
        }
    }
}
