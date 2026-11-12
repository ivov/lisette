//! Let a copy `y := x` write to `x` itself when nothing reads `x` afterwards.

use std::mem;

use crate::plan::bodies::{
    AssignForm, CompoundKind, ElseArm, IfPlan, LoopHeader, LoweredBlock, LoweredStatement,
    SelectArmKind, Statement, SwitchKind,
};
use crate::plan::go_expression::{GoExpressionNode, TreeVisitor, verbatim_identifiers};
use crate::plan::local::GoIdentifier;
use crate::plan::visit::{VisitorMut, visit_statements_mut};
use crate::state::scope::ScopeState;

pub(crate) fn reuse_dead_copy_sources(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
    scope: &ScopeState,
) {
    reuse_in_list(statements, parameters, scope);
}

fn reuse_in_list(list: &mut Vec<Statement>, parameters: &[GoIdentifier], scope: &ScopeState) {
    if has_copy_candidate(list) {
        let mut flat = flattened(list.clone());
        let mut changed = false;
        let mut index = 0;
        while index < flat.len() {
            if reuse_copy_source(&mut flat, index, parameters, scope) {
                changed = true;
            } else {
                index += 1;
            }
        }
        if changed {
            *list = flat;
        }
    }
    for statement in list.iter_mut() {
        reuse_in_statement(&mut statement.kind, scope);
    }
}

fn reuse_in_statement(statement: &mut LoweredStatement, scope: &ScopeState) {
    match statement {
        LoweredStatement::If(plan) => reuse_in_if(plan, &[], scope),
        LoweredStatement::Loop(plan) => {
            // Range variables are fresh in each iteration, counted variables are not.
            let mut per_iteration: Vec<GoIdentifier> = Vec::new();
            match &mut plan.header {
                LoopHeader::Infinite => {}
                LoopHeader::While(condition) => reuse_in_expression(condition.node_mut(), scope),
                LoopHeader::Range {
                    key,
                    value,
                    iterable,
                } => {
                    reuse_in_expression(iterable.node_mut(), scope);
                    per_iteration.extend(key.iter().chain(value.iter()).cloned());
                }
                LoopHeader::Counted {
                    start, condition, ..
                } => {
                    reuse_in_expression(start.node_mut(), scope);
                    if let Some(condition) = condition {
                        reuse_in_expression(condition.node_mut(), scope);
                    }
                }
            }
            reuse_in_list(&mut plan.body.statements, &per_iteration, scope);
        }
        LoweredStatement::Block(body) => reuse_in_list(&mut body.statements, &[], scope),
        LoweredStatement::Body(body) => {
            for statement in &mut body.statements {
                reuse_in_statement(&mut statement.kind, scope);
            }
        }
        LoweredStatement::Break(_)
        | LoweredStatement::Continue(_)
        | LoweredStatement::UnreachablePanic => {}
        LoweredStatement::Const(plan) => reuse_in_expression(plan.value.node_mut(), scope),
        LoweredStatement::Return(values) => {
            for value in values {
                reuse_in_expression(value.node_mut(), scope);
            }
        }
        LoweredStatement::Assign(AssignForm::Compound { target, kind }) => {
            reuse_in_expression(target.node_mut(), scope);
            if let CompoundKind::OpAssign { rhs, .. } = kind {
                reuse_in_expression(rhs.node_mut(), scope);
            }
        }
        LoweredStatement::Assign(AssignForm::Simple { target, value }) => {
            reuse_in_expression(target.node_mut(), scope);
            reuse_in_expression(value.node_mut(), scope);
        }
        LoweredStatement::Async { call, .. } => reuse_in_expression(call.node_mut(), scope),
        LoweredStatement::Select(plan) => {
            for arm in &mut plan.arms {
                let body = match &mut arm.kind {
                    SelectArmKind::Receive { channel, body, .. } => {
                        reuse_in_expression(channel.node_mut(), scope);
                        body
                    }
                    SelectArmKind::Send {
                        channel,
                        value,
                        body,
                    } => {
                        reuse_in_expression(channel.node_mut(), scope);
                        reuse_in_expression(value.node_mut(), scope);
                        body
                    }
                    SelectArmKind::Default { body } => body,
                };
                reuse_in_list(&mut body.statements, &[], scope);
            }
        }
        LoweredStatement::Switch(plan) => {
            match &mut plan.kind {
                SwitchKind::Conditional => {}
                SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => {
                    reuse_in_expression(subject.node_mut(), scope);
                }
            }
            for case in &mut plan.cases {
                for label in &mut case.labels {
                    reuse_in_expression(label.node_mut(), scope);
                }
                reuse_in_list(&mut case.body.statements, &[], scope);
            }
            if let Some(default) = &mut plan.default {
                reuse_in_list(&mut default.statements, &[], scope);
            }
        }
        LoweredStatement::Define(definition) => {
            reuse_in_expression(definition.value.node_mut(), scope)
        }
        LoweredStatement::VarDecl { value, .. } => {
            if let Some(value) = value {
                reuse_in_expression(value.node_mut(), scope);
            }
        }
        LoweredStatement::Discard {
            value: expression, ..
        }
        | LoweredStatement::ExpressionStatement { expression, .. } => {
            reuse_in_expression(expression.node_mut(), scope);
        }
    }
}

