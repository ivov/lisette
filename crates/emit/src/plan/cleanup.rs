//! Rewrites of a lowered body that the tree makes safe to decide.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::names::go_name;
use crate::plan::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, LoopHeader, LoopTransfer, LoweredBlock,
    LoweredStatement, SelectArmPlan, SwitchKind, for_each_statement, for_each_statements_mut,
};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{GoExpression, ValuePlan};

pub(crate) fn clean_up(statements: &mut Vec<LoweredStatement>) {
    inline_name_aliases(statements);
    inline_return_aliases(statements);
    drop_unread_temps(statements);
    fold_compound_assignments(statements);
    return_found_elements_directly(statements);
    unwrap_terminal_else(statements);
}

fn return_found_elements_directly(statements: &mut Vec<LoweredStatement>) {
    for_each_statements_mut(statements, &mut |block| {
        let Some(found) = found_loop_return(block) else {
            return;
        };
        let uses = NameUses::of(block);
        if uses.reads(&found.flag) != 2 || uses.bindings(&found.flag) != 1 {
            return;
        }
        if let Some(value) = &found.value
            && (uses.reads(value) != 2 || uses.bindings(value) != 1)
        {
            return;
        }

        let mut success = found.success.clone();
        if let (Some(value), Some(element)) = (&found.value, &found.element) {
            for returned in &mut success {
                returned.rename_identifier(value, element);
            }
        }

        let LoweredStatement::Loop(plan) = &mut block[found.loop_at] else {
            unreachable!("found_loop_return matched a loop");
        };
        let Some(LoweredStatement::If(hit)) = plan.body.statements.last_mut() else {
            unreachable!("found_loop_return matched a trailing if");
        };
        hit.then_body = LoweredBlock {
            statements: vec![LoweredStatement::Return(success)],
        };

        block.truncate(found.loop_at + 1);
        block.push(LoweredStatement::Return(found.failure));
        block.drain(found.declares_at..found.loop_at);
    });
}

struct FoundLoopReturn {
    declares_at: usize,
    loop_at: usize,
    flag: String,
    value: Option<String>,
    element: Option<String>,
    success: Vec<GoExpression>,
    failure: Vec<GoExpression>,
}

fn found_loop_return(block: &[LoweredStatement]) -> Option<FoundLoopReturn> {
    let loop_at = block.len().checked_sub(3)?;
    let define_at = loop_at.checked_sub(1)?;
    let flag = false_define(&block[define_at])?;
    let (declares_at, value) = match define_at.checked_sub(1).map(|at| (at, &block[at])) {
        Some((
            at,
            LoweredStatement::VarDecl {
                name, value: None, ..
            },
        )) => (at, Some(name.clone())),
        _ => (define_at, None),
    };
    let LoweredStatement::Loop(plan) = &block[loop_at] else {
        return None;
    };
    let element = found_hit(plan.body.statements.last()?, &flag, value.as_deref())?;
    let failure = flag_guarded_return(&block[loop_at + 1], &flag)?;
    let LoweredStatement::Return(success) = &block[loop_at + 2] else {
        return None;
    };
    Some(FoundLoopReturn {
        declares_at,
        loop_at,
        flag,
        value,
        element,
        success: success.clone(),
        failure,
    })
}

fn false_define(statement: &LoweredStatement) -> Option<String> {
    let LoweredStatement::Define(Definition { names, value }) = statement else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    matches!(value.node(), GoExpressionNode::Literal(text) if text == "false").then(|| name.clone())
}

fn found_hit(
    statement: &LoweredStatement,
    flag: &str,
    value: Option<&str>,
) -> Option<Option<String>> {
    let LoweredStatement::If(plan) = statement else {
        return None;
    };
    if !matches!(plan.else_arm, ElseArm::None) || plan.initializer.is_some() {
        return None;
    }
    let body = plan.then_body.statements.as_slice();
    let (element, rest) = match (value, body) {
        (Some(value), [first, rest @ ..]) => (Some(assigned_name(first, value)?), rest),
        (None, rest) => (None, rest),
        _ => return None,
    };
    match rest {
        [raise, LoweredStatement::Break(LoopTransfer::Unlabeled)] if assigns_true(raise, flag) => {
            Some(element)
        }
        _ => None,
    }
}

fn assigned_name(statement: &LoweredStatement, target: &str) -> Option<String> {
    let LoweredStatement::Assign(AssignForm::Simple {
        target_capture,
        target: place,
        value,
    }) = statement
    else {
        return None;
    };
    if !target_capture.is_empty() || !value.setup.is_empty() {
        return None;
    }
    if !matches!(place.node(), GoExpressionNode::Identifier(name) if name == target) {
        return None;
    }
    match value.expression.node() {
        GoExpressionNode::Identifier(element) => Some(element.clone()),
        _ => None,
    }
}

fn assigns_true(statement: &LoweredStatement, target: &str) -> bool {
    let LoweredStatement::Assign(AssignForm::Simple {
        target_capture,
        target: place,
        value,
    }) = statement
    else {
        return false;
    };
    target_capture.is_empty()
        && value.setup.is_empty()
        && matches!(place.node(), GoExpressionNode::Identifier(name) if name == target)
        && matches!(value.expression.node(), GoExpressionNode::Literal(text) if text == "true")
}

