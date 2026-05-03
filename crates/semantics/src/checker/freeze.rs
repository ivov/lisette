//! Post-inference freeze pass.
//!
//! After inference finishes, every `Type` field reachable through the AST is
//! env-resolved: bound type variables are substituted with their values,
//! unbound vars are left as-is. Downstream crates (emit, lsp, format, cache)
//! therefore do not need access to the checker's `TypeEnv` — the emitter maps
//! any remaining unbound `Type::Var` to Go's `any`.

use std::convert::Infallible;

use syntax::ast::{
    Binding, EnumFieldDefinition, Expression, MatchArm, Pattern, SelectArm, SelectArmPattern,
    StructFieldDefinition, TypedPattern, VariantFields,
};
use syntax::ast_folder::AstFolder;
use syntax::types::Type;

use crate::checker::type_env::TypeEnv;

pub struct FreezeFolder<'a> {
    env: &'a TypeEnv,
}

impl<'a> FreezeFolder<'a> {
    pub fn new(env: &'a TypeEnv) -> Self {
        Self { env }
    }

    pub fn freeze_items(&mut self, items: Vec<Expression>) -> Vec<Expression> {
        items
            .into_iter()
            .map(|item| {
                let Ok(folded) = self.fold_expression(item);
                folded
            })
            .collect()
    }

    pub fn freeze_facts(&self, facts: &mut crate::facts::Facts) {
        for check in &mut facts.generic_call_checks {
            check.return_ty = self.env.resolve(&check.return_ty);
        }
        for check in &mut facts.empty_collection_checks {
            check.ty = self.env.resolve(&check.ty);
        }
        for check in &mut facts.statement_tail_checks {
            check.expected_ty = self.env.resolve(&check.expected_ty);
        }
    }

    fn freeze_ty(&self, ty: &mut Type) {
        *ty = self.env.resolve(ty);
    }

    fn freeze_binding(&self, binding: &mut Binding) {
        self.freeze_ty(&mut binding.ty);
        self.freeze_pattern(&mut binding.pattern);
        if let Some(tp) = &mut binding.typed_pattern {
            self.freeze_typed_pattern(tp);
        }
    }

    fn freeze_pattern(&self, pattern: &mut Pattern) {
        match pattern {
            Pattern::Literal { ty, .. } | Pattern::Unit { ty, .. } => self.freeze_ty(ty),
            Pattern::EnumVariant { ty, fields, .. } => {
                self.freeze_ty(ty);
                for f in fields {
                    self.freeze_pattern(f);
                }
            }
            Pattern::Struct { ty, fields, .. } => {
                self.freeze_ty(ty);
                for f in fields {
                    self.freeze_pattern(&mut f.value);
                }
            }
            Pattern::Slice {
                element_ty, prefix, ..
            } => {
                self.freeze_ty(element_ty);
                for p in prefix {
                    self.freeze_pattern(p);
                }
            }
            Pattern::Tuple { elements, .. } => {
                for e in elements {
                    self.freeze_pattern(e);
                }
            }
            Pattern::Or { patterns, .. } => {
                for p in patterns {
                    self.freeze_pattern(p);
                }
            }
            Pattern::AsBinding { pattern, .. } => self.freeze_pattern(pattern),
            Pattern::WildCard { .. } | Pattern::Identifier { .. } => {}
        }
    }

    fn freeze_typed_pattern(&self, tp: &mut TypedPattern) {
        match tp {
            TypedPattern::Wildcard | TypedPattern::Literal(_) => {}
            TypedPattern::EnumVariant {
                type_args,
                field_types,
                fields,
                variant_fields,
                ..
            } => {
                for t in type_args {
                    self.freeze_ty(t);
                }
                for t in field_types.iter_mut() {
                    self.freeze_ty(t);
                }
                for f in fields {
                    self.freeze_typed_pattern(f);
                }
                for vf in variant_fields {
                    self.freeze_ty(&mut vf.ty);
                }
            }
            TypedPattern::EnumStructVariant {
                type_args,
                pattern_fields,
                variant_fields,
                ..
            } => {
                for t in type_args {
                    self.freeze_ty(t);
                }
                for (_, f) in pattern_fields {
                    self.freeze_typed_pattern(f);
                }
                for vf in variant_fields {
                    self.freeze_ty(&mut vf.ty);
                }
            }
            TypedPattern::Struct {
                type_args,
                pattern_fields,
                struct_fields,
                ..
            } => {
                for t in type_args {
                    self.freeze_ty(t);
                }
                for (_, f) in pattern_fields {
                    self.freeze_typed_pattern(f);
                }
                for sf in struct_fields {
                    self.freeze_ty(&mut sf.ty);
                }
            }
            TypedPattern::Slice {
                element_type,
                prefix,
                ..
            } => {
                self.freeze_ty(element_type);
                for p in prefix {
                    self.freeze_typed_pattern(p);
                }
            }
            TypedPattern::Tuple { elements, .. } => {
                for e in elements {
                    self.freeze_typed_pattern(e);
                }
            }
            TypedPattern::Or { alternatives } => {
                for a in alternatives {
                    self.freeze_typed_pattern(a);
                }
            }
        }
    }

