use crate::Emitter;
use crate::expressions::context::ExpressionContext;
use crate::patterns::sites::PatternSubject;
use crate::write_line;
use syntax::ast::{Binding, Expression, Pattern};

enum LetKind {
    /// Simple identifier binding: `let x = expression`
    SimpleIdentifier,
    /// Discard pattern: `let _ = expression`
    Discard,
    /// Complex pattern with temp var: `let (a, b) = expression`
    ComplexPattern,
    /// Go multi-value call optimization: `let (a, b) = go_func()`
    MultiValueCall,
    /// Let-else binding: `let P = expression else { ... }`
    LetElse,
}

pub(crate) struct LetEmitter<'a, 'e> {
    emitter: &'a mut Emitter<'e>,
    binding: &'a Binding,
    value: &'a Expression,
    else_block: Option<&'a Expression>,
    mutable: bool,
}

impl<'a, 'e> LetEmitter<'a, 'e> {
    pub(crate) fn new(
        emitter: &'a mut Emitter<'e>,
        binding: &'a Binding,
        value: &'a Expression,
        else_block: Option<&'a Expression>,
        mutable: bool,
    ) -> Self {
        Self {
            emitter,
            binding,
            value,
            else_block,
            mutable,
        }
    }

    pub(crate) fn emit(mut self, output: &mut String) {
        // Never-typed values diverge (break/continue/return).
        // Declare the binding variable (so later dead code can reference it),
        // then emit the value as a statement.
        if self.value.get_type().is_never() {
            self.emit_never_binding(output);
            return;
        }
        match self.classify() {
            LetKind::LetElse => {
                let else_block = self
                    .else_block
                    .expect("LetKind::LetElse classified without else block");
                self.emitter.emit_let_else_pattern_site(
                    output,
                    &self.binding.pattern,
                    self.binding.typed_pattern.as_ref(),
                    &self.binding.ty,
                    self.value,
                    else_block,
                );
            }
            LetKind::SimpleIdentifier => self.emit_simple_identifier(output),
            LetKind::Discard => self.emit_discard(output),
            LetKind::MultiValueCall => self.emit_multi_value_call(output),
            LetKind::ComplexPattern => {
                let value_ty = self.value.get_type();
                self.emitter.emit_irrefutable_pattern_site(
                    output,
                    PatternSubject::expression(self.value, &self.binding.pattern, None),
                    &self.binding.pattern,
                    self.binding.typed_pattern.as_ref(),
                    &value_ty,
                );
            }
        }
    }

    /// Handle a let binding whose value expression diverges (Never type).
    /// Declare the variable with its zero value so dead code can reference it,
    /// then emit the diverging value as a statement.
    fn emit_never_binding(&mut self, output: &mut String) {
        if let Pattern::Identifier { identifier, .. } = &self.binding.pattern
            && let Some(raw_go_name) = self.emitter.go_name_for_binding(&self.binding.pattern)
        {
            let go_identifier = self.emitter.scope.bind(identifier, &raw_go_name);
            self.emitter.try_declare(&go_identifier);
            let var_ty = self.emitter.go_type_as_string(&self.binding.ty);
            write_line!(output, "var {} {}", go_identifier, var_ty);
        }
        self.emitter.emit_statement(output, self.value);
    }

    fn classify(&self) -> LetKind {
        if self.else_block.is_some() {
            return LetKind::LetElse;
        }

        match &self.binding.pattern {
            Pattern::Identifier { .. } => LetKind::SimpleIdentifier,
            Pattern::WildCard { .. } => LetKind::Discard,
            Pattern::Tuple { elements, .. } => {
                let all_unused = elements.iter().all(|el| match el {
                    Pattern::WildCard { .. } => true,
                    Pattern::Identifier { .. } => self.emitter.facts.is_unused_binding(el),
                    _ => false,
                });
                if all_unused {
                    LetKind::Discard
                } else if self.can_use_multi_value_optimization() {
                    LetKind::MultiValueCall
                } else {
                    LetKind::ComplexPattern
                }
            }
            _ => LetKind::ComplexPattern,
        }
    }

