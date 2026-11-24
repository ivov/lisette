use std::cmp::Ordering;
use std::fmt;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::ast::{BinaryOperator, BindingId, Expression, Literal, Span, UnaryOperator};
use crate::lex::{rune_codepoint, string_bytes};
use crate::program::{Definition, DotAccessResolution, is_go_import, strip_prelude_prefix};
use crate::types::{self, SimpleKind, Type};

pub const MAX_CONSTANT_SHIFT: u32 = 1074;

const MAX_INTEGER_BITS: u64 = 512;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Value {
    Bool(bool),
    String(Vec<u8>),
    Integer(BigInt),
    Rune(BigInt),
    Float(BigRational),
    Complex(BigRational, BigRational),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub value: Value,
    pub go_type: Option<SimpleKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstantError {
    DivisionByZero {
        span: Span,
    },
    Overflow {
        span: Span,
        value: Box<Value>,
        kind: SimpleKind,
        in_conversion: bool,
    },
    Truncated {
        span: Span,
    },
    ShiftCount {
        span: Span,
    },
    TooLarge {
        span: Span,
    },
}

pub type Evaluation = Result<Option<Constant>, ConstantError>;

pub trait ConstantSource {
    fn definition(&self, qualified_name: &str) -> Option<&Definition>;

    fn local_constant(&self, _binding: BindingId) -> Option<Constant> {
        None
    }

    fn named_constant(&self, qualified_name: &str) -> Option<Value> {
        let definition = self.definition(qualified_name)?;
        let value = definition.const_value()?.value()?;
        Some(match self.underlying_kind(&definition.ty) {
            Some(kind) => represent(value.clone(), kind, Span::dummy()).unwrap_or(value),
            None => value,
        })
    }

    fn underlying_kind(&self, ty: &Type) -> Option<SimpleKind> {
        types::underlying_simple_kind(ty, |id| self.definition(id))
    }

    fn array_length(&self, ty: &Type) -> Option<u64> {
        let lookup = |id: &str| self.definition(id);
        types::peel_underlying(&types::receiver_shape(ty, lookup), lookup).array_len()
    }

    fn is_newtype(&self, qualified_name: &str) -> bool {
        self.definition(qualified_name)
            .is_some_and(Definition::is_newtype)
    }
}

pub fn local_constant(
    initializer: &Expression,
    ty: &Type,
    source: &dyn ConstantSource,
) -> Option<Constant> {
    let value = value_of(initializer, source)?;
    let kind = source.underlying_kind(ty);
    let value = match kind {
        Some(kind) => represent(value, kind, initializer.get_span()).ok()?,
        None => value,
    };
    let go_type = kind.filter(|kind| !matches!(kind, SimpleKind::String | SimpleKind::Bool));
    Some(Constant { value, go_type })
}

pub struct Literals;

impl ConstantSource for Literals {
    fn definition(&self, _qualified_name: &str) -> Option<&Definition> {
        None
    }
}

macro_rules! constant {
    ($evaluation:expr) => {
        match $evaluation? {
            Some(constant) => constant,
            None => return Ok(None),
        }
    };
}

pub fn evaluate(expression: &Expression, source: &dyn ConstantSource) -> Evaluation {
    let span = expression.get_span();
    match expression {
        Expression::Paren { expression, .. } => evaluate(expression, source),
        Expression::Literal { literal, span, .. } => match literal_value(literal) {
            Some(value) => fits(value, *span).map(|value| Some(untyped(value))),
            None => Ok(None),
        },
        Expression::Identifier { resolution, ty, .. } => {
            if let Some(binding) = resolution.binding_id() {
                return Ok(source.local_constant(binding));
            }
            Ok(resolution
                .definition()
                .and_then(|name| named(name, ty, source)))
        }
        Expression::DotAccess {
            expression: receiver,
            resolution: DotAccessResolution::TupleStructField { is_newtype: true },
            ty,
            ..
        } => convert_to(receiver, ty, span, source),
        Expression::DotAccess { resolution, ty, .. } => Ok(resolution
            .definition()
            .and_then(|name| named(name, ty, source))),
        Expression::Unary {
            operator,
            expression,
            ..
        } => {
            let operand = constant!(evaluate(expression, source));
            unary(operator, operand, span)
        }
        Expression::Binary {
            operator,
            left,
            right,
            ..
        } => {
            let left = constant!(evaluate(left, source));
            let right = constant!(evaluate(right, source));
            binary(*operator, left, right, span).map(Some)
        }
        Expression::Cast {
            expression: operand,
            ty,
            ..
        } => convert_to(operand, ty, span, source),
        Expression::Call { .. } => evaluate_call(expression, source),
        _ => Ok(None),
    }
}

fn convert_to(
    operand: &Expression,
    ty: &Type,
    span: Span,
    source: &dyn ConstantSource,
) -> Evaluation {
    let operand = constant!(evaluate(operand, source));
    let Some(kind) = source.underlying_kind(ty) else {
        return Ok(None);
    };
    convert(operand, kind, span).map(|constant| constant.map(typed_as(kind)))
}

pub fn value_of(expression: &Expression, source: &dyn ConstantSource) -> Option<Value> {
    evaluate(expression, source)
        .ok()
        .flatten()
        .map(|constant| constant.value)
}

fn untyped(value: Value) -> Constant {
    Constant {
        value,
        go_type: None,
    }
}

fn typed_as(kind: SimpleKind) -> impl Fn(Value) -> Constant {
    move |value| Constant {
        value,
        go_type: Some(kind),
    }
}

fn named(name: &str, ty: &Type, source: &dyn ConstantSource) -> Option<Constant> {
    let value = source.named_constant(name)?;
    let typed = if is_go_import(name) {
        matches!(ty.unwrap_forall(), Type::Nominal { .. }) || converts_go_constant(&value, ty)
    } else {
        true
    };
    let go_type = source
        .underlying_kind(ty)
        .filter(|kind| typed && kind.is_arithmetic());
    Some(Constant { value, go_type })
}

pub fn converts_go_constant(value: &Value, ty: &Type) -> bool {
    ty.is_unsigned_int()
        && matches!(value, Value::Integer(integer) if *integer > BigInt::from(i64::MAX))
}

pub fn builtin_length_receiver(call: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        spread: None,
        ..
    } = call
    else {
        return None;
    };
    match callee.unwrap_parens() {
        Expression::DotAccess {
            expression: receiver,
            resolution:
                DotAccessResolution::InstanceMethod {
                    definition: Some(definition),
                    ..
                },
            ..
        } if args.is_empty()
            && matches!(
                definition.as_str(),
                "prelude.string.length" | "prelude.Array.length"
            ) =>
        {
            Some(receiver)
        }
        _ => None,
    }
}