    fn freeze_struct_field(&self, field: &mut StructFieldDefinition) {
        self.freeze_ty(&mut field.ty);
    }

    fn freeze_enum_field(&self, field: &mut EnumFieldDefinition) {
        self.freeze_ty(&mut field.ty);
    }

    fn freeze_variant_fields(&self, vf: &mut VariantFields) {
        match vf {
            VariantFields::Unit => {}
            VariantFields::Tuple(fields) | VariantFields::Struct(fields) => {
                for f in fields {
                    self.freeze_enum_field(f);
                }
            }
        }
    }

    /// Fold a left-associative Binary chain iteratively. Unrolls the left
    /// spine into a stack, folds each non-Binary leaf via `fold_expression`
    /// (short recursion), then rebuilds bottom-up freezing each Binary as it
    /// goes.
    fn fold_binary_chain(&mut self, expression: Expression) -> Expression {
        let mut stack: Vec<(
            syntax::ast::BinaryOperator,
            Box<Expression>,
            Type,
            syntax::ast::Span,
        )> = Vec::new();
        let mut current = expression;
        loop {
            match current {
                Expression::Binary {
                    operator,
                    left,
                    right,
                    ty,
                    span,
                } => {
                    stack.push((operator, right, ty, span));
                    current = *left;
                }
                other => {
                    current = other;
                    break;
                }
            }
        }
        // Fold the leaf (non-Binary) and each right operand, which may or
        // may not be Binary themselves — `fold_expression` handles that.
        let Ok(mut acc) = self.fold_expression(current);
        while let Some((operator, right, mut ty, span)) = stack.pop() {
            let Ok(right_folded) = self.fold_expression(*right);
            self.freeze_ty(&mut ty);
            acc = Expression::Binary {
                operator,
                left: Box::new(acc),
                right: Box::new(right_folded),
                ty,
                span,
            };
        }
        acc
    }

