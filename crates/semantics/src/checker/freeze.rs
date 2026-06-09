//! Post-inference freeze pass.
//!
//! After inference finishes, every `Type` field reachable through the AST is
//! env-resolved: bound type variables are substituted with their values,
//! unbound vars are left as-is. Downstream crates (emit, lsp, format, cache)
//! therefore do not need access to the checker's `TypeEnv` — the emitter maps
//! any remaining unbound `Type::Var` to Go's `any`.

use syntax::ast::{
    Binding, EnumFieldDefinition, Expression, FormatStringPart, Literal, Pattern, SelectArm,
    SelectArmPattern, StructFieldDefinition, StructSpread, TypedPattern, VariantFields,
};
use syntax::types::Type;

use crate::checker::type_env::TypeEnv;

pub struct FreezeFolder<'a> {
    env: &'a TypeEnv,
}

impl<'a> FreezeFolder<'a> {
    pub fn new(env: &'a TypeEnv) -> Self {
        Self { env }
    }

    pub fn freeze_items(&mut self, mut items: Vec<Expression>) -> Vec<Expression> {
        for item in &mut items {
            self.freeze_expr(item);
        }
        items
    }

    fn freeze_expr(&mut self, expression: &mut Expression) {
        if let Expression::Binary { .. } = expression {
            let mut current = expression;
            loop {
                match current {
                    Expression::Binary {
                        left, right, ty, ..
                    } => {
                        self.freeze_ty(ty);
                        self.freeze_expr(right.as_mut());
                        current = left.as_mut();
                    }
                    leaf => {
                        self.freeze_expr(leaf);
                        break;
                    }
                }
            }
            return;
        }

        self.recurse_children(expression);
        self.freeze_outer(expression);
    }