fn reuse_in_if(plan: &mut IfPlan, declared: &[GoIdentifier], scope: &ScopeState) {
    let mut declared = declared.to_vec();
    if let Some(initializer) = &mut plan.initializer {
        reuse_in_expression(initializer.value.node_mut(), scope);
        declared.extend(initializer.names.iter().cloned());
    }
    reuse_in_expression(plan.condition.node_mut(), scope);
    reuse_in_list(&mut plan.then_body.statements, &declared, scope);
    match &mut plan.else_arm {
        ElseArm::None => {}
        ElseArm::ElseIf(plan) => reuse_in_if(plan, &declared, scope),
        ElseArm::Else {
            body,
            inline: false,
        } => reuse_in_list(&mut body.statements, &declared, scope),
        // An inline else shares the enclosing list's scope.
        ElseArm::Else { body, inline: true } => {
            for statement in &mut body.statements {
                reuse_in_statement(&mut statement.kind, scope);
            }
        }
    }
}

fn reuse_in_expression(node: &mut GoExpressionNode, scope: &ScopeState) {
    if let GoExpressionNode::FunctionLiteral {
        parameters, body, ..
    } = node
    {
        let names: Vec<GoIdentifier> = parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect();
        reuse_in_list(&mut body.statements, &names, scope);
    } else {
        node.visit_children_mut(&mut |child| reuse_in_expression(child, scope));
    }
}

fn has_copy_candidate(list: &[Statement]) -> bool {
    list.iter().any(|statement| match &statement.kind {
        LoweredStatement::Define(_) => copy_define(&statement.kind).is_some(),
        LoweredStatement::Body(body) => has_copy_candidate(&body.statements),
        LoweredStatement::If(plan) => {
            inline_else(plan).is_some_and(|body| has_copy_candidate(&body.statements))
        }
        _ => false,
    })
}

fn inline_else(plan: &IfPlan) -> Option<&LoweredBlock> {
    match &plan.else_arm {
        ElseArm::ElseIf(inner) => inline_else(inner),
        ElseArm::Else { body, inline: true } => Some(body),
        ElseArm::Else { inline: false, .. } | ElseArm::None => None,
    }
}

fn take_inline_else(plan: &mut IfPlan) -> Option<LoweredBlock> {
    match &mut plan.else_arm {
        ElseArm::ElseIf(inner) => take_inline_else(inner),
        ElseArm::Else { inline: true, .. } => {
            match mem::replace(&mut plan.else_arm, ElseArm::None) {
                ElseArm::Else { body, .. } => Some(body),
                _ => unreachable!("the arm was just matched as an inline else"),
            }
        }
        ElseArm::Else { inline: false, .. } | ElseArm::None => None,
    }
}

fn flattened(list: Vec<Statement>) -> Vec<Statement> {
    let mut flat = Vec::with_capacity(list.len());
    for statement in list {
        match statement.kind {
            LoweredStatement::Body(body) if !body.statements.is_empty() => {
                let mut inner = flattened(body.statements);
                if inner[0].line.is_none() {
                    inner[0].line = statement.line;
                }
                flat.extend(inner);
            }
            LoweredStatement::If(mut plan) => {
                let after = take_inline_else(&mut plan);
                flat.push(Statement {
                    line: statement.line,
                    kind: LoweredStatement::If(plan),
                });
                if let Some(body) = after {
                    flat.extend(flattened(body.statements));
                }
            }
            kind => flat.push(Statement {
                line: statement.line,
                kind,
            }),
        }
    }
    flat
}