    /// Check if we can use Go multi-value call optimization.
    ///
    /// This optimization applies when:
    /// 1. The pattern is a tuple of simple patterns (identifiers/wildcards)
    /// 2. The value is a Go function call returning multiple values
    /// 3. The result type is not Result (which needs wrapping)
    fn can_use_multi_value_optimization(&self) -> bool {
        let Pattern::Tuple { .. } = &self.binding.pattern else {
            return false;
        };

        self.emitter
            .resolve_go_call_strategy(self.value)
            .is_some_and(|s| s.is_multi_return())
            && !self.value.get_type().is_result()
            && extract_simple_tuple_vars(&self.binding.pattern).is_some()
    }

    fn emit_simple_identifier(&mut self, output: &mut String) {
        let Pattern::Identifier { identifier, .. } = &self.binding.pattern else {
            unreachable!("emit_simple_identifier called with non-identifier pattern");
        };
        let raw_go_name = self.emitter.go_name_for_binding(&self.binding.pattern);
        self.emitter.emit_let_value(
            output,
            identifier,
            raw_go_name.as_deref(),
            self.value,
            &self.binding.ty,
            self.mutable,
        );
    }

    fn emit_discard(&mut self, output: &mut String) {
        self.emitter.emit_discard(output, self.value);
    }

    fn emit_multi_value_call(&mut self, output: &mut String) {
        let Pattern::Tuple { elements, .. } = &self.binding.pattern else {
            unreachable!("emit_multi_value_call called with non-tuple pattern");
        };

        let vars = extract_simple_tuple_vars(&self.binding.pattern)
            .expect("multi-value optimization requires simple tuple vars");

        let mut any_new = false;
        let mut planned: Vec<Option<(&str, String)>> = Vec::new();
        let go_vars: Vec<String> = vars
            .iter()
            .zip(elements.iter())
            .map(|(var, pat)| {
                if var == "_" {
                    planned.push(None);
                    "_".to_string()
                } else if let Pattern::Identifier { identifier, .. } = pat
                    && let Some(go_name) = self.emitter.go_name_for_binding(pat)
                {
                    let escaped = crate::escape_reserved(&go_name).into_owned();
                    let name = if self.emitter.is_declared(&escaped) {
                        let fresh = self.emitter.fresh_var(Some(identifier));
                        any_new = true;
                        fresh
                    } else {
                        any_new = true;
                        escaped
                    };
                    planned.push(Some((identifier, name.clone())));
                    name
                } else {
                    planned.push(None);
                    "_".to_string()
                }
            })
            .collect();

        let call_str = self
            .emitter
            .emit_call(output, self.value, None, ExpressionContext::value());

        for (identifier, go_name) in planned.iter().flatten() {
            self.emitter.scope.bind(*identifier, go_name);
            self.emitter.try_declare(go_name);
        }

        let op = if any_new { ":=" } else { "=" };
        write_line!(output, "{} {} {}", go_vars.join(", "), op, call_str);
    }
}

/// Extracts variable names from a tuple pattern for direct Go multi-value destructuring.
///
/// Returns `Some(vec)` if all elements are simple (identifiers or wildcards),
/// `None` if any element is complex (nested tuple, struct, etc.).
///
/// - Identifiers become their name
/// - Wildcards become "_"
fn extract_simple_tuple_vars(pattern: &Pattern) -> Option<Vec<String>> {
    let Pattern::Tuple { elements, .. } = pattern else {
        return None;
    };

    let mut vars = Vec::with_capacity(elements.len());

    for element in elements {
        match element {
            Pattern::Identifier { identifier, .. } => {
                vars.push(identifier.to_string());
            }
            Pattern::WildCard { .. } => {
                vars.push("_".to_string());
            }
            _ => return None,
        }
    }

    Some(vars)
}

impl Emitter<'_> {
    pub(crate) fn emit_let(
        &mut self,
        output: &mut String,
        binding: &Binding,
        value: &Expression,
        else_block: Option<&Expression>,
        mutable: bool,
    ) {
        LetEmitter::new(self, binding, value, else_block, mutable).emit(output);
    }
}
