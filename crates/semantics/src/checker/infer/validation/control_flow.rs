use syntax::ast::{Expression, Span};
use syntax::types::Type;

use crate::checker::infer::InferCtx;
use crate::checker::scopes::FallibleBlockKind;

#[derive(Clone, Copy)]
pub(crate) enum LoopExit {
    Break,
    Continue,
}

fn is_sync_lock_type(ty: &Type) -> bool {
    matches!(
        ty.strip_refs(),
        Type::Nominal { id, .. }
            if matches!(id.as_str(), "go:sync.Mutex" | "go:sync.RWMutex" | "go:sync.Locker")
    )
}

impl InferCtx<'_> {
    pub(crate) fn check_deferred_lock(&mut self, deferred: &Expression) {
        let Expression::Call {
            expression: callee,
            args,
            span,
            ..
        } = deferred.unwrap_parens()
        else {
            return;
        };

        if !args.is_empty() {
            return;
        }

        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee.unwrap_parens()
        else {
            return;
        };

        let (locked, unlock) = match member.as_str() {
            "Lock" => ("Lock", "Unlock"),
            "RLock" => ("RLock", "RUnlock"),
            _ => return,
        };

        if !is_sync_lock_type(&receiver.get_type()) {
            return;
        }

        self.sink
            .push(diagnostics::infer::deferred_lock(*span, locked, unlock));
    }

    pub(crate) fn check_defer_in_fallible_block(&mut self, span: Span) {
        match self.scopes.lookup_fallible_block_kind() {
            Some(FallibleBlockKind::Try) => {
                self.sink.push(diagnostics::infer::defer_in_try_block(span));
            }
            Some(FallibleBlockKind::Recover) => {
                self.sink
                    .push(diagnostics::infer::defer_in_recover_block(span));
            }
            None => {}
        }
    }

    /// Check loop exits in diagnostic order, preserving the nearest block boundaries.
    pub(crate) fn check_loop_exit(&self, span: Span, exit: LoopExit) {
        if !self.is_inside_loop()
            && self.scopes.lookup_try_block_context().is_none()
            && self.scopes.lookup_recover_block_context().is_none()
            && !self.is_inside_defer_block()
        {
            self.sink.push(match exit {
                LoopExit::Break => diagnostics::infer::break_outside_loop(span),
                LoopExit::Continue => diagnostics::infer::continue_outside_loop(span),
            });
        }
        if let Some(context) = self.scopes.lookup_try_block_context()
            && self.loop_depth() == context.entry_loop_depth
        {
            self.sink.push(match exit {
                LoopExit::Break => diagnostics::infer::break_in_try_block(span),
                LoopExit::Continue => diagnostics::infer::continue_in_try_block(span),
            });
        }
        if let Some(context) = self.scopes.lookup_recover_block_context()
            && self.loop_depth() == context.entry_loop_depth
        {
            self.sink.push(match exit {
                LoopExit::Break => diagnostics::infer::break_in_recover_block(span),
                LoopExit::Continue => diagnostics::infer::continue_in_recover_block(span),
            });
        }
        if self.is_inside_defer_block() && self.loop_depth() == 0 {
            self.sink.push(match exit {
                LoopExit::Break => diagnostics::infer::break_in_defer_block(span),
                LoopExit::Continue => diagnostics::infer::continue_in_defer_block(span),
            });
        }
    }

    pub(crate) fn check_return_in_try_block(&mut self, span: Span) {
        if self.scopes.lookup_try_block_context().is_some() {
            self.sink
                .push(diagnostics::infer::return_in_try_block(span));
        }
    }

    pub(crate) fn check_return_in_recover_block(&mut self, span: Span) {
        if self.scopes.lookup_recover_block_context().is_some() {
            self.sink
                .push(diagnostics::infer::return_in_recover_block(span));
        }
    }

    pub(crate) fn check_return_in_defer_block(&mut self, span: Span) {
        if self.is_inside_defer_block() {
            self.sink
                .push(diagnostics::infer::return_in_defer_block(span));
        }
    }
}
