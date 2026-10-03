//! What evaluating a Go expression can do besides producing its value.

use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{EvaluationEffect, Stability};

/// What can change a value that an evaluation reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Reads {
    #[default]
    Nothing,
    /// Only an assignment to a named local.
    Locals,
    /// Also a call, through an alias or a reference.
    Shared,
}

impl Reads {
    pub(crate) fn of(stability: Stability) -> Self {
        match stability {
            Stability::Literal | Stability::Fixed => Self::Nothing,
            Stability::StableAcrossCalls => Self::Locals,
            Stability::Observable => Self::Shared,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Writes {
    #[default]
    Nothing,
    /// What a call can reach.
    Shared,
    Any,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    runs_code: bool,
    runs_effectful_code: bool,
    /// Outside a call, whose own panics are part of the call.
    may_panic: bool,
    may_block: bool,
    reads: Reads,
    writes: Writes,
    creates_identity: bool,
}

impl Effects {
    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            runs_code: self.runs_code || other.runs_code,
            runs_effectful_code: self.runs_effectful_code || other.runs_effectful_code,
            may_panic: self.may_panic || other.may_panic,
            may_block: self.may_block || other.may_block,
            reads: self.reads.max(other.reads),
            writes: self.writes.max(other.writes),
            creates_identity: self.creates_identity || other.creates_identity,
        }
    }

    pub(crate) fn anything() -> Self {
        Self {
            runs_code: true,
            runs_effectful_code: true,
            may_panic: true,
            may_block: true,
            reads: Reads::Shared,
            writes: Writes::Any,
            creates_identity: true,
        }
    }

    pub(crate) fn local_read() -> Self {
        Self {
            reads: Reads::Locals,
            ..Self::default()
        }
    }

    pub(crate) fn read_of(stability: Stability) -> Self {
        Self {
            reads: Reads::of(stability),
            ..Self::default()
        }
    }

    pub(crate) fn calls_of(effect: EvaluationEffect) -> Self {
        Self {
            runs_code: effect.has_call(),
            runs_effectful_code: effect.has_call(),
            writes: if effect.has_effectful_call() {
                Writes::Shared
            } else {
                Writes::Nothing
            },
            ..Self::default()
        }
    }

    pub(crate) fn with_facts(self, reads: Reads, effect: EvaluationEffect) -> Self {
        let calls = Self::calls_of(effect);
        Self {
            runs_code: self.runs_code && calls.runs_code,
            runs_effectful_code: self.runs_effectful_code && calls.runs_code,
            reads,
            writes: self.writes.min(calls.writes),
            ..self
        }
    }

    /// Go orders calls within an expression. Like the Go compiler, but not
    /// the spec, this assumes an earlier call runs before a later read.
    pub(crate) fn without_go_order(self) -> Self {
        Self {
            runs_code: false,
            runs_effectful_code: false,
            writes: Writes::Nothing,
            ..self
        }
    }

    pub(crate) fn runs_code(self) -> bool {
        self.runs_code
    }

    pub(crate) fn runs_effectful_code(self) -> bool {
        self.runs_effectful_code
    }

    pub(crate) fn panics_or_blocks(self) -> bool {
        self.may_panic || self.may_block
    }

    pub(crate) fn reads(self) -> Reads {
        self.reads
    }

    pub(crate) fn can_erase(self) -> bool {
        !self.runs_code && !self.panics_or_blocks() && self.writes == Writes::Nothing
    }

    /// A panic repeats only after the first evaluation panicked.
    pub(crate) fn can_duplicate(self) -> bool {
        !self.runs_code
            && !self.may_block
            && self.writes == Writes::Nothing
            && !self.creates_identity
    }

    /// The order of two panics is not kept.
    pub(crate) fn can_move_across(self, between: Self) -> bool {
        let orders = |first: Self, second: Self| {
            first.runs_effectful_code && (second.runs_code || second.panics_or_blocks())
        };
        !orders(self, between)
            && !orders(between, self)
            && !(self.may_block && between.may_block)
            && !observes(self.reads, between.writes)
            && !observes(between.reads, self.writes)
    }
}

fn observes(reads: Reads, writes: Writes) -> bool {
    !matches!(
        (reads, writes),
        (Reads::Nothing, _) | (_, Writes::Nothing) | (Reads::Locals, Writes::Shared)
    )
}

impl GoExpressionNode {
    /// A function literal does not run its body when evaluated.
    pub(crate) fn effects(&self) -> Effects {
        let mut effects = Effects::default();
        self.visit_children(&mut |child| effects = effects.union(child.effects()));
        if matches!(self, Self::Call { .. }) {
            effects.may_panic = false;
            effects.may_block = false;
        }
        effects.union(self.own_effects())
    }

