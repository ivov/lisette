use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, Pattern, SelectArm, Span};
use syntax::program::{File, Module};

use semantics::facts::Facts;
use semantics::store::Store;

pub(crate) struct NodeCtx<'a> {
    pub store: &'a Store,
    pub facts: &'a Facts,
    module: &'a Module,
    file: &'a File,
    pub sink: &'a LocalSink,
    claimed_spans: HashSet<(ClaimKind, Span)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClaimKind {
    AlwaysTrueDisjunction,
    CollapsibleMatch,
    ImpossibleComparison,
    MapKey,
    MinMax,
    OutOfDomain,
    RedundantClosure,
}

impl<'a> NodeCtx<'a> {
    pub fn new(
        store: &'a Store,
        facts: &'a Facts,
        module: &'a Module,
        file: &'a File,
        sink: &'a LocalSink,
    ) -> Self {
        Self {
            store,
            facts,
            module,
            file,
            sink,
            claimed_spans: HashSet::default(),
        }
    }

    pub fn module_id(&self) -> &str {
        &self.module.id
    }

    pub fn source(&self) -> &str {
        &self.file.source
    }

    pub fn is_d_lis(&self) -> bool {
        self.file.is_d_lis()
    }

    pub fn is_test(&self) -> bool {
        self.file.is_test()
    }

    pub fn is_claimed(&self, kind: ClaimKind, span: &Span) -> bool {
        self.claimed_spans.contains(&(kind, *span))
    }

    pub fn claim(&mut self, kind: ClaimKind, span: Span) {
        self.claimed_spans.insert((kind, span));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternRole {
    Parameter,
    #[default]
    Binding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FunctionRole<'a> {
    InterfaceMethod {
        public: bool,
    },
    ImplMethod {
        type_name: &'a str,
    },
    #[default]
    Free,
}

macro_rules! apply_expression_checks {
    ($expression:expr, $ctx:expr, $(($check:path, &[$($kind:ident),+ $(,)?] $(,)?)),* $(,)?) => {
        $(
            if matches!($expression, $(syntax::ast::Expression::$kind { .. })|+) {
                $check($expression, $ctx);
            }
        )*
    };
}

pub(crate) use apply_expression_checks;

macro_rules! apply_pattern_checks {
    ($pattern:expr, $ctx:expr, $(($check:path, &[$($kind:ident),+ $(,)?] $(,)?)),* $(,)?) => {
        $(
            if matches!($pattern, $(syntax::ast::Pattern::$kind { .. })|+) {
                $check($pattern, $ctx);
            }
        )*
    };
}

pub(crate) use apply_pattern_checks;

pub(crate) fn walk_nodes<'a, E, P>(
    ast: &'a [Expression],
    ctx: &mut NodeCtx<'a>,
    expression_checks: E,
    pattern_checks: P,
) where
    E: Fn(&Expression, &mut NodeCtx<'a>, FunctionRole<'a>),
    P: Fn(&Pattern, &mut NodeCtx<'a>, PatternRole),
{
    visit_ast_with(
        ast,
        ctx,
        &mut |expression, role, ctx| {
            expression_checks(expression, ctx, role);
        },
        &mut |pattern, role, ctx| {
            pattern_checks(pattern, ctx, role);
        },
    );
}

pub fn visit_ast<'a, E, P>(
    ast: &'a [Expression],
    expression_visitor: &mut E,
    pattern_visitor: &mut P,
) where
    E: FnMut(&Expression, FunctionRole<'a>),
    P: FnMut(&Pattern, PatternRole),
{
    visit_ast_with(
        ast,
        &mut (),
        &mut |expression, role, ()| expression_visitor(expression, role),
        &mut |pattern, role, ()| pattern_visitor(pattern, role),
    );
}

fn visit_ast_with<'a, C, E, P>(
    ast: &'a [Expression],
    ctx: &mut C,
    expression_visitor: &mut E,
    pattern_visitor: &mut P,
) where
    E: FnMut(&Expression, FunctionRole<'a>, &mut C),
    P: FnMut(&Pattern, PatternRole, &mut C),
{
    for expression in ast {
        visit_node(
            expression,
            FunctionRole::Free,
            ctx,
            expression_visitor,
            pattern_visitor,
        );
    }
}

fn visit_node<'a, C, E, P>(
    expression: &'a Expression,
    role: FunctionRole<'a>,
    ctx: &mut C,
    expression_visitor: &mut E,
    pattern_visitor: &mut P,
) where
    E: FnMut(&Expression, FunctionRole<'a>, &mut C),
    P: FnMut(&Pattern, PatternRole, &mut C),
{
    expression_visitor(expression, role, ctx);

    match expression {
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                visit_pattern(&param.pattern, PatternRole::Parameter, ctx, pattern_visitor);
            }
        }
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            visit_pattern(&binding.pattern, PatternRole::Binding, ctx, pattern_visitor);
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            visit_pattern(pattern, PatternRole::Binding, ctx, pattern_visitor);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                visit_pattern(&arm.pattern, PatternRole::Binding, ctx, pattern_visitor);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive { binding, .. } => {
                        visit_pattern(binding, PatternRole::Binding, ctx, pattern_visitor);
                    }
                    SelectArm::MatchReceive {
                        arms: match_arms, ..
                    } => {
                        for match_arm in match_arms {
                            visit_pattern(
                                &match_arm.pattern,
                                PatternRole::Binding,
                                ctx,
                                pattern_visitor,
                            );
                        }
                    }
                    SelectArm::Send { .. } | SelectArm::WildCard { .. } => {}
                }
            }
        }
        _ => {}
    }

    let child_role = match expression {
        Expression::Interface { visibility, .. } => FunctionRole::InterfaceMethod {
            public: visibility.is_public(),
        },
        Expression::ImplBlock { receiver_name, .. } => FunctionRole::ImplMethod {
            type_name: receiver_name.as_str(),
        },
        _ => FunctionRole::Free,
    };
    for child in expression.children() {
        visit_node(child, child_role, ctx, expression_visitor, pattern_visitor);
    }
}