fn copy_define(statement: &LoweredStatement) -> Option<(GoIdentifier, GoIdentifier)> {
    let LoweredStatement::Define(definition) = statement else {
        return None;
    };
    let ([copy], GoExpressionNode::Identifier(source)) =
        (definition.names.as_slice(), definition.value.node())
    else {
        return None;
    };
    (copy.id().is_some() && source.id().is_some() && !copy.refers_to_same(source))
        .then(|| (copy.clone(), source.clone()))
}

fn copy_keeps_no_name_of_its_own(
    copy: &GoIdentifier,
    source: &GoIdentifier,
    scope: &ScopeState,
) -> bool {
    let base = |name: &GoIdentifier| -> String {
        name.id()
            .and_then(|id| scope.generated_base(id))
            .unwrap_or(name.spelling())
            .to_string()
    };
    let Some(copy_id) = copy.id() else {
        return false;
    };
    if scope.is_struct_update_copy(copy_id) {
        return true;
    }
    scope
        .generated_base(copy_id)
        .is_some_and(|copy_base| copy_base == base(source))
}

fn declares(statement: &LoweredStatement, name: &GoIdentifier) -> bool {
    match statement {
        LoweredStatement::Define(definition) => definition
            .names
            .iter()
            .any(|declared| declared.refers_to_same(name)),
        LoweredStatement::VarDecl { name: declared, .. } => declared.refers_to_same(name),
        _ => false,
    }
}

fn copy_field_write<'s>(
    statement: &'s LoweredStatement,
    copy: &GoIdentifier,
) -> Option<(&'s str, &'s GoExpressionNode)> {
    let LoweredStatement::Assign(AssignForm::Simple { target, value }) = statement else {
        return None;
    };
    let GoExpressionNode::Selector { base, field, .. } = target.node() else {
        return None;
    };
    matches!(base.as_ref(), GoExpressionNode::Identifier(name) if name.refers_to_same(copy))
        .then_some((field.as_str(), value.node()))
}

fn reuse_copy_source(
    flat: &mut Vec<Statement>,
    index: usize,
    parameters: &[GoIdentifier],
    scope: &ScopeState,
) -> bool {
    let Some((copy, source)) = copy_define(&flat[index].kind) else {
        return false;
    };
    if !copy_keeps_no_name_of_its_own(&copy, &source, scope) {
        return false;
    }
    // A loop can run this list again and read `source` once more.
    let declared_here = parameters.iter().any(|name| name.refers_to_same(&source))
        || flat[..index]
            .iter()
            .any(|statement| declares(&statement.kind, &source));
    if !declared_here {
        return false;
    }

    let writes: Vec<(&str, &GoExpressionNode)> = flat[index + 1..]
        .iter()
        .map_while(|statement| copy_field_write(&statement.kind, &copy))
        .collect();
    let fields: Vec<&str> = writes.iter().map(|(field, _)| *field).collect();
    let after = index + 1 + writes.len();

    if !flat[..index].iter().all(|statement| {
        SourceReads::of_statement(&statement.kind, &source, &fields).reads_only_by_value()
    }) {
        return false;
    }
    for (position, (_, value)) in writes.iter().enumerate() {
        let reads = SourceReads::of_expression(value, &source, &fields[position..]);
        let reads_fields_not_yet_written = position == 0 || reads.reads_only_listed_fields();
        if !reads.reads_only_by_value() || !reads_fields_not_yet_written {
            return false;
        }
    }
    if flat[after..]
        .iter()
        .any(|statement| SourceReads::of_statement(&statement.kind, &source, &[]).mentions_source())
        || flat.iter().any(|statement| {
            names_in_verbatim(&statement.kind, copy.spelling())
                || names_in_verbatim(&statement.kind, source.spelling())
        })
    {
        return false;
    }

    let mut rest = flat.split_off(index + 1);
    flat.pop();
    visit_statements_mut(
        &mut rest,
        &mut Rename {
            from: &copy,
            to: &source,
        },
    );
    flat.append(&mut rest);
    true
}

struct SourceReads<'a> {
    source: &'a GoIdentifier,
    fields: &'a [&'a str],
    mentions: usize,
    field_reads: usize,
    escapes: bool,
}

impl<'a> SourceReads<'a> {
    fn new(source: &'a GoIdentifier, fields: &'a [&'a str]) -> Self {
        Self {
            source,
            fields,
            mentions: 0,
            field_reads: 0,
            escapes: false,
        }
    }

    fn of_statement(
        statement: &LoweredStatement,
        source: &'a GoIdentifier,
        fields: &'a [&'a str],
    ) -> Self {
        let mut reads = Self::new(source, fields);
        statement.walk(&mut reads);
        reads
    }