    fn own_effects(&self) -> Effects {
        let reference_read = |may_panic| Effects {
            may_panic,
            reads: Reads::Shared,
            ..Effects::default()
        };
        match self {
            Self::Literal(_)
            | Self::Type(_)
            | Self::Empty
            | Self::Instantiation { .. }
            | Self::Spread(_)
            | Self::FunctionLiteral { .. } => Effects::default(),
            Self::Identifier(_) => Effects::local_read(),
            Self::Qualified { .. } => reference_read(false),
            Self::Call { pure: true, .. } => Effects {
                runs_code: true,
                ..Effects::default()
            },
            Self::Call { .. } | Self::Verbatim(_) => Effects {
                runs_code: true,
                runs_effectful_code: true,
                reads: Reads::Shared,
                writes: Writes::Any,
                ..Effects::default()
            },
            Self::Index { .. } | Self::Slice { .. } | Self::Dereference(_) => reference_read(true),
            Self::Selector { may_panic, .. } if *may_panic => reference_read(true),
            Self::Selector { .. } => Effects::default(),
            Self::TypeAssertion { .. } => Effects {
                may_panic: true,
                ..Effects::default()
            },
            Self::AddressOf(operand) => Effects {
                creates_identity: matches!(operand.as_ref(), Self::CompositeLiteral { .. }),
                ..Effects::default()
            },
            Self::CompositeLiteral { go_type, .. } => {
                let go_type = go_type.as_deref();
                Effects {
                    may_panic: go_type.is_some_and(|ty| ty.starts_with("map[")),
                    creates_identity: go_type
                        .is_none_or(|ty| ty.starts_with("[]") || ty.starts_with("map[")),
                    ..Effects::default()
                }
            }
            Self::Conversion { go_type, .. }
                if go_type.starts_with("*[") || go_type.starts_with('[') =>
            {
                Effects {
                    may_panic: true,
                    ..Effects::default()
                }
            }
            Self::Conversion { .. } => Effects::default(),
            Self::Unary { operator, .. } => match operator.as_str() {
                "+" | "-" | "!" | "^" | "&" => Effects::default(),
                "*" => reference_read(true),
                "<-" => Effects {
                    runs_code: true,
                    may_block: true,
                    reads: Reads::Shared,
                    writes: Writes::Any,
                    ..Effects::default()
                },
                _ => Effects::anything(),
            },
            Self::Binary {
                operator,
                left,
                right,
            } => match operator.as_str() {
                "+" | "-" | "*" | "&" | "|" | "^" | "&^" | "&&" | "||" | "<" | "<=" | ">"
                | ">=" => Effects::default(),
                // Division by zero, a negative shift, and comparing
                // uncomparable interface values panic.
                "/" | "%" | "<<" | ">>" => Effects {
                    may_panic: true,
                    ..Effects::default()
                },
                "==" | "!="
                    if !matches!(left.as_ref(), Self::Literal(_))
                        && !matches!(right.as_ref(), Self::Literal(_))
                        && !(left.is_unit_struct_literal() && right.is_unit_struct_literal()) =>
                {
                    Effects {
                        may_panic: true,
                        ..Effects::default()
                    }
                }
                "==" | "!=" => Effects::default(),
                _ => Effects::anything(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::bodies::{LoweredBlock, expression_statement};
    use crate::plan::go_expression::{CompositeLayout, FunctionLiteralLayout};
    use crate::plan::local::GoIdentifier;
    use crate::plan::values::GoExpression;

    fn name(text: &str) -> GoExpressionNode {
        GoExpressionNode::Identifier(GoIdentifier::name(text.to_string()))
    }

    fn literal(text: &str) -> GoExpressionNode {
        GoExpressionNode::Literal(text.to_string())
    }

    fn call(callee: &str, arguments: Vec<GoExpressionNode>) -> GoExpressionNode {
        GoExpressionNode::Call {
            callee: Box::new(name(callee)),
            arguments,
            pure: false,
        }
    }

    fn index() -> GoExpressionNode {
        GoExpressionNode::Index {
            base: Box::new(name("xs")),
            index: Box::new(name("i")),
        }
    }

    fn binary(left: GoExpressionNode, operator: &str, right: GoExpressionNode) -> GoExpressionNode {
        GoExpressionNode::Binary {
            operator: operator.to_string(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn each_question_has_its_own_answer() {
        let read = name("mutable").effects();
        let constant = literal("1").effects();
        let call = call("next", Vec::new()).effects();
        let index = index().effects();

        assert!(read.can_erase() && read.can_duplicate());
        assert!(read.can_move_across(constant) && read.can_move_across(index));
        assert!(!read.can_move_across(call));

        assert!(!index.can_erase() && index.can_duplicate());
        assert!(!index.can_move_across(call) && index.can_move_across(index));

        assert!(!call.can_erase() && !call.can_duplicate());
        assert!(!call.can_move_across(read) && call.can_move_across(constant));
    }

    #[test]
    fn go_orders_calls_but_not_the_reads_around_them() {
        let effectful = Effects::calls_of(EvaluationEffect::EffectfulCall);
        assert!(effectful.without_go_order().can_move_across(effectful));
        let call = call("next", Vec::new()).effects();
        assert!(!index().effects().without_go_order().can_move_across(call));
        assert!(!name("x").effects().can_move_across(call));
    }

    #[test]
    fn a_constructor_does_not_order_a_panicking_read() {
        let constructor = GoExpressionNode::Call {
            callee: Box::new(name("MakeModeClean")),
            arguments: Vec::new(),
            pure: true,
        }
        .effects();
        let read = index().effects().without_go_order();
        assert!(read.can_move_across(constructor));
        assert!(!read.can_move_across(call("next", Vec::new()).effects()));
        assert!(!constructor.can_erase() && !constructor.can_duplicate());
    }

    #[test]
    fn a_call_orders_the_panics_of_its_arguments() {
        let wrapped = call("f", vec![index()]).effects();
        assert!(!wrapped.panics_or_blocks());
        assert!(!wrapped.can_erase());
        assert!(
            binary(call("f", vec![index()]), "+", index())
                .effects()
                .panics_or_blocks()
        );
    }

    #[test]
    fn source_facts_refine_reads_and_calls() {
        let local = name("x").effects();
        let fixed = local.with_facts(Reads::Nothing, EvaluationEffect::Pure);
        let effectful = Effects::calls_of(EvaluationEffect::EffectfulCall);
        assert!(!local.can_move_across(Effects::anything()));
        assert!(fixed.can_move_across(Effects::anything().without_go_order()));
        assert!(Effects::read_of(Stability::StableAcrossCalls).can_move_across(effectful));
        assert!(!Effects::read_of(Stability::Observable).can_move_across(effectful));
    }

    #[test]
    fn panicking_operators_cannot_be_erased() {
        for operator in ["/", "%", "<<", ">>"] {
            assert!(!binary(name("a"), operator, name("b")).effects().can_erase());
        }
        assert!(binary(name("a"), "+", name("b")).effects().can_erase());
        assert!(binary(name("a"), "==", literal("1")).effects().can_erase());
        assert!(!binary(name("a"), "==", name("b")).effects().can_erase());
    }

    #[test]
    fn unknown_operators_can_do_anything() {
        let unknown = GoExpressionNode::Unary {
            operator: "~".to_string(),
            operand: Box::new(name("x")),
        };
        assert!(!unknown.effects().can_erase());
        assert!(!unknown.effects().can_duplicate());
        assert!(!binary(name("a"), "??", name("b")).effects().can_erase());
    }

    #[test]
    fn a_receive_blocks_and_cannot_repeat() {
        let receive = GoExpressionNode::Unary {
            operator: "<-".to_string(),
            operand: Box::new(name("ch")),
        }
        .effects();
        assert!(!receive.can_erase() && !receive.can_duplicate());
        assert!(!receive.can_move_across(name("x").effects()));
    }

    #[test]
    fn a_function_literal_does_not_run_its_body() {
        let function = GoExpressionNode::FunctionLiteral {
            parameters: Vec::new(),
            result: String::new(),
            body: LoweredBlock {
                statements: vec![expression_statement(GoExpression::from_node(call(
                    "effect",
                    Vec::new(),
                )))],
            },
            layout: FunctionLiteralLayout::Inline,
        };
        assert!(function.effects().can_erase());
    }

    #[test]
    fn new_slices_and_pointers_cannot_repeat() {
        let slice = GoExpressionNode::CompositeLiteral {
            go_type: Some("[]int".to_string()),
            elements: Vec::new(),
            layout: CompositeLayout::Inline { padded: true },
        };
        assert!(slice.effects().can_erase());
        assert!(!slice.effects().can_duplicate());
        let pointer = GoExpressionNode::AddressOf(Box::new(GoExpressionNode::CompositeLiteral {
            go_type: Some("Point".to_string()),
            elements: Vec::new(),
            layout: CompositeLayout::Inline { padded: true },
        }));
        assert!(!pointer.effects().can_duplicate());
        assert!(
            GoExpressionNode::AddressOf(Box::new(name("x")))
                .effects()
                .can_duplicate()
        );
    }
}