fn evaluate_call(expression: &Expression, source: &dyn ConstantSource) -> Evaluation {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        type_arguments,
        ty,
        span,
        ..
    } = expression
    else {
        return Ok(None);
    };
    if spread.is_some() {
        return Ok(None);
    }
    if let Some(receiver) = builtin_length_receiver(expression) {
        return length(receiver, source);
    }
    let callee = match callee.unwrap_parens() {
        Expression::Identifier { resolution, .. } => resolution.definition(),
        Expression::DotAccess { resolution, .. } => resolution.definition(),
        _ => None,
    };
    let Some(callee) = callee else {
        return Ok(None);
    };
    if source.is_newtype(callee) {
        let [argument] = args.as_slice() else {
            return Ok(None);
        };
        return convert_to(argument, ty, *span, source);
    }
    let ordering = match strip_prelude_prefix(callee) {
        Some("min") => Ordering::Less,
        Some("max") => Ordering::Greater,
        Some("zero") => return Ok(zero_value(ty, source)),
        Some("real" | "imaginary") => {
            let [argument] = args.as_slice() else {
                return Ok(None);
            };
            let argument = constant!(evaluate(argument, source));
            let (real, imaginary) = match raise(argument.value, NumericKind::Complex) {
                Value::Complex(real, imaginary) => (real, imaginary),
                _ => return Ok(None),
            };
            let part = if callee == "prelude.real" {
                real
            } else {
                imaginary
            };
            let go_type = argument.go_type.map(|kind| match kind {
                SimpleKind::Complex64 => SimpleKind::Float32,
                _ => SimpleKind::Float64,
            });
            return finish(Value::Float(part), go_type, *span).map(Some);
        }
        Some("complex") => {
            let [real, imaginary] = args.as_slice() else {
                return Ok(None);
            };
            let real = constant!(evaluate(real, source));
            let imaginary = constant!(evaluate(imaginary, source));
            let go_type = real.go_type.or(imaginary.go_type).map(|kind| match kind {
                SimpleKind::Float32 => SimpleKind::Complex64,
                _ => SimpleKind::Complex128,
            });
            let (Some(real), Some(imaginary)) = (rational(&real.value), rational(&imaginary.value))
            else {
                return Ok(None);
            };
            return finish(Value::Complex(real, imaginary), go_type, *span).map(Some);
        }
        _ => return Ok(None),
    };
    let mut arguments = args.iter();
    let Some(first) = arguments.next() else {
        return Ok(None);
    };
    let mut chosen = constant!(evaluate(first, source));
    for argument in arguments {
        let candidate = constant!(evaluate(argument, source));
        let Some(next) = choose(chosen, candidate, ordering, *span)? else {
            return Ok(None);
        };
        chosen = next;
    }
    if !type_arguments.is_empty()
        && let Some(kind) = source.underlying_kind(ty)
    {
        return finish(chosen.value, Some(kind), *span).map(Some);
    }
    Ok(Some(chosen))
}