fn visit_pattern<C, F: FnMut(&Pattern, PatternRole, &mut C)>(
    pattern: &Pattern,
    role: PatternRole,
    ctx: &mut C,
    visitor: &mut F,
) {
    visitor(pattern, role, ctx);

    match pattern {
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::WildCard { .. }
        | Pattern::Identifier { .. } => {}

        Pattern::EnumVariant { fields, .. } => {
            for field in fields {
                visit_pattern(field, role, ctx, visitor);
            }
        }

        Pattern::Struct { fields, .. } => {
            for field in fields {
                visit_pattern(&field.value, role, ctx, visitor);
            }
        }

        Pattern::Tuple { elements, .. } => {
            for element in elements {
                visit_pattern(element, role, ctx, visitor);
            }
        }

        Pattern::Slice { prefix, .. } => {
            for p in prefix {
                visit_pattern(p, role, ctx, visitor);
            }
        }

        Pattern::Or { patterns, .. } => {
            for p in patterns {
                visit_pattern(p, role, ctx, visitor);
            }
        }

        Pattern::AsBinding { pattern, .. } => {
            visit_pattern(pattern, role, ctx, visitor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_only_suppress_their_own_diagnostic_family() {
        let store = Store::new();
        let facts = Facts::new(Default::default());
        let module = Module::new("main");
        let file = File::new_cached("main", "main.lis", "main.lis", "", 0);
        let sink = LocalSink::new();
        let mut ctx = NodeCtx::new(&store, &facts, &module, &file, &sink);
        let span = Span::new(0, 0, 1);

        ctx.claim(ClaimKind::MinMax, span);

        assert!(!ctx.is_claimed(ClaimKind::MapKey, &span));
    }
}
