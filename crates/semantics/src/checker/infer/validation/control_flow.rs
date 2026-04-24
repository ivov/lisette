use syntax::ast::Span;

use crate::checker::TaskState;

impl TaskState<'_> {
    pub(crate) fn check_return_in_try_block(&mut self, span: Span) {
        if self.scopes.lookup_try_block_context().is_some() {
            self.sink
                .push(diagnostics::infer::return_in_try_block(span));
        }
    }

    pub(crate) fn check_break_outside_loop(&mut self, span: Span) {
        // Only emit the generic "break outside loop" error when not in a
        // try/recover/defer block, since those have their own more specific errors.
        if !self.scopes.is_inside_loop()
            && self.scopes.lookup_try_block_context().is_none()
            && self.scopes.lookup_recover_block_context().is_none()
            && !self.scopes.is_inside_defer_block()
        {
            self.sink.push(diagnostics::infer::break_outside_loop(span));
        }
    }

    pub(crate) fn check_continue_outside_loop(&mut self, span: Span) {
        // Only emit the generic "continue outside loop" error when not in a
        // try/recover/defer block, since those have their own more specific errors.
        if !self.scopes.is_inside_loop()
            && self.scopes.lookup_try_block_context().is_none()
            && self.scopes.lookup_recover_block_context().is_none()
            && !self.scopes.is_inside_defer_block()
        {
            self.sink
                .push(diagnostics::infer::continue_outside_loop(span));
        }
    }

    pub(crate) fn check_break_in_try_block(&mut self, span: Span) {
        if let Some(ctx) = self.scopes.lookup_try_block_context()
            && !ctx.loop_depth.is_active()
        {
            self.sink.push(diagnostics::infer::break_in_try_block(span));
        }
    }

    pub(crate) fn check_continue_in_try_block(&mut self, span: Span) {
        if let Some(ctx) = self.scopes.lookup_try_block_context()
            && !ctx.loop_depth.is_active()
        {
            self.sink
                .push(diagnostics::infer::continue_in_try_block(span));
        }
    }

    pub(crate) fn check_return_in_recover_block(&mut self, span: Span) {
        if self.scopes.lookup_recover_block_context().is_some() {
            self.sink
                .push(diagnostics::infer::return_in_recover_block(span));
        }
    }

    pub(crate) fn check_break_in_recover_block(&mut self, span: Span) {
        if let Some(ctx) = self.scopes.lookup_recover_block_context()
            && !ctx.loop_depth.is_active()
        {
            self.sink
                .push(diagnostics::infer::break_in_recover_block(span));
        }
    }

    pub(crate) fn check_continue_in_recover_block(&mut self, span: Span) {
        if let Some(ctx) = self.scopes.lookup_recover_block_context()
            && !ctx.loop_depth.is_active()
        {
            self.sink
                .push(diagnostics::infer::continue_in_recover_block(span));
        }
    }

    pub(crate) fn check_return_in_defer_block(&mut self, span: Span) {
        if self.scopes.is_inside_defer_block() {
            self.sink
                .push(diagnostics::infer::return_in_defer_block(span));
        }
    }

    pub(crate) fn check_break_in_defer_block(&mut self, span: Span) {
        if self.scopes.is_inside_defer_block() && self.scopes.defer_block_loop_depth() == 0 {
            self.sink
                .push(diagnostics::infer::break_in_defer_block(span));
        }
    }

    pub(crate) fn check_continue_in_defer_block(&mut self, span: Span) {
        if self.scopes.is_inside_defer_block() && self.scopes.defer_block_loop_depth() == 0 {
            self.sink
                .push(diagnostics::infer::continue_in_defer_block(span));
        }
    }
}