    /// Freeze all `Type` fields on the outer expression and on any nested
    /// structural nodes (bindings, patterns, variant fields, interface
    /// methods) that the `AstFolder` default does not walk.
    fn freeze_outer(&mut self, expression: &mut Expression) {
        match expression {
            Expression::Literal { ty, .. }
            | Expression::Identifier { ty, .. }
            | Expression::Call { ty, .. }
            | Expression::If { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Tuple { ty, .. }
            | Expression::StructCall { ty, .. }
            | Expression::DotAccess { ty, .. }
            | Expression::Return { ty, .. }
            | Expression::Propagate { ty, .. }
            | Expression::TryBlock { ty, .. }
            | Expression::RecoverBlock { ty, .. }
            | Expression::ImplBlock { ty, .. }
            | Expression::Binary { ty, .. }
            | Expression::Unary { ty, .. }
            | Expression::Paren { ty, .. }
            | Expression::Const { ty, .. }
            | Expression::VariableDeclaration { ty, .. }
            | Expression::Loop { ty, .. }
            | Expression::Reference { ty, .. }
            | Expression::IndexedAccess { ty, .. }
            | Expression::Task { ty, .. }
            | Expression::Defer { ty, .. }
            | Expression::Select { ty, .. }
            | Expression::Unit { ty, .. }
            | Expression::Range { ty, .. }
            | Expression::Cast { ty, .. }
            | Expression::Block { ty, .. } => self.freeze_ty(ty),

            Expression::Function {
                ty,
                return_type,
                params,
                ..
            } => {
                self.freeze_ty(ty);
                self.freeze_ty(return_type);
                for p in params {
                    self.freeze_binding(p);
                }
            }

            Expression::Lambda { ty, params, .. } => {
                self.freeze_ty(ty);
                for p in params {
                    self.freeze_binding(p);
                }
            }

            Expression::Let {
                ty,
                binding,
                typed_pattern,
                ..
            } => {
                self.freeze_ty(ty);
                self.freeze_binding(binding);
                if let Some(tp) = typed_pattern {
                    self.freeze_typed_pattern(tp);
                }
            }

            Expression::IfLet {
                ty,
                pattern,
                typed_pattern,
                ..
            } => {
                self.freeze_ty(ty);
                self.freeze_pattern(pattern);
                if let Some(tp) = typed_pattern {
                    self.freeze_typed_pattern(tp);
                }
            }

            Expression::For { binding, .. } => {
                self.freeze_binding(binding);
            }

            Expression::WhileLet {
                pattern,
                typed_pattern,
                ..
            } => {
                self.freeze_pattern(pattern);
                if let Some(tp) = typed_pattern {
                    self.freeze_typed_pattern(tp);
                }
            }

            Expression::Struct { fields, .. } => {
                for f in fields {
                    self.freeze_struct_field(f);
                }
            }

            Expression::Enum { variants, .. } => {
                for v in variants {
                    self.freeze_variant_fields(&mut v.fields);
                }
            }

            Expression::TypeAlias { ty, .. } => self.freeze_ty(ty),

            Expression::Interface {
                parents,
                method_signatures,
                ..
            } => {
                for parent in parents {
                    self.freeze_ty(&mut parent.ty);
                }
                let sigs = std::mem::take(method_signatures);
                *method_signatures = sigs
                    .into_iter()
                    .map(|s| {
                        let Ok(folded) = self.fold_expression(s);
                        folded
                    })
                    .collect();
            }

            Expression::Assignment { .. }
            | Expression::While { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::ValueEnum { .. }
            | Expression::ModuleImport { .. }
            | Expression::RawGo { .. }
            | Expression::NoOp => {}
        }
    }
}

impl<'a> AstFolder for FreezeFolder<'a> {
    type Error = Infallible;

    fn fold_expression(&mut self, expression: Expression) -> Result<Expression, Self::Error> {
        // Left-associative binary chains (`a + b + c + ...`) are left-deep
        // in the AST, so a naive recursive fold blows the stack on stress
        // inputs (500+ operators). Unroll them into an explicit stack and
        // rebuild bottom-up to keep recursion shallow.
        if let Expression::Binary { .. } = &expression {
            return Ok(self.fold_binary_chain(expression));
        }
        let mut expression = self.fold_expression_default(expression)?;
        self.freeze_outer(&mut expression);
        Ok(expression)
    }

    fn fold_match_arm(&mut self, mut arm: MatchArm) -> Result<MatchArm, Self::Error> {
        arm.expression = Box::new(self.fold_expression(*arm.expression)?);
        arm.guard = arm
            .guard
            .map(|g| self.fold_expression(*g).map(Box::new))
            .transpose()?;
        self.freeze_pattern(&mut arm.pattern);
        if let Some(tp) = &mut arm.typed_pattern {
            self.freeze_typed_pattern(tp);
        }
        Ok(arm)
    }

    fn fold_select_arm(&mut self, arm: SelectArm) -> Result<SelectArm, Self::Error> {
        let pattern = match arm.pattern {
            SelectArmPattern::Receive {
                mut binding,
                mut typed_pattern,
                receive_expression,
                body,
            } => {
                self.freeze_pattern(&mut binding);
                if let Some(tp) = &mut typed_pattern {
                    self.freeze_typed_pattern(tp);
                }
                SelectArmPattern::Receive {
                    binding,
                    typed_pattern,
                    receive_expression: Box::new(self.fold_expression(*receive_expression)?),
                    body: Box::new(self.fold_expression(*body)?),
                }
            }
            SelectArmPattern::Send {
                send_expression,
                body,
            } => SelectArmPattern::Send {
                send_expression: Box::new(self.fold_expression(*send_expression)?),
                body: Box::new(self.fold_expression(*body)?),
            },
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => SelectArmPattern::MatchReceive {
                receive_expression: Box::new(self.fold_expression(*receive_expression)?),
                arms: arms
                    .into_iter()
                    .map(|arm| self.fold_match_arm(arm))
                    .collect::<Result<_, _>>()?,
            },
            SelectArmPattern::WildCard { body } => SelectArmPattern::WildCard {
                body: Box::new(self.fold_expression(*body)?),
            },
        };
        Ok(SelectArm { pattern })
    }
}
