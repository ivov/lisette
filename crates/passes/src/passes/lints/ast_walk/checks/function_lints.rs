use crate::passes::walk::{FunctionRole, NodeCtx};
use syntax::ast::Expression;
use syntax::program::resolved_definition;

use super::{exit_after_defer, unconditional_recursion, waitgroup};

pub fn check_function_lints(
    expression: &Expression,
    ctx: &NodeCtx,
    role: FunctionRole<'_>,
    features: FunctionLintFeatures,
) {
    if features.has_task && features.has_waitgroup_call {
        waitgroup::check_waitgroup(expression, ctx);
    }
    if features.has_defer && features.has_exit_call {
        exit_after_defer::check_exit_after_defer(expression, ctx);
    }
    if features.has_self_call {
        unconditional_recursion::check_unconditional_recursion(expression, ctx, role);
    }
}

fn recursive_target_fingerprint(
    expression: &Expression,
    ctx: &NodeCtx,
    role: FunctionRole<'_>,
) -> Option<(usize, u64)> {
    let Expression::Function { name, .. } = expression else {
        return None;
    };
    let package = ctx.package_id();
    let (length, hash) = match role {
        FunctionRole::Free => (
            package.len() + 1 + name.len(),
            fingerprint_segments(&[package, name]),
        ),
        FunctionRole::ImplMethod { type_name } => (
            package.len() + 1 + type_name.len() + 1 + name.len(),
            fingerprint_segments(&[package, type_name, name]),
        ),
        FunctionRole::InterfaceMethod { .. } => return None,
    };
    Some((length, hash))
}

fn fingerprint_segments(segments: &[&str]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for (index, segment) in segments.iter().enumerate() {
        if index != 0 {
            hash = (hash ^ u64::from(b'.')).wrapping_mul(0x100000001b3);
        }
        for byte in segment.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

pub struct FunctionLintFeatures {
    has_defer: bool,
    has_exit_call: bool,
    has_task: bool,
    has_waitgroup_call: bool,
    has_self_call: bool,
    recursive_target: Option<(usize, u64)>,
}

impl FunctionLintFeatures {
    pub fn new(expression: &Expression, ctx: &NodeCtx, role: FunctionRole<'_>) -> Self {
        Self {
            has_defer: false,
            has_exit_call: false,
            has_task: false,
            has_waitgroup_call: false,
            has_self_call: false,
            recursive_target: recursive_target_fingerprint(expression, ctx, role),
        }
    }

    pub fn observe(&mut self, expression: &Expression) {
        match expression {
            Expression::Defer { .. } => self.has_defer = true,
            Expression::Task { .. } => self.has_task = true,
            Expression::Call {
                expression: callee, ..
            } => {
                self.has_exit_call |= exit_after_defer::is_os_exit(callee);
                self.has_waitgroup_call |= waitgroup::waitgroup_method(callee).is_some();
                self.has_self_call |= self.recursive_target.is_some_and(|target| {
                    resolved_definition(callee).is_some_and(|definition| {
                        definition.len() == target.0
                            && fingerprint_segments(&[definition]) == target.1
                    })
                });
            }
            _ => {}
        }
    }
}
