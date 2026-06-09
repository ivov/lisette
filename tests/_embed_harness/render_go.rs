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
        GoMode::RunMain(_) => out.push_str("package main\n\n"),
    }
    emit_go_imports(scenario, &mode, &mut out);

    for node in &scenario.nodes {
        render_node(scenario, node, &mut out);
        out.push('\n');
    }

    if let GoMode::RunMain(printed) = mode {
        render_main(scenario, printed, &mut out);
    }
    out
}

fn emit_go_imports(scenario: &Scenario, mode: &GoMode, out: &mut String) {
    let mut paths: Vec<&str> = Vec::new();
    if matches!(mode, GoMode::RunMain(_)) {
        paths.push("fmt");
    }
    for node in &scenario.nodes {
        if let Some(pkg) = node.origin.pkg()
            && !paths.contains(&pkg)
        {
            paths.push(pkg);
        }
    }
    match paths.as_slice() {
        [] => {}
        [one] => out.push_str(&format!("import \"{one}\"\n\n")),
        many => {
            out.push_str("import (\n");
            for path in many {
                out.push_str(&format!("\t\"{path}\"\n"));
            }
            out.push_str(")\n\n");
        }
    }
}

fn go_generics(node: &Node) -> String {
    if node.type_params.is_empty() {
        return String::new();
    }
    let params: Vec<String> = node
        .type_params
        .iter()
        .map(|p| format!("{p} any"))
        .collect();
    format!("[{}]", params.join(", "))
}

/// A node reference with optional type arguments: `Box` or `Box[int]`.
fn go_node_ref(scenario: &Scenario, target: NodeId, type_args: &[MemberType]) -> String {
    let name = scenario.node_ref(target);
    if type_args.is_empty() {
        return name;
    }
    let args: Vec<String> = type_args.iter().map(|a| go_type(scenario, a)).collect();
    format!("{name}[{}]", args.join(", "))
}

fn render_node(scenario: &Scenario, node: &Node, out: &mut String) {
    if node.origin.pkg().is_some() {
        return; // imported nodes are referenced, not defined
    }
    let generics = go_generics(node);
    match &node.kind {
        NodeKind::Struct {
            fields,
            embeds,
            methods,
        } => {
            out.push_str(&format!("type {}{generics} struct {{\n", node.name));
            for embed in embeds {
                let target = go_node_ref(scenario, embed.target, &embed.type_args);
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
                render_method(scenario, node, method, out);
            }
        }
        NodeKind::Interface { methods, embeds } => {
            out.push_str(&format!("type {}{generics} interface {{\n", node.name));
            for embed in embeds {
                out.push_str(&format!(
                    "\t{}\n",
                    go_node_ref(scenario, embed.target, &embed.type_args)
                ));
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
                render_method(scenario, node, method, out);
            }
        }
    }
}

/// The receiver's type parameters without constraints: `[T]` or empty. Go names
/// the params on a generic receiver but omits the `any` bound there.
fn go_receiver_params(node: &Node) -> String {
    if node.type_params.is_empty() {
        return String::new();
    }
    format!("[{}]", node.type_params.join(", "))
}

fn render_method(scenario: &Scenario, node: &Node, method: &Method, out: &mut String) {
    let owner = &node.name;
    let recv_params = go_receiver_params(node);
    let receiver = match method.receiver {
        Receiver::Value => format!("r {owner}{recv_params}"),
        Receiver::Pointer => format!("r *{owner}{recv_params}"),
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
        MemberType::TypeParam(name) => name.clone(),
        MemberType::Node(id) => scenario.node_ref(*id),
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