    fn recurse_children(&mut self, expression: &mut Expression) {
        match expression {
            Expression::Block { items, .. }
            | Expression::TryBlock { items, .. }
            | Expression::RecoverBlock { items, .. }
            | Expression::Tuple {
                elements: items, ..
            } => {
                for item in items {
                    self.freeze_expr(item);
                }
            }

            Expression::ImplBlock { methods, .. } => {
                for method in methods {
                    self.freeze_expr(method);
                }
            }

            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                self.freeze_expr(expression.as_mut());
                for arg in args {
                    self.freeze_expr(arg);
                }
                if let Some(spread) = spread.as_mut() {
                    self.freeze_expr(spread);
                }
            }

            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.freeze_expr(condition.as_mut());
                self.freeze_expr(consequence.as_mut());
                self.freeze_expr(alternative.as_mut());
            }

            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.freeze_expr(scrutinee.as_mut());
                self.freeze_expr(consequence.as_mut());
                self.freeze_expr(alternative.as_mut());
            }

            Expression::Match { subject, arms, .. } => {
                self.freeze_expr(subject.as_mut());
                for arm in arms {
                    self.freeze_pattern(&mut arm.pattern);
                    if let Some(tp) = &mut arm.typed_pattern {
                        self.freeze_typed_pattern(tp);
                    }
                    self.freeze_expr(arm.expression.as_mut());
                    if let Some(guard) = &mut arm.guard {
                        self.freeze_expr(guard.as_mut());
                    }
                }
            }

            Expression::Let {
                value, else_block, ..
            } => {
                self.freeze_expr(value.as_mut());
                if let Some(else_block) = else_block {
                    self.freeze_expr(else_block.as_mut());
                }
            }

            Expression::Return { expression, .. }
            | Expression::Propagate { expression, .. }
            | Expression::Unary { expression, .. }
            | Expression::Paren { expression, .. }
            | Expression::DotAccess { expression, .. }
            | Expression::Reference { expression, .. }
            | Expression::Task { expression, .. }
            | Expression::Defer { expression, .. }
            | Expression::Cast { expression, .. }
            | Expression::Const { expression, .. } => {
                self.freeze_expr(expression.as_mut());
            }

            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.freeze_expr(expression.as_mut());
                self.freeze_expr(index.as_mut());
            }

            Expression::Assignment { target, value, .. } => {
                self.freeze_expr(target.as_mut());
                self.freeze_expr(value.as_mut());
            }

            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                for assignment in field_assignments {
                    self.freeze_expr(assignment.value.as_mut());
                }
                if let StructSpread::From(spread) = spread {
                    self.freeze_expr(spread.as_mut());
                }
            }

            Expression::Function { body, .. }
            | Expression::Lambda { body, .. }
            | Expression::Loop { body, .. } => {
                self.freeze_expr(body.as_mut());
            }

            Expression::For { iterable, body, .. } => {
                self.freeze_expr(iterable.as_mut());
                self.freeze_expr(body.as_mut());
            }

            Expression::While {
                condition, body, ..
            } => {
                self.freeze_expr(condition.as_mut());
                self.freeze_expr(body.as_mut());
            }

            Expression::WhileLet {
                scrutinee, body, ..
            } => {
                self.freeze_expr(scrutinee.as_mut());
                self.freeze_expr(body.as_mut());
            }

            Expression::Select { arms, .. } => {
                for arm in arms {
                    self.recurse_select_arm(arm);
                }
            }

            Expression::Break {
                value: Some(value), ..
            } => {
                self.freeze_expr(value.as_mut());
            }

            Expression::Range { start, end, .. } => {
                if let Some(start) = start {
                    self.freeze_expr(start.as_mut());
                }
                if let Some(end) = end {
                    self.freeze_expr(end.as_mut());
                }
            }

            Expression::Literal { literal, .. } => match literal {
                Literal::Slice(elements) => {
                    for element in elements {
                        self.freeze_expr(element);
                    }
                }
                Literal::FormatString(parts) => {
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            self.freeze_expr(expression.as_mut());
                        }
                    }
                }
                _ => {}
            },

            Expression::Binary { .. }
            | Expression::Interface { .. }
            | Expression::Identifier { .. }
            | Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::ModuleImport { .. }
            | Expression::Break { value: None, .. }
            | Expression::Continue { .. }
            | Expression::Unit { .. }
            | Expression::RawGo { .. }
            | Expression::NoOp => {}
        }
    }

    fn recurse_select_arm(&mut self, arm: &mut SelectArm) {
        match &mut arm.pattern {
            SelectArmPattern::Receive {
                binding,
                typed_pattern,
                receive_expression,
                body,
            } => {
                self.freeze_pattern(binding);
                if let Some(tp) = typed_pattern {
                    self.freeze_typed_pattern(tp);
                }
                self.freeze_expr(receive_expression.as_mut());
                self.freeze_expr(body.as_mut());
            }
            SelectArmPattern::Send {
                send_expression,
                body,
            } => {
                self.freeze_expr(send_expression.as_mut());
                self.freeze_expr(body.as_mut());
            }
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => {
                self.freeze_expr(receive_expression.as_mut());
                for arm in arms {
                    self.freeze_pattern(&mut arm.pattern);
                    if let Some(tp) = &mut arm.typed_pattern {
                        self.freeze_typed_pattern(tp);
                    }
                    self.freeze_expr(arm.expression.as_mut());
                    if let Some(guard) = &mut arm.guard {
                        self.freeze_expr(guard.as_mut());
                    }
                }
            }
            SelectArmPattern::WildCard { body } => {
                self.freeze_expr(body.as_mut());
            }
        }
    }

    pub fn freeze_facts(&self, facts: &mut crate::facts::Facts) {
        for check in &mut facts.generic_call_checks {
            self.env.resolve_in_place(&mut check.return_ty);
        }
        for check in &mut facts.empty_collection_checks {
            self.env.resolve_in_place(&mut check.ty);
        }
        for check in &mut facts.statement_tail_checks {
            self.env.resolve_in_place(&mut check.expected_ty);
        }
    }

    fn freeze_ty(&self, ty: &mut Type) {
        self.env.resolve_in_place(ty);
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
            TypedPattern::Const { ty, .. } => self.freeze_ty(ty),
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

    /// Freeze all `Type` fields on the outer expression and on any nested
    /// structural nodes (bindings, patterns, variant fields, interface
    /// methods) that `recurse_children` does not walk.
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
                for signature in method_signatures {
                    self.freeze_expr(signature);
                }
            }

            Expression::Assignment { .. }
            | Expression::While { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::ModuleImport { .. }
            | Expression::RawGo { .. }
            | Expression::NoOp => {}
        }
    }
}
