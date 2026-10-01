//! Rewrites of a lowered body that the tree makes safe to decide.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::names::go_name;
use crate::plan::bodies::{
    AssignForm, CompoundKind, Definition, ElseArm, LoopHeader, LoopTransfer, LoweredBlock,
    LoweredStatement, SelectArmPlan, SwitchKind, for_each_statement, for_each_statements_mut,
    legalize_else_if_scopes,
};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::values::{GoExpression, ValuePlan};

pub(crate) fn clean_up(statements: &mut Vec<LoweredStatement>, shadowing: &HashSet<LocalId>) {
    // Later deletions can expose earlier rules, so another sweep changes emitted Go.
    inline_name_aliases(statements);
    inline_return_aliases(statements);
    drop_unread_temps(statements, shadowing);
    fold_compound_assignments(statements);
    return_found_elements_directly(statements);
    // Declaration removal and return rewriting can make an else safe to inline.
    unwrap_terminal_else(statements);
    legalize_else_if_scopes(statements);
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
                replace_identifier(returned.node_mut(), value, element);
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
    flag: GoIdentifier,
    value: Option<GoIdentifier>,
    element: Option<GoIdentifier>,
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
    let element = found_hit(plan.body.statements.last()?, &flag, value.as_ref())?;
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

fn false_define(statement: &LoweredStatement) -> Option<GoIdentifier> {
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
    flag: &GoIdentifier,
    value: Option<&GoIdentifier>,
) -> Option<Option<GoIdentifier>> {
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

fn assigned_name(statement: &LoweredStatement, target: &GoIdentifier) -> Option<GoIdentifier> {
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
    if !matches!(place.node(), GoExpressionNode::Identifier(name) if name.refers_to_same(target)) {
        return None;
    }
    match value.expression.node() {
        GoExpressionNode::Identifier(element) => Some(element.clone()),
        _ => None,
    }
}

fn assigns_true(statement: &LoweredStatement, target: &GoIdentifier) -> bool {
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
        && matches!(place.node(), GoExpressionNode::Identifier(name) if name.refers_to_same(target))
        && matches!(value.expression.node(), GoExpressionNode::Literal(text) if text == "true")
}

fn flag_guarded_return(
    statement: &LoweredStatement,
    flag: &GoIdentifier,
) -> Option<Vec<GoExpression>> {
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
        || !matches!(operand.as_ref(), GoExpressionNode::Identifier(name) if name.refers_to_same(flag))
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
    let mut then_diverges = true;
    // An initializer binds its names for the whole statement, which an inlined body leaves.
    let mut keeps_scope = true;
    loop {
        then_diverges &= plan.then_body.ends_with_diverge();
        keeps_scope &= plan.initializer.is_none();
        match &mut plan.else_arm {
            ElseArm::ElseIf(inner) => {
                if !inner.condition_setup.is_empty() {
                    then_diverges = true;
                }
                plan = inner;
            }
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
            if uses.reads(&name) != 1 || uses.bindings(&name) != 1 || uses.is_written(&name) {
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

fn returned_alias(definition: &LoweredStatement, next: &LoweredStatement) -> Option<GoIdentifier> {
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
    matches!(returned.node(), GoExpressionNode::Identifier(read) if read.refers_to_same(name))
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
        ..
    } = value.expression.node()
    else {
        return None;
    };
    if !matches!(left.as_ref(), GoExpressionNode::Identifier(read) if read.refers_to_same(name))
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
    reads: LocalCounts,
    opaque_reads: HashMap<String, usize>,
    assigned: LocalCounts,
    writes: LocalCounts,
    bindings: LocalCounts,
}

#[derive(Default)]
struct LocalCounts {
    by_id: HashMap<LocalId, usize>,
    by_spelling: HashMap<String, usize>,
}

impl LocalCounts {
    fn add(&mut self, name: &GoIdentifier) {
        *self
            .by_spelling
            .entry(name.spelling().to_string())
            .or_default() += 1;
        if let Some(id) = name.id() {
            *self.by_id.entry(id).or_default() += 1;
        }
    }

    fn get(&self, name: &GoIdentifier) -> usize {
        match name.id() {
            Some(id) => self.by_id.get(&id).copied().unwrap_or_default(),
            None => self
                .by_spelling
                .get(name.spelling())
                .copied()
                .unwrap_or_default(),
        }
    }
}

impl NameUses {
    fn of(statements: &[LoweredStatement]) -> Self {
        let mut uses = Self::default();
        for statement in statements {
            statement.visit_expressions(&mut |node| match node {
                GoExpressionNode::Identifier(name) => {
                    uses.reads.add(name);
                }
                GoExpressionNode::Verbatim(text) => uses.record_opaque_reads(text),
                _ => {}
            });
        }
        for_each_statement(statements, &mut |statement| uses.record_bindings(statement));
        uses
    }

    fn record_opaque_reads(&mut self, text: &str) {
        for token in
            text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        {
            if token
                .chars()
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_')
            {
                *self.opaque_reads.entry(token.to_string()).or_default() += 1;
            }
        }
    }

    fn reads(&self, name: &GoIdentifier) -> usize {
        self.reads.get(name)
            + self
                .opaque_reads
                .get(name.spelling())
                .copied()
                .unwrap_or_default()
    }

    fn bindings(&self, name: &GoIdentifier) -> usize {
        self.bindings.get(name)
    }

    fn value_reads(&self, name: &GoIdentifier) -> usize {
        self.reads(name).saturating_sub(self.assigned.get(name))
    }

    fn is_written(&self, name: &GoIdentifier) -> bool {
        self.writes.get(name) > 0
    }

    fn bind(&mut self, name: &GoIdentifier) {
        self.bindings.add(name);
    }

    fn write_through(&mut self, target: &GoExpression) {
        if let GoExpressionNode::Identifier(name) = target.node() {
            self.assigned.add(name);
        }
        target.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                self.writes.add(name);
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
                && !uses.is_written(&temp)
                && replace_only_read(&mut list[index + 1], &temp, &source)
            {
                list.remove(index);
            } else {
                index += 1;
            }
        }
    });
}

fn alias_define(statement: &LoweredStatement) -> Option<(GoIdentifier, GoIdentifier)> {
    match statement {
        LoweredStatement::Directed { inner, .. } => alias_define(inner),
        LoweredStatement::Define(Definition { names, value }) => {
            match (names.as_slice(), value.node()) {
                ([temp], GoExpressionNode::Identifier(source))
                    if go_name::is_plain_identifier(source) && !temp.refers_to_same(source) =>
                {
                    Some((temp.clone(), source.clone()))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn replace_only_read(
    statement: &mut LoweredStatement,
    temp: &GoIdentifier,
    source: &GoIdentifier,
) -> bool {
    match statement {
        LoweredStatement::Directed { inner, .. } => replace_only_read(inner, temp, source),
        LoweredStatement::Define(Definition { names, value }) => {
            !names.iter().any(|name| name == source.spelling())
                && rename_in(vec![value.node_mut()], None, temp, source)
        }
        LoweredStatement::Assign(AssignForm::Simple {
            target_capture,
            target,
            value,
        }) if target_capture.is_empty() && value.setup.is_empty() => rename_in(
            vec![target.node_mut(), value.expression.node_mut()],
            Some(0),
            temp,
            source,
        ),
        LoweredStatement::Assign(AssignForm::Compound {
            target_capture,
            target,
            kind: CompoundKind::OpAssign {
                rhs, pinned_left, ..
            },
        }) if target_capture.is_empty() && rhs.setup.is_empty() => {
            let mut siblings = vec![target.node_mut(), rhs.expression.node_mut()];
            siblings.extend(pinned_left.as_mut().map(GoExpression::node_mut));
            rename_in(siblings, Some(0), temp, source)
        }
        LoweredStatement::Return(values) => rename_in(
            values.iter_mut().map(GoExpression::node_mut).collect(),
            None,
            temp,
            source,
        ),
        LoweredStatement::ExpressionStatement { expression, .. } => {
            rename_in(vec![expression.node_mut()], None, temp, source)
        }
        LoweredStatement::If(plan)
            if plan.condition_setup.is_empty() && plan.initializer.is_none() =>
        {
            rename_in(vec![plan.condition.node_mut()], None, temp, source)
        }
        LoweredStatement::Switch(plan) => match &mut plan.kind {
            SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                rename_in(vec![subject.node_mut()], None, temp, source)
            }
            SwitchKind::Conditional => false,
        },
        LoweredStatement::Select(plan) => {
            let siblings = plan
                .arms
                .iter_mut()
                .flat_map(|arm| match arm {
                    SelectArmPlan::Receive { channel, .. } => vec![channel.node_mut()],
                    SelectArmPlan::Send { channel, value, .. } => {
                        vec![channel.node_mut(), value.node_mut()]
                    }
                    SelectArmPlan::Default { .. } => Vec::new(),
                })
                .collect();
            rename_in(siblings, None, temp, source)
        }
        _ => false,
    }
}

fn rename_in(
    siblings: Vec<&mut GoExpressionNode>,
    written: Option<usize>,
    temp: &GoIdentifier,
    source: &GoIdentifier,
) -> bool {
    let analyses: Vec<Analysis> = siblings
        .iter()
        .map(|sibling| analyze(sibling, temp))
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
    replace_identifier(siblings[reader], temp, source);
    true
}

fn replace_identifier(node: &mut GoExpressionNode, from: &GoIdentifier, to: &GoIdentifier) {
    match node {
        GoExpressionNode::Identifier(name) if name.refers_to_same(from) => *name = to.clone(),
        _ => node.visit_children_mut(&mut |child| replace_identifier(child, from, to)),
    }
}

fn mentions_local(node: &GoExpressionNode, local: &GoIdentifier) -> bool {
    let mut found = false;
    node.visit(&mut |child| {
        if let GoExpressionNode::Identifier(name) = child {
            found |= name.refers_to_same(local);
        }
    });
    found
}

struct Analysis {
    reads: usize,
    works: bool,
    ordered: bool,
}

fn analyze(node: &GoExpressionNode, temp: &GoIdentifier) -> Analysis {
    if let GoExpressionNode::Identifier(name) = node {
        return Analysis {
            reads: usize::from(name.refers_to_same(temp)),
            works: false,
            ordered: true,
        };
    }
    if let GoExpressionNode::FunctionLiteral { body, .. } = node {
        let mut reads = 0;
        body.visit_expressions(&mut |inner| {
            if let GoExpressionNode::Identifier(name) = inner
                && name.refers_to_same(temp)
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
            mentions_local(operand, temp)
        }
        GoExpressionNode::Call { callee, .. } => {
            matches!(callee.as_ref(), GoExpressionNode::Selector { base, .. } if mentions_local(base, temp))
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
        works: node.does_work()
            || node.requires_ordering_without_call()
            || children.iter().any(|child| child.works),
        ordered,
    }
}

fn drop_unread_temps(statements: &mut Vec<LoweredStatement>, shadowing: &HashSet<LocalId>) {
    loop {
        let uses = NameUses::of(statements);
        let mut discards: HashMap<LocalId, usize> = HashMap::default();
        let mut untracked_discards: HashMap<String, usize> = HashMap::default();
        for_each_statement(statements, &mut |statement| {
            if let Some(name) = discarded_name(statement) {
                if let Some(id) = name.id() {
                    *discards.entry(id).or_default() += 1;
                } else {
                    *untracked_discards
                        .entry(name.spelling().to_string())
                        .or_default() += 1;
                }
            }
        });
        let mut dropped: Option<GoIdentifier> = None;
        for_each_statement(statements, &mut |statement| {
            if dropped.is_some() {
                return;
            }
            let Some((name, value)) = pure_define(statement) else {
                return;
            };
            if name.id().is_some_and(|id| shadowing.contains(&id)) {
                return;
            }
            let discard_count = name
                .id()
                .and_then(|id| discards.get(&id))
                .copied()
                .unwrap_or_default()
                + untracked_discards
                    .get(name.spelling())
                    .copied()
                    .unwrap_or_default();
            let unread = uses.value_reads(name) == discard_count
                && uses.bindings(name) == 1
                && !uses.is_written(name);
            let mut sources = Vec::new();
            value.node().visit(&mut |node| {
                if let GoExpressionNode::Identifier(source) = node {
                    sources.push(source.clone());
                }
            });
            if unread && sources.iter().all(|source| uses.value_reads(source) > 1) {
                dropped = Some(name.clone());
            }
        });
        let Some(name) = dropped else {
            return;
        };
        for_each_statements_mut(statements, &mut |list| {
            list.retain(|statement| {
                pure_define(statement).is_none_or(|(defined, _)| !defined.refers_to_same(&name))
                    && discarded_name(statement)
                        .is_none_or(|discarded| !discarded.refers_to_same(&name))
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

fn pure_define(statement: &LoweredStatement) -> Option<(&GoIdentifier, &GoExpression)> {
    match statement {
        LoweredStatement::Directed { inner, .. } => pure_define(inner),
        LoweredStatement::Define(Definition { names, value }) => match names.as_slice() {
            [name] if value.can_erase() => Some((name, value)),
            _ => None,
        },
        _ => None,
    }
}

fn discarded_name(statement: &LoweredStatement) -> Option<&GoIdentifier> {
    match statement {
        LoweredStatement::Directed { inner, .. } => discarded_name(inner),
        LoweredStatement::Discard(expression) => match expression.node() {
            GoExpressionNode::Identifier(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::bodies::{IfPlan, LoopKind, LoopPlan, assign, define, discard};
    use crate::plan::local::LocalId;

    fn name(text: &str) -> GoExpression {
        GoExpression::name(text.to_string())
    }

    fn literal(text: &str) -> GoExpression {
        GoExpression::literal(text.to_string())
    }

    fn returned(value: GoExpression) -> LoweredStatement {
        LoweredStatement::Return(vec![value])
    }

    fn returning_if(
        initializer: Option<Definition>,
        else_body: Vec<LoweredStatement>,
    ) -> LoweredStatement {
        LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            initializer,
            condition: name("ok"),
            then_body: LoweredBlock {
                statements: vec![returned(literal("0"))],
            },
            else_arm: ElseArm::Else {
                body: LoweredBlock {
                    statements: else_body,
                },
                inline: false,
            },
        })
    }

    #[test]
    fn terminal_else_keeps_initializer_bindings_in_scope() {
        let mut statements = vec![returning_if(
            Some(Definition::single("value".to_string(), name("source"))),
            vec![returned(name("value"))],
        )];
        let original = statements.clone();
        clean_up(&mut statements, &HashSet::default());
        clean_up(&mut statements, &HashSet::default());
        assert_eq!(statements, original);
    }

    #[test]
    fn return_alias_removal_exposes_a_terminal_else() {
        let mut statements = vec![returning_if(
            None,
            vec![
                define("value".to_string(), literal("1")),
                returned(name("value")),
            ],
        )];
        clean_up(&mut statements, &HashSet::default());
        let LoweredStatement::If(plan) = &statements[0] else {
            panic!("expected if");
        };
        assert_eq!(
            plan.else_arm,
            ElseArm::Else {
                body: LoweredBlock {
                    statements: vec![returned(literal("1"))]
                },
                inline: true,
            }
        );
        let once = statements.clone();
        clean_up(&mut statements, &HashSet::default());
        assert_eq!(statements, once);
    }

    #[test]
    fn name_alias_pass_finishes_an_adjacent_chain() {
        let mut statements = vec![
            define("first".to_string(), name("source")),
            define("second".to_string(), name("first")),
            returned(name("second")),
        ];
        inline_name_aliases(&mut statements);
        assert_eq!(statements, vec![returned(name("source"))]);
        inline_name_aliases(&mut statements);
        assert_eq!(statements, vec![returned(name("source"))]);
    }

    #[test]
    fn alias_replacement_keeps_the_sources_local_id() {
        let source = GoIdentifier::local("source".to_string(), LocalId(7));
        let mut statements = vec![
            define(
                "temp".to_string(),
                GoExpression::from_node(GoExpressionNode::Identifier(source.clone())),
            ),
            returned(name("temp")),
        ];
        inline_name_aliases(&mut statements);
        let [LoweredStatement::Return(values)] = statements.as_slice() else {
            panic!("expected a direct return");
        };
        let GoExpressionNode::Identifier(actual) = values[0].node() else {
            panic!("expected a local reference");
        };
        assert_eq!(actual.id(), source.id());
    }

    #[test]
    fn alias_replacement_does_not_match_a_shadowing_local() {
        let temp = GoIdentifier::local("temp".to_string(), LocalId(3));
        let source = GoIdentifier::local("source".to_string(), LocalId(4));
        let mut other =
            GoExpressionNode::Identifier(GoIdentifier::local("temp".to_string(), LocalId(5)));
        assert!(!rename_in(vec![&mut other], None, &temp, &source));
        assert_eq!(
            other,
            GoExpressionNode::Identifier(GoIdentifier::local("temp".to_string(), LocalId(5)))
        );
    }

    #[test]
    fn unread_temp_pass_preserves_the_sources_last_read() {
        let mut statements = vec![
            define("first".to_string(), literal("1")),
            define("second".to_string(), name("first")),
            discard(name("second")),
        ];
        let original = statements.clone();
        drop_unread_temps(&mut statements, &HashSet::default());
        assert_eq!(statements, original);

        statements.push(returned(name("first")));
        drop_unread_temps(&mut statements, &HashSet::default());
        let expected = vec![
            define("first".to_string(), literal("1")),
            returned(name("first")),
        ];
        assert_eq!(statements, expected);
        drop_unread_temps(&mut statements, &HashSet::default());
        assert_eq!(statements, expected);
    }

    #[test]
    fn return_alias_pass_is_not_idempotent() {
        let mut statements = vec![
            define("first".to_string(), literal("1")),
            define("second".to_string(), name("first")),
            returned(name("second")),
        ];
        inline_return_aliases(&mut statements);
        assert_eq!(
            statements,
            vec![
                define("first".to_string(), literal("1")),
                returned(name("first"))
            ]
        );
        inline_return_aliases(&mut statements);
        assert_eq!(statements, vec![returned(literal("1"))]);
    }

    #[test]
    fn cleanup_is_one_sweep_rather_than_a_fixed_point() {
        let mut statements = vec![
            define("first".to_string(), name("source")),
            define("second".to_string(), name("first")),
            discard(name("second")),
            returned(name("first")),
        ];
        clean_up(&mut statements, &HashSet::default());
        assert_eq!(
            statements,
            vec![
                define("first".to_string(), name("source")),
                returned(name("first"))
            ]
        );
        clean_up(&mut statements, &HashSet::default());
        assert_eq!(statements, vec![returned(name("source"))]);
    }

    #[test]
    fn compound_folding_keeps_its_result_on_a_second_pass() {
        let mut statements = vec![assign(
            name("total"),
            GoExpression::binary(name("total"), "+", literal("1")),
        )];
        fold_compound_assignments(&mut statements);
        assert_eq!(
            statements,
            vec![LoweredStatement::Assign(AssignForm::Compound {
                target_capture: Vec::new(),
                target: name("total"),
                kind: CompoundKind::Increment,
            })]
        );
        let once = statements.clone();
        fold_compound_assignments(&mut statements);
        assert_eq!(statements, once);
    }

    #[test]
    fn found_loop_return_keeps_its_result_on_a_second_pass() {
        let hit = LoweredStatement::If(IfPlan::plain(
            GoExpression::binary(name("element"), ">", literal("0")),
            LoweredBlock {
                statements: vec![
                    assign(
                        name("value"),
                        GoExpression::identifier(GoIdentifier::local(
                            "element".to_string(),
                            LocalId(17),
                        )),
                    ),
                    assign(name("found"), literal("true")),
                    LoweredStatement::Break(LoopTransfer::Unlabeled),
                ],
            },
            ElseArm::None,
        ));
        let mut statements = vec![
            LoweredStatement::VarDecl {
                name: "value".to_string().into(),
                go_type: "int".to_string(),
                value: None,
            },
            define("found".to_string(), literal("false")),
            LoweredStatement::Loop(LoopPlan {
                prologue: Vec::new(),
                kind: LoopKind::Generated { label: None },
                header: LoopHeader::Range {
                    key: None,
                    value: Some("element".to_string().into()),
                    iterable: name("items"),
                },
                body: LoweredBlock {
                    statements: vec![hit],
                },
            }),
            LoweredStatement::If(IfPlan::plain(
                GoExpression::unary("!", name("found")),
                LoweredBlock {
                    statements: vec![returned(literal("0"))],
                },
                ElseArm::None,
            )),
            returned(name("value")),
        ];
        return_found_elements_directly(&mut statements);
        let [LoweredStatement::Loop(plan), failure] = statements.as_slice() else {
            panic!("expected loop and failure return");
        };
        assert_eq!(*failure, returned(literal("0")));
        let [LoweredStatement::If(hit)] = plan.body.statements.as_slice() else {
            panic!("expected hit test");
        };
        assert_eq!(hit.then_body.statements.len(), 1);
        let [LoweredStatement::Return(values)] = hit.then_body.statements.as_slice() else {
            panic!("expected return");
        };
        assert!(
            matches!(values[0].node(), GoExpressionNode::Identifier(name) if name.id() == Some(LocalId(17)))
        );
        let once = statements.clone();
        return_found_elements_directly(&mut statements);
        assert_eq!(statements, once);
    }
}
