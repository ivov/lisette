//! Rewrites of a lowered body that the tree makes safe to decide.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::names::go_name;
use crate::plan::bodies::{
    AssignForm, BreakValueAction, BreakValuePlan, CompoundKind, Definition, ElseArm, LoopHeader,
    LoweredStatement, ReturnForm, SelectArmPlan, SwitchKind, for_each_statement,
    for_each_statements_mut,
};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::GoExpression;

pub(crate) fn clean_up(statements: &mut Vec<LoweredStatement>) {
    inline_name_aliases(statements);
    drop_unread_temps(statements);
}

#[derive(Default)]
struct NameUses {
    reads: HashMap<String, usize>,
    assigned: HashMap<String, usize>,
    writes: HashSet<String>,
    bindings: HashMap<String, usize>,
}

impl NameUses {
    fn of(statements: &[LoweredStatement]) -> Self {
        let mut uses = Self::default();
        for statement in statements {
            statement.visit_expressions(&mut |node| {
                if let GoExpressionNode::Identifier(text) | GoExpressionNode::Verbatim(text) = node
                {
                    uses.record_reads(text);
                }
            });
        }
        for_each_statement(statements, &mut |statement| uses.record_bindings(statement));
        uses
    }

    fn record_reads(&mut self, text: &str) {
        for token in
            text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        {
            if token
                .chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
            {
                *self.reads.entry(token.to_string()).or_default() += 1;
            }
        }
    }

    fn reads(&self, name: &str) -> usize {
        self.reads.get(name).copied().unwrap_or_default()
    }

    fn bindings(&self, name: &str) -> usize {
        self.bindings.get(name).copied().unwrap_or_default()
    }

    fn value_reads(&self, name: &str) -> usize {
        self.reads(name)
            .saturating_sub(self.assigned.get(name).copied().unwrap_or_default())
    }

    fn bind(&mut self, name: &str) {
        *self.bindings.entry(name.to_string()).or_default() += 1;
    }

    fn write_through(&mut self, target: &GoExpression) {
        if let GoExpressionNode::Identifier(name) = target.node() {
            *self.assigned.entry(name.clone()).or_default() += 1;
        }
        target.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                self.writes.insert(name.clone());
            }
        });
    }

    fn record_bindings(&mut self, statement: &LoweredStatement) {
        match statement {
            LoweredStatement::Define(definition) => {
                for name in &definition.names {
                    self.bind(name);
                }
            }
            LoweredStatement::VarDecl { name, .. } => self.bind(name),
            LoweredStatement::Const(plan) => self.bind(&plan.name),
            LoweredStatement::If(plan) => {
                if let Some(initializer) = &plan.initializer {
                    for name in &initializer.names {
                        self.bind(name);
                    }
                }
            }
            LoweredStatement::Loop(plan) => match &plan.header {
                LoopHeader::Range { key, value, .. } => {
                    for name in key.iter().chain(value) {
                        self.bind(name);
                    }
                }
                LoopHeader::Counted { variable, .. } => self.bind(variable),
                LoopHeader::Infinite | LoopHeader::While(_) => {}
            },
            LoweredStatement::Switch(plan) => {
                if let SwitchKind::Type {
                    binding: Some(name),
                    ..
                } = &plan.kind
                {
                    self.bind(name);
                }
            }
            LoweredStatement::Select(plan) => {
                for arm in &plan.arms {
                    if let SelectArmPlan::Receive {
                        receive_vars: Some(names),
                        ..
                    } = arm
                    {
                        for name in names.split(", ") {
                            self.bind(name);
                        }
                    }
                }
            }
            LoweredStatement::Assign(
                AssignForm::Simple { target, .. } | AssignForm::Compound { target, .. },
            ) => self.write_through(target),
            LoweredStatement::AssignMany { targets, .. } => {
                for target in targets {
                    self.write_through(target);
                }
            }
            LoweredStatement::BreakValue(BreakValuePlan::Transfer { action, .. }) => match action {
                BreakValueAction::UnitCallIntoResult { result_var }
                | BreakValueAction::AssignToResult { result_var } => {
                    self.writes.insert(result_var.clone());
                }
                BreakValueAction::Discard => {}
            },
            _ => {}
        }
    }
}