fn flag_guarded_return(statement: &LoweredStatement, flag: &str) -> Option<Vec<GoExpression>> {
    let LoweredStatement::If(plan) = statement else {
        return None;
    };
    if !matches!(plan.else_arm, ElseArm::None) || plan.initializer.is_some() {
        return None;
    }
    let GoExpressionNode::Unary { operator, operand } = plan.condition.node() else {
        return None;
    };
    if operator != "!"
        || !matches!(operand.as_ref(), GoExpressionNode::Identifier(name) if name == flag)
    {
        return None;
    }
    match plan.then_body.statements.as_slice() {
        [LoweredStatement::Return(values)] => Some(values.clone()),
        _ => None,
    }
}

fn unwrap_terminal_else(statements: &mut Vec<LoweredStatement>) {
    for_each_statements_mut(statements, &mut |block| {
        for statement in block.iter_mut() {
            unwrap_terminal_else_of(statement);
        }
    });
}

fn unwrap_terminal_else_of(statement: &mut LoweredStatement) {
    let mut plan = match statement {
        LoweredStatement::Directed { inner, .. } => return unwrap_terminal_else_of(inner),
        LoweredStatement::If(plan) => plan,
        _ => return,
    };
    loop {
        let then_diverges = plan.then_body.ends_with_diverge();
        // An initializer binds its names for the whole statement, which an inlined body leaves.
        let keeps_scope = plan.initializer.is_none();
        match &mut plan.else_arm {
            ElseArm::ElseIf(inner) => plan = inner,
            ElseArm::Else { body, inline } => {
                if then_diverges && keeps_scope && declares_no_names(body) {
                    *inline = true;
                }
                return;
            }
            ElseArm::None => return,
        }
    }
}

fn declares_no_names(body: &LoweredBlock) -> bool {
    body.statements.iter().all(|statement| {
        let statement = match statement {
            LoweredStatement::Directed { inner, .. } => inner.as_ref(),
            other => other,
        };
        !matches!(
            statement,
            LoweredStatement::Define(_)
                | LoweredStatement::VarDecl { .. }
                | LoweredStatement::Const(_)
                | LoweredStatement::Select(_)
                | LoweredStatement::Switch(_)
                | LoweredStatement::Loop(_)
                | LoweredStatement::If(_)
        )
    })
}

fn inline_return_aliases(statements: &mut Vec<LoweredStatement>) {
    let uses = NameUses::of(statements);
    for_each_statements_mut(statements, &mut |list| {
        let mut index = 0;
        while index + 1 < list.len() {
            let Some(name) = returned_alias(&list[index], &list[index + 1]) else {
                index += 1;
                continue;
            };
            if uses.reads(&name) != 1 || uses.bindings(&name) != 1 || uses.writes.contains(&name) {
                index += 1;
                continue;
            }
            let LoweredStatement::Define(Definition { value, .. }) = list.remove(index) else {
                unreachable!("returned_alias matched a definition");
            };
            let LoweredStatement::Return(values) = &mut list[index] else {
                unreachable!("returned_alias matched a return");
            };
            values[0] = value;
            index += 1;
        }
    });
}

fn returned_alias(definition: &LoweredStatement, next: &LoweredStatement) -> Option<String> {
    let LoweredStatement::Define(Definition { names, value }) = definition else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    if value.does_work() {
        return None;
    }
    let LoweredStatement::Return(values) = next else {
        return None;
    };
    let [returned] = values.as_slice() else {
        return None;
    };
    matches!(returned.node(), GoExpressionNode::Identifier(read) if read == name)
        .then(|| name.clone())
}

fn fold_compound_assignments(statements: &mut Vec<LoweredStatement>) {
    for_each_statements_mut(statements, &mut |block| {
        for statement in block.iter_mut() {
            if let Some(folded) = folded_compound_assignment(statement) {
                *statement = folded;
            }
        }
    });
}

fn folded_compound_assignment(statement: &LoweredStatement) -> Option<LoweredStatement> {
    let LoweredStatement::Assign(AssignForm::Simple {
        target_capture,
        target,
        value,
    }) = statement
    else {
        return None;
    };
    if !target_capture.is_empty() || !value.setup.is_empty() {
        return None;
    }
    let GoExpressionNode::Identifier(name) = target.node() else {
        return None;
    };
    let GoExpressionNode::Binary {
        operator,
        left,
        right,
    } = value.expression.node()
    else {
        return None;
    };
    if !matches!(left.as_ref(), GoExpressionNode::Identifier(read) if read == name)
        || !is_compound_operator(operator)
    {
        return None;
    }

    let kind = match (operator.as_str(), right.as_ref()) {
        ("+", GoExpressionNode::Literal(one)) if one == "1" => CompoundKind::Increment,
        ("-", GoExpressionNode::Literal(one)) if one == "1" => CompoundKind::Decrement,
        _ => CompoundKind::OpAssign {
            op_text: operator.clone(),
            rhs: Box::new(ValuePlan::computed(
                Vec::new(),
                GoExpression::from_node(right.as_ref().clone()),
                value.evaluation.effect,
            )),
            pinned_left: None,
        },
    };
    Some(LoweredStatement::Assign(AssignForm::Compound {
        target_capture: Vec::new(),
        target: target.clone(),
        kind,
    }))
}

fn is_compound_operator(operator: &str) -> bool {
    matches!(
        operator,
        "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^" | "&^" | "<<" | ">>"
    )
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
                    if let SelectArmPlan::Receive { receive_vars, .. } = arm {
                        for name in receive_vars {
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
        LoweredStatement::Return(values) => {
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
