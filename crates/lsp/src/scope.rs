use rustc_hash::FxHashMap;
use syntax::ast::{Expression, MatchArm, Pattern, SelectArm, collect_pattern_bindings};
use syntax::program::File;
use syntax::types::Type;

use crate::analysis::{cursor_offset, offset_in_span};
use crate::patterns::get_pattern_element_type;
use crate::snapshot::AnalysisSnapshot;
use crate::traversal::expression_ancestors;

/// Bindings visible at the cursor, with inner scopes and later declarations winning.
pub(crate) fn visible_bindings(
    file: &File,
    offset: u32,
    snapshot: &AnalysisSnapshot,
) -> FxHashMap<String, Type> {
    let mut bindings = FxHashMap::default();
    let mut add_pattern = |pattern: &Pattern, ty: &Type| {
        for (name, span) in collect_pattern_bindings(pattern) {
            if let Some((ty, _)) = get_pattern_element_type(snapshot, pattern, ty, span.byte_offset)
            {
                bindings.insert(name, ty);
            }
        }
    };
    let scope_offset = cursor_offset(&file.source, offset)
        .or_else(|| {
            // Recovery ends unfinished blocks at their last token, before trailing
            // whitespace. A closing brace must still leave its scope.
            (offset as usize == file.source.len())
                .then(|| file.source.trim_end().char_indices().next_back())
                .flatten()
                .filter(|(_, c)| *c != '}')
                .map(|(index, _)| index as u32)
        })
        .unwrap_or(offset);
    let contains = |expression: &Expression| offset_in_span(scope_offset, &expression.get_span());

    for ancestor in expression_ancestors(&file.items, scope_offset) {
        match ancestor {
            Expression::Block { items, .. }
            | Expression::TryBlock { items, .. }
            | Expression::RecoverBlock { items, .. } => {
                for item in items {
                    // A declaration is not visible in its own initializer or else block,
                    // including when the caret is at the end of the initializer.
                    let span = item.get_span();
                    if span.byte_offset + span.byte_length >= offset {
                        break;
                    }
                    if let Expression::Let { binding, .. } = item {
                        add_pattern(&binding.pattern, &binding.ty);
                    }
                }
            }
            Expression::Function { params, body, .. }
                if body.definition().is_some_and(contains) =>
            {
                for param in params {
                    add_pattern(&param.pattern, &param.ty);
                }
            }
            Expression::Lambda { params, body, .. } if contains(body) => {
                for param in params {
                    add_pattern(&param.pattern, &param.ty);
                }
            }
            Expression::For { binding, body, .. } if contains(body) => {
                add_pattern(&binding.pattern, &binding.ty);
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                ..
            } if contains(consequence) => add_pattern(pattern, &scrutinee.get_type()),
            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                ..
            } if contains(body) => add_pattern(pattern, &scrutinee.get_type()),
            Expression::Match { subject, arms, .. } => {
                if let Some(arm) = arm_at(arms, scope_offset) {
                    add_pattern(&arm.pattern, &subject.get_type());
                }
            }
            Expression::Select { arms, .. } => {
                for arm in arms {
                    match arm {
                        SelectArm::Receive {
                            binding,
                            receive_expression,
                            body,
                        } if contains(body) => add_pattern(binding, &receive_expression.get_type()),
                        SelectArm::MatchReceive {
                            receive_expression,
                            arms,
                        } => {
                            if let Some(arm) = arm_at(arms, scope_offset) {
                                add_pattern(&arm.pattern, &receive_expression.get_type());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    bindings
}

fn arm_at(arms: &[MatchArm], offset: u32) -> Option<&MatchArm> {
    arms.iter().find(|arm| {
        let pattern = arm.pattern.get_span();
        let body = arm.expression.get_span();
        offset >= pattern.byte_offset + pattern.byte_length
            && offset < body.byte_offset + body.byte_length
    })
}