fn zero_value(ty: &Type, source: &dyn ConstantSource) -> Option<Constant> {
    let kind = source.underlying_kind(ty)?;
    let value = match kind {
        SimpleKind::Bool => Value::Bool(false),
        SimpleKind::String => Value::String(Vec::new()),
        SimpleKind::Rune => Value::Rune(BigInt::zero()),
        kind if kind.integer_range().is_some() => Value::Integer(BigInt::zero()),
        SimpleKind::Float32 | SimpleKind::Float64 => Value::Float(BigRational::zero()),
        SimpleKind::Complex64 | SimpleKind::Complex128 => {
            Value::Complex(BigRational::zero(), BigRational::zero())
        }
        _ => return None,
    };
    Some(typed_as(kind)(value))
}

fn length(receiver: &Expression, source: &dyn ConstantSource) -> Evaluation {
    let typed_int = typed_as(SimpleKind::Int);
    if let Some(length) = source.array_length(&receiver.get_type()) {
        return Ok(Some(typed_int(Value::Integer(BigInt::from(length)))));
    }
    let receiver = constant!(evaluate(receiver, source));
    Ok(match receiver.value {
        Value::String(bytes) => Some(typed_int(Value::Integer(BigInt::from(bytes.len())))),
        _ => None,
    })
}

fn choose(current: Constant, candidate: Constant, ordering: Ordering, span: Span) -> Evaluation {
    let (go_type, left, right) = align(current, candidate, span)?;
    let value = match compare(&left, &right) {
        Some(found) if found == ordering => left,
        Some(_) => right,
        None => return Ok(None),
    };
    Ok(Some(Constant { value, go_type }))
}

fn align(
    left: Constant,
    right: Constant,
    span: Span,
) -> Result<(Option<SimpleKind>, Value, Value), ConstantError> {
    let go_type = left.go_type.or(right.go_type);
    let (left, right) = match go_type {
        Some(kind) => (
            represent(left.value, kind, span)?,
            represent(right.value, kind, span)?,
        ),
        None => promote(left.value, right.value),
    };
    Ok((go_type, left, right))
}

fn unary(operator: &UnaryOperator, operand: Constant, span: Span) -> Evaluation {
    let value = match (operator, operand.value) {
        (UnaryOperator::Negative, Value::Integer(value)) => Value::Integer(-value),
        (UnaryOperator::Negative, Value::Rune(value)) => Value::Rune(-value),
        (UnaryOperator::Negative, Value::Float(value)) => Value::Float(-value),
        (UnaryOperator::Negative, Value::Complex(real, imaginary)) => {
            Value::Complex(-real, -imaginary)
        }
        (UnaryOperator::Not, Value::Bool(value)) => Value::Bool(!value),
        (UnaryOperator::BitwiseNot, Value::Integer(value) | Value::Rune(value)) => {
            Value::Integer(complement(value, operand.go_type))
        }
        _ => return Ok(None),
    };
    finish(value, operand.go_type, span).map(Some)
}

