//! Turns a graph into Go source. Uses the same type and member names as the
//! Lisette renderer, so if the two behave differently it is a real difference,
//! not a naming mismatch.

use super::PrintedQuestion;
use super::scenario::*;

pub enum GoMode<'a> {
    TypeDefs,
    RunMain(&'a [PrintedQuestion]),
}

pub fn render_go(scenario: &Scenario, mode: GoMode) -> String {
    let mut out = String::new();
    match mode {
        GoMode::TypeDefs => out.push_str("package p\n\n"),
        GoMode::RunMain(_) => out.push_str("package main\n\nimport \"fmt\"\n\n"),
    }

    for node in &scenario.nodes {
        render_node(scenario, node, &mut out);
        out.push('\n');
    }

    if let GoMode::RunMain(printed) = mode {
        render_main(scenario, printed, &mut out);
    }
    out
}

fn render_node(scenario: &Scenario, node: &Node, out: &mut String) {
    match &node.kind {
        NodeKind::Struct {
            fields,
            embeds,
            methods,
        } => {
            out.push_str(&format!("type {} struct {{\n", node.name));
            for embed in embeds {
                let target = scenario.node_name(embed.target);
                match embed.edge {
                    EdgeKind::Value => out.push_str(&format!("\t{target}\n")),
                    EdgeKind::Pointer => out.push_str(&format!("\t*{target}\n")),
                }
            }
            for field in fields {
                out.push_str(&format!(
                    "\t{} {}\n",
                    field.name,
                    go_type(scenario, &field.member_type)
                ));
            }
            out.push_str("}\n");
            for method in methods {
                render_method(scenario, &node.name, method, out);
            }
        }
        NodeKind::Interface { methods, embeds } => {
            out.push_str(&format!("type {} interface {{\n", node.name));
            for embed in embeds {
                out.push_str(&format!("\t{}\n", scenario.node_name(embed.target)));
            }
            for method in methods {
                out.push_str(&format!(
                    "\t{}({}) {}\n",
                    method.name,
                    go_params(scenario, &method.signature.parameters),
                    go_type(scenario, &method.signature.return_type),
                ));
            }
            out.push_str("}\n");
        }
        NodeKind::NamedBasic {
            underlying,
            methods,
        } => {
            out.push_str(&format!("type {} {}\n", node.name, underlying.go()));
            for method in methods {
                render_method(scenario, &node.name, method, out);
            }
        }
    }
}

fn render_method(scenario: &Scenario, owner: &str, method: &Method, out: &mut String) {
    let receiver = match method.receiver {
        Receiver::Value => format!("r {owner}"),
        Receiver::Pointer => format!("r *{owner}"),
    };
    out.push_str(&format!(
        "func ({}) {}({}) {} {{\n",
        receiver,
        method.name,
        go_params(scenario, &method.signature.parameters),
        go_type(scenario, &method.signature.return_type),
    ));
    match &method.signature.return_type {
        MemberType::Basic(BasicType::String) => {
            out.push_str(&format!("\treturn \"{}.{}\"\n", owner, method.name));
        }
        other => panic!(
            "concrete method {}.{} must return string for identity (got {other:?})",
            owner, method.name
        ),
    }
    out.push_str("}\n");
}

fn go_params(scenario: &Scenario, parameters: &[MemberType]) -> String {
    parameters
        .iter()
        .enumerate()
        .map(|(i, member_type)| format!("p{i} {}", go_type(scenario, member_type)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn go_type(scenario: &Scenario, member_type: &MemberType) -> String {
    match member_type {
        MemberType::Basic(basic) => basic.go().to_string(),
        MemberType::Node(id) => scenario.node_name(*id).to_string(),
        MemberType::Ref(inner) | MemberType::Option(inner) => {
            format!("*{}", go_type(scenario, inner))
        }
        MemberType::Slice(inner) => format!("[]{}", go_type(scenario, inner)),
    }
}

fn render_main(scenario: &Scenario, printed: &[PrintedQuestion], out: &mut String) {
    out.push_str("func main() {\n");
    for question in printed {
        let root = scenario.node_name(question.root);
        // Block-scope each question so `r` can be reused and stays addressable.
        out.push_str("\t{\n");
        out.push_str(&format!("\t\tvar r {root}\n"));
        out.push_str(&format!("\t\tfmt.Println(r.{}())\n", question.member));
        out.push_str(&format!("\t\tf := r.{}\n", question.member));
        out.push_str("\t\tfmt.Println(f())\n");
        out.push_str("\t}\n");
    }
    out.push_str("}\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::_embed_harness::fixtures;

    fn assert_snap(name: &str, value: String) {
        insta::with_settings!({ prepend_module_to_snapshot => false, omit_expression => true }, {
            insta::assert_snapshot!(name, value);
        });
    }

    #[test]
    fn declarations_diamond() {
        let scenario = fixtures::diamond();
        scenario.validate().unwrap();
        assert_snap(
            "go_declarations_diamond",
            render_go(&scenario, GoMode::TypeDefs),
        );
    }

    #[test]
    fn declarations_interface() {
        let scenario = fixtures::interface_direct_satisfaction();
        assert_snap(
            "go_declarations_interface",
            render_go(&scenario, GoMode::TypeDefs),
        );
    }

    #[test]
    fn run_main_direct() {
        let scenario = fixtures::direct_method();
        let printed = [PrintedQuestion {
            root: 0,
            member: "M".into(),
        }];
        assert_snap(
            "go_run_direct",
            render_go(&scenario, GoMode::RunMain(&printed)),
        );
    }

    #[test]
    fn every_node_name_appears() {
        for scenario in fixtures::all() {
            let go = render_go(&scenario, GoMode::TypeDefs);
            for node in &scenario.nodes {
                assert!(
                    go.contains(&node.name),
                    "{}: Go rendering omits node `{}`",
                    scenario.name,
                    node.name
                );
            }
        }
    }
}
