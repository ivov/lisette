use diagnostics::LocalSink;

use crate::facts::Facts;

pub(super) fn run(facts: &mut Facts, sink: &LocalSink) {
    for check in std::mem::take(&mut facts.generic_call_checks) {
        if check.return_ty.has_unbound_variables() {
            sink.push(diagnostics::infer::cannot_infer_type_argument(check.span));
        }
    }
    for check in std::mem::take(&mut facts.empty_collection_checks) {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::uninferred_binding(
                &check.name,
                check.span,
            ));
        }
    }
    for check in std::mem::take(&mut facts.statement_tail_checks) {
        if !check.expected_ty.is_unit()
            && !check.expected_ty.is_variable()
            && !check.expected_ty.is_ignored()
        {
            sink.push(diagnostics::infer::statement_as_tail(check.span));
        }
    }
}