fn inline_name_aliases(statements: &mut Vec<LoweredStatement>) {
    let uses = NameUses::of(statements);
    for_each_statements_mut(statements, &mut |list| {
        let mut index = 0;
        while index + 1 < list.len() {
            if let Some((temp, source)) = alias_define(&list[index])
                && uses.reads(&temp) == 1
                && uses.bindings(&temp) == 1
                && !uses.writes.contains(&temp)
                && replace_only_read(&mut list[index + 1], &temp, &source)
            {
                list.remove(index);
            } else {
                index += 1;
            }
        }
    });
}

fn alias_define(statement: &LoweredStatement) -> Option<(String, String)> {
    match statement {
        LoweredStatement::Directed { inner, .. } => alias_define(inner),
        LoweredStatement::Define(Definition { names, value }) => {
            match (names.as_slice(), value.node()) {
                ([temp], GoExpressionNode::Identifier(source))
                    if go_name::is_plain_identifier(source) =>
                {
                    Some((temp.clone(), source.clone()))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn replace_only_read(statement: &mut LoweredStatement, temp: &str, source: &str) -> bool {
    match statement {
        LoweredStatement::Directed { inner, .. } => replace_only_read(inner, temp, source),
        LoweredStatement::Define(Definition { names, value }) => {
            !names.iter().any(|name| name == source) && rename_in(vec![value], None, temp, source)
        }
        LoweredStatement::Assign(AssignForm::Simple {
            target_capture,
            target,
            value,
        }) if target_capture.is_empty() && value.setup.is_empty() => {
            rename_in(vec![target, &mut value.expression], Some(0), temp, source)
        }
        LoweredStatement::Assign(AssignForm::Compound {
            target_capture,
            target,
            kind: CompoundKind::OpAssign {
                rhs, pinned_left, ..
            },
        }) if target_capture.is_empty() && rhs.setup.is_empty() => {
            let mut siblings = vec![target, &mut rhs.expression];
            siblings.extend(pinned_left.as_mut());
            rename_in(siblings, Some(0), temp, source)
        }
        LoweredStatement::Return(ReturnForm::Plain { value }) if value.setup.is_empty() => {
            rename_in(vec![&mut value.expression], None, temp, source)
        }
        LoweredStatement::Return(ReturnForm::Multi { values }) => {
            rename_in(values.iter_mut().collect(), None, temp, source)
        }
        LoweredStatement::ExpressionStatement { expression, .. } => {
            rename_in(vec![expression], None, temp, source)
        }
        LoweredStatement::If(plan)
            if plan.condition_setup.is_empty() && plan.initializer.is_none() =>
        {
            rename_in(vec![&mut plan.condition], None, temp, source)
        }
        LoweredStatement::Switch(plan) => match &mut plan.kind {
            SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                rename_in(vec![subject], None, temp, source)
            }
            SwitchKind::Conditional => false,
        },
        LoweredStatement::Select(plan) if plan.setup.is_empty() => {
            let siblings = plan
                .arms
                .iter_mut()
                .flat_map(|arm| match arm {
                    SelectArmPlan::Receive { channel, .. } => vec![channel],
                    SelectArmPlan::Send { channel, value, .. } => vec![channel, value],
                    SelectArmPlan::Default { .. } => Vec::new(),
                })
                .collect();
            rename_in(siblings, None, temp, source)
        }
        _ => false,
    }
}

fn rename_in(
    siblings: Vec<&mut GoExpression>,
    written: Option<usize>,
    temp: &str,
    source: &str,
) -> bool {
    let analyses: Vec<Analysis> = siblings
        .iter()
        .map(|sibling| analyze(sibling.node(), temp))
        .collect();
    let reader = analyses.iter().position(|analysis| analysis.reads > 0);
    let Some(reader) = reader else {
        return false;
    };
    if written == Some(reader) || analyses[reader].reads != 1 || !analyses[reader].ordered {
        return false;
    }
    let other_work = analyses
        .iter()
        .enumerate()
        .any(|(index, analysis)| index != reader && analysis.works);
    if other_work {
        return false;
    }
    let mut siblings = siblings;
    siblings[reader].rename_identifier(temp, source);
    true
}

struct Analysis {
    reads: usize,
    works: bool,
    ordered: bool,
}

fn analyze(node: &GoExpressionNode, temp: &str) -> Analysis {
    if let GoExpressionNode::Identifier(name) = node {
        return Analysis {
            reads: usize::from(name == temp),
            works: false,
            ordered: true,
        };
    }
    if let GoExpressionNode::FunctionLiteral { body, .. } = node {
        let mut reads = 0;
        body.visit_expressions(&mut |inner| {
            if let GoExpressionNode::Identifier(name) = inner
                && name == temp
            {
                reads += 1;
            }
        });
        return Analysis {
            reads,
            works: false,
            ordered: reads == 0,
        };
    }
    if let GoExpressionNode::Verbatim(_) = node {
        return Analysis {
            reads: 0,
            works: true,
            ordered: true,
        };
    }
    let identity_read = match node {
        GoExpressionNode::AddressOf(operand) | GoExpressionNode::Slice { base: operand, .. } => {
            mentions(operand, temp)
        }
        GoExpressionNode::Call { callee, .. } => {
            matches!(callee.as_ref(), GoExpressionNode::Selector { base, .. } if mentions(base, temp))
        }
        _ => false,
    };
    let mut children = Vec::new();
    node.visit_children(&mut |child| children.push(analyze(child, temp)));
    let reads = children.iter().map(|child| child.reads).sum();
    let reader = children.iter().position(|child| child.reads > 0);
    let other_work = children
        .iter()
        .enumerate()
        .any(|(index, child)| Some(index) != reader && child.works);
    let ordered = children.iter().all(|child| child.ordered)
        && !(reader.is_some() && other_work)
        && !identity_read;
    Analysis {
        reads,
        works: node.does_work() || children.iter().any(|child| child.works),
        ordered,
    }
}

fn mentions(node: &GoExpressionNode, name: &str) -> bool {
    let mut found = false;
    node.visit(&mut |inner| {
        if let GoExpressionNode::Identifier(read) = inner
            && read == name
        {
            found = true;
        }
    });
    found
}

fn drop_unread_temps(statements: &mut Vec<LoweredStatement>) {
    loop {
        let uses = NameUses::of(statements);
        let mut discards: HashMap<String, usize> = HashMap::default();
        for_each_statement(statements, &mut |statement| {
            if let Some(name) = discarded_name(statement) {
                *discards.entry(name.to_string()).or_default() += 1;
            }
        });
        let mut dropped: Option<String> = None;
        for_each_statement(statements, &mut |statement| {
            if dropped.is_some() {
                return;
            }
            let Some((name, value)) = pure_define(statement) else {
                return;
            };
            let unread = uses.value_reads(name) == discards.get(name).copied().unwrap_or_default()
                && uses.bindings(name) == 1
                && !uses.writes.contains(name);
            let mut sources = Vec::new();
            value.node().visit(&mut |node| {
                if let GoExpressionNode::Identifier(source) = node {
                    sources.push(source.clone());
                }
            });
            if unread && sources.iter().all(|source| uses.value_reads(source) > 1) {
                dropped = Some(name.to_string());
            }
        });
        let Some(name) = dropped else {
            return;
        };
        for_each_statements_mut(statements, &mut |list| {
            list.retain(|statement| {
                pure_define(statement).is_none_or(|(defined, _)| defined != name)
                    && discarded_name(statement) != Some(name.as_str())
            });
        });
        for_each_statements_mut(statements, &mut |list| {
            for statement in list {
                drop_empty_else(statement);
            }
        });
    }
}

fn drop_empty_else(statement: &mut LoweredStatement) {
    let mut plan = match statement {
        LoweredStatement::Directed { inner, .. } => return drop_empty_else(inner),
        LoweredStatement::If(plan) => plan,
        _ => return,
    };
    loop {
        if matches!(&plan.else_arm, ElseArm::Else { body, .. } if body.renders_empty()) {
            plan.else_arm = ElseArm::None;
            return;
        }
        match &mut plan.else_arm {
            ElseArm::ElseIf(inner) => plan = inner,
            ElseArm::Else { .. } | ElseArm::None => return,
        }
    }
}

fn pure_define(statement: &LoweredStatement) -> Option<(&str, &GoExpression)> {
    match statement {
        LoweredStatement::Directed { inner, .. } => pure_define(inner),
        LoweredStatement::Define(Definition { names, value }) => match names.as_slice() {
            [name] if !value.does_work() => Some((name, value)),
            _ => None,
        },
        _ => None,
    }
}

fn discarded_name(statement: &LoweredStatement) -> Option<&str> {
    match statement {
        LoweredStatement::Directed { inner, .. } => discarded_name(inner),
        LoweredStatement::Discard(expression) => match expression.node() {
            GoExpressionNode::Identifier(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}