fn complement(value: BigInt, go_type: Option<SimpleKind>) -> BigInt {
    match go_type
        .filter(|kind| kind.is_unsigned())
        .and_then(SimpleKind::integer_range)
    {
        Some((_, max)) => BigInt::from(max) ^ value,
        None => -value - 1,
    }
}

fn binary(
    operator: BinaryOperator,
    left: Constant,
    right: Constant,
    span: Span,
) -> Result<Constant, ConstantError> {
    use BinaryOperator::*;
    if matches!(operator, ShiftLeft | ShiftRight) {
        return shift(operator, left, right, span);
    }
    let (go_type, left, right) = align(left, right, span)?;
    let comparison = |accepts: fn(Ordering) -> bool| -> Result<Constant, ConstantError> {
        let ordering = compare(&left, &right).ok_or(ConstantError::Truncated { span })?;
        Ok(untyped(Value::Bool(accepts(ordering))))
    };
    match operator {
        Equal => return Ok(untyped(Value::Bool(left == right))),
        NotEqual => return Ok(untyped(Value::Bool(left != right))),
        LessThan => return comparison(Ordering::is_lt),
        LessThanOrEqual => return comparison(Ordering::is_le),
        GreaterThan => return comparison(Ordering::is_gt),
        GreaterThanOrEqual => return comparison(Ordering::is_ge),
        _ => {}
    }
    let value = arithmetic(operator, left, right, span)?;
    finish(fits(value, span)?, go_type, span)
}

fn arithmetic(
    operator: BinaryOperator,
    left: Value,
    right: Value,
    span: Span,
) -> Result<Value, ConstantError> {
    use BinaryOperator::*;
    let division_by_zero = ConstantError::DivisionByZero { span };
    Ok(match (operator, left, right) {
        (And, Value::Bool(left), Value::Bool(right)) => Value::Bool(left && right),
        (Or, Value::Bool(left), Value::Bool(right)) => Value::Bool(left || right),
        (Addition, Value::String(mut left), Value::String(right)) => {
            left.extend(right);
            Value::String(left)
        }
        (_, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(integer_arithmetic(operator, left, right, span)?)
        }
        (_, Value::Rune(left), Value::Rune(right)) => {
            Value::Rune(integer_arithmetic(operator, left, right, span)?)
        }
        (_, Value::Float(left), Value::Float(right)) => match operator {
            Addition => Value::Float(left + right),
            Subtraction => Value::Float(left - right),
            Multiplication => Value::Float(left * right),
            Division if right.is_zero() => return Err(division_by_zero),
            Division => Value::Float(left / right),
            _ => return Err(ConstantError::Truncated { span }),
        },
        (_, Value::Complex(a, b), Value::Complex(c, d)) => match operator {
            Addition => Value::Complex(a + c, b + d),
            Subtraction => Value::Complex(a - c, b - d),
            Multiplication => Value::Complex(&a * &c - &b * &d, &a * &d + &b * &c),
            Division => {
                let denominator = &c * &c + &d * &d;
                if denominator.is_zero() {
                    return Err(division_by_zero);
                }
                Value::Complex(
                    (&a * &c + &b * &d) / &denominator,
                    (&b * &c - &a * &d) / &denominator,
                )
            }
            _ => return Err(ConstantError::Truncated { span }),
        },
        _ => return Err(ConstantError::Truncated { span }),
    })
}

fn integer_arithmetic(
    operator: BinaryOperator,
    left: BigInt,
    right: BigInt,
    span: Span,
) -> Result<BigInt, ConstantError> {
    use BinaryOperator::*;
    match operator {
        Division | Remainder if right.is_zero() => Err(ConstantError::DivisionByZero { span }),
        Addition => Ok(left + right),
        Subtraction => Ok(left - right),
        Multiplication => Ok(left * right),
        Division => Ok(left / right),
        Remainder => Ok(left % right),
        BitwiseAnd => Ok(left & right),
        BitwiseOr => Ok(left | right),
        BitwiseXor => Ok(left ^ right),
        BitwiseAndNot => Ok(left & (-right - 1)),
        _ => Err(ConstantError::Truncated { span }),
    }
}

