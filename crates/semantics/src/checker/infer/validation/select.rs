use syntax::ast::{Pattern, Span};

use crate::checker::Checker;

impl Checker<'_, '_> {
    pub(crate) fn check_select_match_arms(
        &mut self,
        match_arms: &[syntax::ast::MatchArm],
        receive_span: Span,
    ) {
        if match_arms.is_empty() {
            self.sink
                .push(diagnostics::infer::select_match_missing_some_arm(
                    receive_span,
                ));
            self.sink
                .push(diagnostics::infer::select_match_missing_none_arm(
                    receive_span,
                ));
            return;
        }

        let mut some_arm_span: Option<Span> = None;
        let mut none_arm_span: Option<Span> = None;

        for arm in match_arms {
            if let Some(guard) = &arm.guard {
                let guard_span = guard.get_span();
                let full_span = Span::new(
                    guard_span.file_id,
                    guard_span.byte_offset.saturating_sub(3), // "if "
                    guard_span.byte_length + 3,
                );
                self.sink
                    .push(diagnostics::infer::select_match_guard_not_allowed(
                        full_span,
                    ));
            }

            let inner_arm_pattern = if let Pattern::AsBinding {
                pattern: inner,
                span,
                ..
            } = &arm.pattern
            {
                if let Pattern::EnumVariant { identifier, .. } = inner.as_ref()
                    && identifier.rsplit('.').next().unwrap_or(identifier) == "Some"
                {
                    self.sink
                        .push(diagnostics::infer::select_some_as_binding_not_supported(
                            *span,
                        ));
                }
                inner.as_ref()
            } else {
                &arm.pattern
            };

            if let Pattern::EnumVariant {
                identifier, fields, ..
            } = inner_arm_pattern
            {
                let variant_name = identifier.rsplit('.').next().unwrap_or(identifier);

                if variant_name == "Some" && fields.len() == 1 {
                    if some_arm_span.is_some() {
                        self.sink
                            .push(diagnostics::infer::select_match_duplicate_some_arm(
                                arm.pattern.get_span(),
                            ));
                    } else {
                        some_arm_span = Some(arm.pattern.get_span());
                    }

                    if !Self::is_irrefutable_select_pattern(&fields[0]) {
                        self.sink
                            .push(diagnostics::infer::select_receive_refutable_pattern(
                                fields[0].get_span(),
                            ));
                    }
                } else if variant_name == "None" && fields.is_empty() {
                    if none_arm_span.is_some() {
                        self.sink
                            .push(diagnostics::infer::select_match_duplicate_none_arm(
                                arm.pattern.get_span(),
                            ));
                    } else {
                        none_arm_span = Some(arm.pattern.get_span());
                    }
                } else {
                    self.sink
                        .push(diagnostics::infer::select_match_invalid_pattern(
                            arm.pattern.get_span(),
                        ));
                }
            } else {
                self.sink
                    .push(diagnostics::infer::select_match_invalid_pattern(
                        arm.pattern.get_span(),
                    ));
            }
        }

        if some_arm_span.is_none() {
            let span = match_arms.first().map(|a| a.pattern.get_span()).unwrap();
            self.sink
                .push(diagnostics::infer::select_match_missing_some_arm(span));
        }
        if none_arm_span.is_none() {
            let span = match_arms.last().map(|a| a.pattern.get_span()).unwrap();
            self.sink
                .push(diagnostics::infer::select_match_missing_none_arm(span));
        }
    }

    /// Check if a pattern always matches any value (irrefutable).
    pub(crate) fn is_irrefutable_select_pattern(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::WildCard { .. } | Pattern::Identifier { .. } => true,
            Pattern::Tuple { elements, .. } => {
                elements.iter().all(Self::is_irrefutable_select_pattern)
            }
            Pattern::Struct { fields, .. } => fields
                .iter()
                .all(|f| Self::is_irrefutable_select_pattern(&f.value)),
            Pattern::AsBinding { pattern, .. } => Self::is_irrefutable_select_pattern(pattern),
            _ => false,
        }
    }

    /// Check for multiple shorthand receive arms in select.
    /// Multiple `let Some(v) = ch.receive()` arms can lead to unexpected behavior
    /// when one channel is closed - it may be selected over channels with values.
    pub(crate) fn check_multiple_select_receives(&mut self, arms: &[syntax::ast::SelectArm]) {
        use syntax::ast::SelectArmPattern;

        let mut first_receive_span: Option<Span> = None;

        for arm in arms {
            let SelectArmPattern::Receive { binding, .. } = &arm.pattern else {
                continue;
            };
            let inner = match binding.as_ref() {
                Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
                p => p,
            };
            let Pattern::EnumVariant {
                identifier, fields, ..
            } = inner
            else {
                continue;
            };
            let variant_name = identifier.rsplit('.').next().unwrap_or(identifier);
            if variant_name == "Some" && fields.len() == 1 {
                if let Some(first_span) = first_receive_span {
                    self.sink.push(diagnostics::infer::multiple_select_receives(
                        first_span,
                        binding.get_span(),
                    ));
                    return; // Only report once
                } else {
                    first_receive_span = Some(binding.get_span());
                }
            }
        }
    }

    pub(crate) fn check_duplicate_select_defaults(&mut self, arms: &[syntax::ast::SelectArm]) {
        use syntax::ast::SelectArmPattern;

        let mut first_default_span: Option<Span> = None;

        for arm in arms {
            if let SelectArmPattern::WildCard { body } = &arm.pattern {
                if let Some(first_span) = first_default_span {
                    self.sink.push(diagnostics::infer::duplicate_select_default(
                        first_span,
                        body.get_span(),
                    ));
                    return;
                } else {
                    first_default_span = Some(body.get_span());
                }
            }
        }
    }
}