    fn of_expression(
        node: &GoExpressionNode,
        source: &'a GoIdentifier,
        fields: &'a [&'a str],
    ) -> Self {
        let mut reads = Self::new(source, fields);
        node.walk(&mut reads);
        reads
    }

    fn reads_only_by_value(&self) -> bool {
        !self.escapes
    }

    fn reads_only_listed_fields(&self) -> bool {
        self.mentions == self.field_reads
    }

    fn mentions_source(&self) -> bool {
        self.mentions > 0
    }

    fn is_source(&self, node: &GoExpressionNode) -> bool {
        matches!(node, GoExpressionNode::Identifier(name) if name.spelling() == self.source.spelling())
    }
}

impl TreeVisitor for SourceReads<'_> {
    fn expression(&mut self, node: &GoExpressionNode) {
        let spelling = self.source.spelling();
        match node {
            GoExpressionNode::Identifier(name) if name.spelling() == spelling => {
                self.mentions += 1;
            }
            GoExpressionNode::Selector { base, .. }
                if !self.is_source(base) && base.mentions(spelling) =>
            {
                self.escapes = true;
            }
            GoExpressionNode::Selector { base, field, .. } if self.is_source(base) => {
                if self.fields.contains(&field.as_str()) {
                    self.field_reads += 1;
                } else {
                    self.escapes = true;
                }
            }
            GoExpressionNode::AddressOf(operand)
            | GoExpressionNode::Slice { base: operand, .. }
                if operand.mentions(spelling) =>
            {
                self.escapes = true;
            }
            GoExpressionNode::Call { callee, .. } if matches!(callee.as_ref(), GoExpressionNode::Selector { base, .. } if base.mentions(spelling)) =>
            {
                self.escapes = true;
            }
            GoExpressionNode::FunctionLiteral {
                parameters, body, ..
            } => {
                if parameters
                    .iter()
                    .any(|parameter| parameter.name.spelling() == spelling)
                {
                    self.mentions += 1;
                }
                let mut inner = SourceReads::new(self.source, &[]);
                body.walk(&mut inner);
                if inner.mentions_source() {
                    self.escapes = true;
                }
            }
            _ => {}
        }
    }

    fn statement(&mut self, statement: &LoweredStatement) {
        if binds_spelling(statement, self.source.spelling()) {
            self.mentions += 1;
        }
    }
}

fn binds_spelling(statement: &LoweredStatement, spelling: &str) -> bool {
    let named = |name: &GoIdentifier| name.spelling() == spelling;
    match statement {
        LoweredStatement::Define(definition) => definition.names.iter().any(named),
        LoweredStatement::VarDecl { name, .. } => named(name),
        LoweredStatement::Const(plan) => named(&plan.name),
        LoweredStatement::If(plan) => plan
            .initializer
            .as_ref()
            .is_some_and(|initializer| initializer.names.iter().any(named)),
        LoweredStatement::Loop(plan) => match &plan.header {
            LoopHeader::Range { key, value, .. } => key.iter().chain(value.iter()).any(named),
            LoopHeader::Counted { variable, .. } => named(variable),
            LoopHeader::Infinite | LoopHeader::While(_) => false,
        },
        LoweredStatement::Select(plan) => plan.arms.iter().any(|arm| {
            matches!(&arm.kind, SelectArmKind::Receive { receive_vars, .. } if receive_vars.iter().any(named))
        }),
        LoweredStatement::Switch(plan) => {
            matches!(&plan.kind, SwitchKind::Type { binding: Some(name), .. } if named(name))
        }
        _ => false,
    }
}

fn names_in_verbatim(statement: &LoweredStatement, spelling: &str) -> bool {
    struct Verbatim<'s> {
        spelling: &'s str,
        found: bool,
    }

    impl TreeVisitor for Verbatim<'_> {
        fn expression(&mut self, node: &GoExpressionNode) {
            if let GoExpressionNode::Verbatim(text) = node {
                self.found |= verbatim_identifiers(text).any(|token| token == self.spelling);
            }
        }
    }

    let mut verbatim = Verbatim {
        spelling,
        found: false,
    };
    statement.walk(&mut verbatim);
    verbatim.found
}

struct Rename<'n> {
    from: &'n GoIdentifier,
    to: &'n GoIdentifier,
}

impl VisitorMut for Rename<'_> {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        if let GoExpressionNode::Identifier(name) = node
            && name.refers_to_same(self.from)
        {
            *name = self.to.clone();
        }
    }
}