fn shift(
    operator: BinaryOperator,
    left: Constant,
    right: Constant,
    span: Span,
) -> Result<Constant, ConstantError> {
    let count = match right.value {
        Value::Integer(count) | Value::Rune(count) => count,
        Value::Float(count) => float_integer(&count, span)?,
        _ => return Err(ConstantError::Truncated { span }),
    };
    let Some(amount) = count
        .to_u32()
        .filter(|amount| *amount <= MAX_CONSTANT_SHIFT)
    else {
        return Err(ConstantError::ShiftCount { span });
    };
    let (shifted, rune) = match left.value {
        Value::Integer(value) => (value, false),
        Value::Rune(value) => (value, true),
        Value::Float(value) => (float_integer(&value, span)?, false),
        _ => return Err(ConstantError::Truncated { span }),
    };
    let shifted = if operator == BinaryOperator::ShiftLeft {
        shifted << amount
    } else {
        shifted >> amount
    };
    let value = if rune {
        Value::Rune(shifted)
    } else {
        Value::Integer(shifted)
    };
    finish(fits(value, span)?, left.go_type, span)
}

fn fits(value: Value, span: Span) -> Result<Value, ConstantError> {
    match &value {
        Value::Integer(integer) | Value::Rune(integer) if integer.bits() > MAX_INTEGER_BITS => {
            Err(ConstantError::TooLarge { span })
        }
        _ => Ok(value),
    }
}

fn float_integer(value: &BigRational, span: Span) -> Result<BigInt, ConstantError> {
    if !value.is_integer() {
        return Err(ConstantError::Truncated { span });
    }
    Ok(value.to_integer())
}

fn finish(
    value: Value,
    go_type: Option<SimpleKind>,
    span: Span,
) -> Result<Constant, ConstantError> {
    match go_type {
        Some(kind) => Ok(Constant {
            value: represent(value, kind, span)?,
            go_type,
        }),
        None => Ok(untyped(value)),
    }
}

fn convert(
    operand: Constant,
    kind: SimpleKind,
    span: Span,
) -> Result<Option<Value>, ConstantError> {
    let value = match (operand.value, kind) {
        (Value::Integer(value) | Value::Rune(value), SimpleKind::String) => {
            let character = value
                .to_u32()
                .and_then(char::from_u32)
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            Value::String(character.to_string().into_bytes())
        }
        (value @ Value::String(_), SimpleKind::String)
        | (value @ Value::Bool(_), SimpleKind::Bool) => value,
        (value, kind) if kind.is_arithmetic() && is_numeric_value(&value) => {
            return represent(value, kind, span)
                .map(Some)
                .map_err(|error| match error {
                    ConstantError::Overflow {
                        span, value, kind, ..
                    } => ConstantError::Overflow {
                        span,
                        value,
                        kind,
                        in_conversion: true,
                    },
                    error => error,
                });
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub fn represent(value: Value, kind: SimpleKind, span: Span) -> Result<Value, ConstantError> {
    let overflow = |value: Value| ConstantError::Overflow {
        span,
        value: Box::new(value),
        kind,
        in_conversion: false,
    };
    if let Some((min, max)) = kind.integer_range() {
        let integer = match value {
            Value::Integer(value) | Value::Rune(value) => value,
            Value::Float(real) if real.is_integer() => real.to_integer(),
            Value::Complex(real, imaginary) if imaginary.is_zero() && real.is_integer() => {
                real.to_integer()
            }
            Value::Float(_) | Value::Complex(..) => return Err(ConstantError::Truncated { span }),
            _ => return Err(overflow(value)),
        };
        if integer < BigInt::from(min) || integer > BigInt::from(max) {
            return Err(overflow(Value::Integer(integer)));
        }
        return Ok(if kind == SimpleKind::Rune {
            Value::Rune(integer)
        } else {
            Value::Integer(integer)
        });
    }
    let single = matches!(kind, SimpleKind::Float32 | SimpleKind::Complex64);
    match kind {
        SimpleKind::Float32 | SimpleKind::Float64 => {
            let real = match value {
                Value::Complex(real, imaginary) if imaginary.is_zero() => real,
                Value::Complex(..) => return Err(ConstantError::Truncated { span }),
                value => rational(&value).ok_or_else(|| overflow(value))?,
            };
            let rounded = round_float(&real, single).ok_or_else(|| overflow(Value::Float(real)))?;
            Ok(Value::Float(rounded))
        }
        SimpleKind::Complex64 | SimpleKind::Complex128 => {
            let (real, imaginary) = match value {
                Value::Complex(real, imaginary) => (real, imaginary),
                value => (
                    rational(&value).ok_or_else(|| overflow(value))?,
                    BigRational::zero(),
                ),
            };
            match (round_float(&real, single), round_float(&imaginary, single)) {
                (Some(real), Some(imaginary)) => Ok(Value::Complex(real, imaginary)),
                _ => Err(overflow(Value::Complex(real, imaginary))),
            }
        }
        SimpleKind::String if matches!(value, Value::String(_)) => Ok(value),
        SimpleKind::Bool if matches!(value, Value::Bool(_)) => Ok(value),
        _ => Err(overflow(value)),
    }
}

fn round_float(value: &BigRational, single: bool) -> Option<BigRational> {
    let (precision, min_exponent, max) = if single {
        (24, -149, f64::from(f32::MAX))
    } else {
        (53, -1074, f64::MAX)
    };
    if value.is_zero() {
        return Some(BigRational::zero());
    }
    let magnitude = value.abs();
    let mut exponent = magnitude.numer().bits() as i64 - magnitude.denom().bits() as i64;
    if magnitude < power_of_two(exponent) {
        exponent -= 1;
    }
    let scale = power_of_two((exponent - precision + 1).max(min_exponent));
    let scaled = &magnitude / &scale;
    let mut mantissa = scaled.to_integer();
    let remainder = scaled.fract() * BigInt::from(2);
    if remainder > BigRational::one() || (remainder.is_one() && mantissa.bit(0)) {
        mantissa += 1;
    }
    let rounded = BigRational::from_integer(mantissa) * scale;
    if rounded > BigRational::from_float(max)? {
        return None;
    }
    Some(if value.is_negative() {
        -rounded
    } else {
        rounded
    })
}

fn power_of_two(exponent: i64) -> BigRational {
    let power = BigRational::from_integer(BigInt::one() << exponent.unsigned_abs());
    if exponent < 0 { power.recip() } else { power }
}

fn rational(value: &Value) -> Option<BigRational> {
    match value {
        Value::Integer(value) | Value::Rune(value) => {
            Some(BigRational::from_integer(value.clone()))
        }
        Value::Float(value) => Some(value.clone()),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NumericKind {
    None,
    Integer,
    Rune,
    Float,
    Complex,
}

fn numeric_kind(value: &Value) -> NumericKind {
    match value {
        Value::Integer(_) => NumericKind::Integer,
        Value::Rune(_) => NumericKind::Rune,
        Value::Float(_) => NumericKind::Float,
        Value::Complex(..) => NumericKind::Complex,
        Value::Bool(_) | Value::String(_) => NumericKind::None,
    }
}

fn promote(left: Value, right: Value) -> (Value, Value) {
    let target = numeric_kind(&left).max(numeric_kind(&right));
    (raise(left, target), raise(right, target))
}

fn raise(value: Value, target: NumericKind) -> Value {
    match (value, target) {
        (Value::Integer(value), NumericKind::Rune) => Value::Rune(value),
        (Value::Integer(value) | Value::Rune(value), NumericKind::Float) => {
            Value::Float(BigRational::from_integer(value))
        }
        (Value::Integer(value) | Value::Rune(value), NumericKind::Complex) => {
            Value::Complex(BigRational::from_integer(value), BigRational::zero())
        }
        (Value::Float(value), NumericKind::Complex) => Value::Complex(value, BigRational::zero()),
        (value, _) => value,
    }
}

fn compare(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) | (Value::Rune(left), Value::Rune(right)) => {
            Some(left.cmp(right))
        }
        (Value::Float(left), Value::Float(right)) => Some(left.cmp(right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn is_numeric_value(value: &Value) -> bool {
    !matches!(value, Value::Bool(_) | Value::String(_))
}

pub fn literal_value(literal: &Literal) -> Option<Value> {
    Some(match literal {
        Literal::Integer { value, text } => {
            Value::Integer(integer_literal(*value, text.as_deref())?)
        }
        Literal::Float { value, text } => Value::Float(
            decimal(text.as_deref().unwrap_or(&value.to_string()))
                .or_else(|| BigRational::from_float(*value))?,
        ),
        Literal::Imaginary(coefficient) => Value::Complex(
            BigRational::zero(),
            decimal(&coefficient.to_string()).or_else(|| BigRational::from_float(*coefficient))?,
        ),
        Literal::Boolean(value) => Value::Bool(*value),
        Literal::String { value, raw } => Value::String(string_bytes(value, *raw)?.into_owned()),
        Literal::Char(text) => Value::Rune(rune_codepoint(text)?.into()),
        Literal::FormatString(_) | Literal::Slice(_) => return None,
    })
}

pub fn exceeds_u64(text: &str) -> bool {
    integer_literal(0, Some(text)).is_none_or(|value| value > BigInt::from(u64::MAX))
}

fn integer_literal(value: u64, text: Option<&str>) -> Option<BigInt> {
    let Some(text) = text else {
        return Some(value.into());
    };
    let text = text.replace('_', "");
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.as_str()),
    };
    let (radix, digits) = match digits.get(..2) {
        Some("0x" | "0X") => (16, &digits[2..]),
        Some("0o" | "0O") => (8, &digits[2..]),
        Some("0b" | "0B") => (2, &digits[2..]),
        _ => (10, digits),
    };
    let magnitude = match u64::from_str_radix(digits, radix) {
        Ok(small) => BigInt::from(small),
        Err(_) => BigInt::parse_bytes(digits.as_bytes(), radix)?,
    };
    Some(if negative { -magnitude } else { magnitude })
}

fn decimal(text: &str) -> Option<BigRational> {
    let text = text.replace('_', "");
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], text[at + 1..].parse::<i64>().ok()?),
        None => (text, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let significand = BigInt::parse_bytes(digits.as_bytes(), 10)?;
    let scale = exponent - fraction.len() as i64;
    let power = num_traits::pow(
        BigInt::from(10),
        usize::try_from(scale.unsigned_abs()).ok()?,
    );
    let magnitude = if scale < 0 {
        BigRational::new(significand, power)
    } else {
        BigRational::from_integer(significand * power)
    };
    Some(if negative { -magnitude } else { magnitude })
}

impl Value {
    pub fn from_i128(value: i128) -> Self {
        Value::Integer(value.into())
    }

    pub fn from_f64(value: f64) -> Option<Self> {
        BigRational::from_float(value).map(Value::Float)
    }

    pub fn as_integer(&self) -> Option<i128> {
        match self {
            Value::Integer(value) | Value::Rune(value) => value.to_i128(),
            _ => None,
        }
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Value::Integer(value) | Value::Rune(value) => value.is_zero(),
            Value::Float(value) => value.is_zero(),
            Value::Complex(real, imaginary) => real.is_zero() && imaginary.is_zero(),
            Value::Bool(_) | Value::String(_) => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let float = |value: &BigRational| match value.to_f64().filter(|value| value.is_finite()) {
            Some(approximate)
                if approximate != 0.0 && !(1e-6..1e21).contains(&approximate.abs()) =>
            {
                format!("{approximate:e}")
            }
            Some(approximate) => approximate.to_string(),
            None => value.to_integer().to_string(),
        };
        match self {
            Value::Bool(value) => write!(f, "{value}"),
            Value::String(bytes) => write!(f, "{:?}", String::from_utf8_lossy(bytes)),
            Value::Integer(value) | Value::Rune(value) => write!(f, "{value}"),
            Value::Float(value) => write!(f, "{}", float(value)),
            Value::Complex(real, imaginary) => {
                write!(f, "({} + {}i)", float(real), float(imaginary))
            }
        }
    }
}
