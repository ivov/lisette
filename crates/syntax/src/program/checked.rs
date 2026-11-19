use rustc_hash::FxHashMap as HashMap;
use std::fmt::Debug;
use std::hash::Hash;

use super::{ConformanceWitness, NativeOperation, NativeTypeKind, ReceiverCoercion};
use crate::EcoString;
use crate::ast::{Expression, ExpressionId, Span};
use crate::types::{SubstitutionMap, Symbol, Type};

/// Names one value use: the occurrence that produces the value, and its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueUseKey {
    expression: ExpressionId,
    role: ValueUseRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ValueUseRole {
    Value,
    PropagatedError,
    CastOperand,
}

impl ValueUseKey {
    /// The value of `expression` where it goes into its slot.
    pub fn value(expression: &Expression) -> Self {
        Self {
            expression: expression.id(),
            role: ValueUseRole::Value,
        }
    }

    /// The error that the `?` applied to the occurrence `operand` returns from its function or `try` block.
    pub fn propagated_error(operand: ExpressionId) -> Self {
        Self {
            expression: operand,
            role: ValueUseRole::PropagatedError,
        }
    }

    /// The occurrence that produces the value.
    pub fn expression(&self) -> ExpressionId {
        self.expression
    }

    /// The operand of the cast with the occurrence `cast`, converted to the cast type.
    pub fn cast_operand(cast: ExpressionId) -> Self {
        Self {
            expression: cast,
            role: ValueUseRole::CastOperand,
        }
    }
}

/// How a value of its source type becomes a value of its slot type.
#[derive(Debug, Clone)]
pub enum Adaptation {
    Identity,
    /// The children that produce the value adapt it to the slot type.
    Delegated {
        /// Whether a child that produces the value changes its type.
        converts: bool,
    },
    Interface(InterfaceConversion),
    /// A Go interface value where an `Option` is expected.
    ImplicitSome {
        payload: Box<Adaptation>,
    },
    Tuple(Vec<Adaptation>),
    Array(Box<Adaptation>),
    /// A method receiver that takes its address or reads through its reference.
    Receiver(ReceiverCoercion),
}

impl Adaptation {
    /// Whether the adapted value has another type than the value before adaptation.
    pub fn converts(&self) -> bool {
        match self {
            Self::Identity => false,
            Self::Delegated { converts } => *converts,
            Self::Interface(_)
            | Self::ImplicitSome { .. }
            | Self::Tuple(_)
            | Self::Array(_)
            | Self::Receiver(_) => true,
        }
    }
}

/// A value converted to an interface type, with the conformance the checker selected.
#[derive(Debug, Clone)]
pub struct InterfaceConversion {
    pub source: Type,
    pub target: Type,
    /// Absent when the target requires no method, or when the source is no named type or type parameter.
    pub witness: Option<ConformanceWitness>,
}

/// The slot type of one value use, and the adaptation into it.
#[derive(Debug, Clone)]
pub struct CheckedValueUse {
    /// The type of the value before its adaptation.
    pub value_type: Type,
    pub slot: Type,
    pub adaptation: Adaptation,
    /// Where the value is written, for diagnostics.
    pub span: Span,
}

/// What a callee runs and with which type arguments, as the checker selected it.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedCallee {
    pub operation: CallOperation,
    /// The declaration that the callee names, with an aliased owner type replaced by its target.
    pub definition: Option<Symbol>,
    /// The solved type arguments of a generic callee, in the order of its type parameters.
    pub type_arguments: SubstitutionMap,
    /// Whether the call must write into storage that no other name sees, as slice growth must.
    pub copies_storage: bool,
    /// Whether the callee holds the receiver that the first parameter of its declaration takes, as `x.m(a)` does.
    pub binds_receiver: bool,
    /// Where the callee is written, for diagnostics.
    pub span: Span,
}

impl CheckedCallee {
    /// The built-in method this callee runs, if any.
    pub fn native(&self) -> Option<NativeOperation<'_>> {
        match &self.operation {
            CallOperation::Native { kind, method } => Some(NativeOperation::new(*kind, method)),
            _ => None,
        }
    }

    /// The index of the declared parameter that the first argument of the call fills.
    pub fn receiver_offset(&self) -> usize {
        usize::from(self.binds_receiver)
    }

    /// The type arguments that a call spells, which are the last `written` of the callee.
    pub fn written_type_arguments(&self, written: usize) -> impl Iterator<Item = &Type> + Clone {
        let types = self.type_arguments.types();
        let skipped = types.len().saturating_sub(written);
        types.skip(skipped)
    }

    /// The type argument of a callee whose first type parameter decides its result.
    pub fn first_type_argument(&self) -> &Type {
        self.type_arguments
            .types()
            .next()
            .expect("internal error: the checker records the type argument of this callee")
    }
}

/// The function, method or built-in operation that a callee selects.
#[derive(Debug, Clone, PartialEq)]
pub enum CallOperation {
    /// A function value, such as a local, a field, a closure or a declared function.
    Function,
    /// A method that lowers to a free function, called on its receiver.
    UfcsFunction,
    /// A receiver method called through its type, with the receiver as the first argument.
    ReceiverMethod {
        is_public: bool,
    },
    /// A method that a type parameter takes from the bound that declares it.
    BoundMethod {
        bound: Type,
        method: EcoString,
    },
    /// A method of a built-in type.
    Native {
        kind: NativeTypeKind,
        method: EcoString,
    },
    /// A constructor of a built-in type.
    NativeConstructor {
        kind: NativeTypeKind,
        method: EcoString,
    },
    TupleStructConstructor,
    AssertType,
    Zero,
    /// No operation, because inference rejected the call before it could select one.
    Unavailable,
}

/// The operand of a binary expression that converts to the type of the other operand, as the checker accepted it.
///
/// The checker accepts a numeric alias with a plain number of the same numeric family, and the plain operand converts to the alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandConversion {
    Left,
    Right,
}

/// How a program tests two values of one type for equality, as the checker selected it.
#[derive(Debug, Clone, PartialEq)]
pub enum EqualityRecipe {
    /// Go `==`.
    Operator,
    /// The `equals` method of `owner`, or of a type parameter whose bound declares it when `owner` is absent.
    Method { owner: Option<Symbol> },
    /// `slices.Equal`, or `slices.EqualFunc` with the equality of the elements when they need one.
    Slice(Option<Box<ElementEquality>>),
    /// `maps.Equal`, or `maps.EqualFunc` with the equality of the values when they need one.
    Map(Option<Box<ElementEquality>>),
}

/// The equality of the elements of a container, which Go `==` cannot test.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementEquality {
    pub ty: Type,
    pub recipe: EqualityRecipe,
}

impl EqualityRecipe {
    /// The types whose `equals` method this equality calls, at any depth.
    pub fn owners(&self) -> Vec<&Symbol> {
        match self {
            Self::Operator | Self::Method { owner: None } => Vec::new(),
            Self::Method { owner: Some(owner) } => vec![owner],
            Self::Slice(element) | Self::Map(element) => element
                .as_ref()
                .map(|element| element.recipe.owners())
                .unwrap_or_default(),
        }
    }
}

/// The records that the checker published for the bodies it checked, which no consumer changes.
#[derive(Debug, Clone, Default)]
pub struct CheckedBodies {
    uses: HashMap<ValueUseKey, CheckedValueUse>,
    callees: HashMap<ValueUseKey, CheckedCallee>,
    /// Keyed by the callee of a built-in operation that compares elements, or by an `assert` of an `equals` call.
    equalities: HashMap<ExpressionId, EqualityRecipe>,
    /// The unit value of each `if let` with no `else`, keyed by the `if let`.
    implicit_units: HashMap<ExpressionId, ExpressionId>,
    /// Keyed by the binary expression whose operand converts.
    operand_conversions: HashMap<ExpressionId, OperandConversion>,
}

impl CheckedBodies {
    pub fn value_use(&self, key: &ValueUseKey) -> Option<&CheckedValueUse> {
        self.uses.get(key)
    }

    pub fn callee(&self, key: &ValueUseKey) -> Option<&CheckedCallee> {
        self.callees.get(key)
    }

    pub fn equality(&self, expression: ExpressionId) -> Option<&EqualityRecipe> {
        self.equalities.get(&expression)
    }

    pub fn callees(&self) -> impl Iterator<Item = (&ValueUseKey, &CheckedCallee)> {
        self.callees.iter()
    }

    /// A lookup of the records for the occurrence `expression`, which does not check that `expression` belongs to these bodies.
    pub fn occurrence<'e>(&self, expression: &'e Expression) -> CheckedExpression<'e, '_> {
        CheckedExpression {
            expression,
            bodies: self,
        }
    }
}

/// The records that the checker collects while it checks bodies, which only it changes before it publishes them.
#[derive(Debug, Clone, Default)]
pub struct CheckedBodiesBuilder {
    records: CheckedBodies,
}

impl CheckedBodiesBuilder {
    /// Record `value_use` for `key`, which no other record names.
    pub fn insert_use(&mut self, key: ValueUseKey, value_use: CheckedValueUse) {
        insert_unique(&mut self.records.uses, key, value_use);
    }

    /// Record `callee` for `key`, which no other callee names.
    pub fn insert_callee(&mut self, key: ValueUseKey, callee: CheckedCallee) {
        insert_unique(&mut self.records.callees, key, callee);
    }

    /// Record the equality that the comparison `expression` tests with.
    pub fn insert_equality(&mut self, expression: ExpressionId, recipe: EqualityRecipe) {
        insert_unique(&mut self.records.equalities, expression, recipe);
    }

    /// Record `unit` as the value that the `if let` with no `else` `if_let` produces when its pattern does not match.
    pub fn insert_implicit_unit(&mut self, if_let: ExpressionId, unit: ExpressionId) {
        insert_unique(&mut self.records.implicit_units, if_let, unit);
    }

    /// Record that the operand `conversion` of the binary expression `binary` converts to the type of the other operand.
    pub fn insert_operand_conversion(
        &mut self,
        binary: ExpressionId,
        conversion: OperandConversion,
    ) {
        insert_unique(&mut self.records.operand_conversions, binary, conversion);
    }

    pub fn merge(&mut self, other: Self) {
        let CheckedBodies {
            uses,
            callees,
            equalities,
            implicit_units,
            operand_conversions,
        } = other.records;
        for (key, value_use) in uses {
            self.insert_use(key, value_use);
        }
        for (key, callee) in callees {
            self.insert_callee(key, callee);
        }
        for (expression, recipe) in equalities {
            self.insert_equality(expression, recipe);
        }
        for (if_let, unit) in implicit_units {
            self.insert_implicit_unit(if_let, unit);
        }
        for (binary, conversion) in operand_conversions {
            self.insert_operand_conversion(binary, conversion);
        }
    }

    /// The records collected so far.
    pub fn records(&self) -> &CheckedBodies {
        &self.records
    }

    /// The records as consumers receive them.
    pub fn publish(self) -> CheckedBodies {
        self.records
    }
}

fn insert_unique<K: Eq + Hash + Copy + Debug, V>(map: &mut HashMap<K, V>, key: K, value: V) {
    let previous = map.insert(key, value);
    assert!(
        previous.is_none(),
        "internal error: two records share the key {key:?}"
    );
}

/// One expression occurrence with a lookup of the records the checker published for it.
#[derive(Clone, Copy)]
pub struct CheckedExpression<'e, 'a> {
    expression: &'e Expression,
    bodies: &'a CheckedBodies,
}

impl<'a> CheckedExpression<'_, 'a> {
    /// The adaptation that the checker decided for this value into its slot.
    pub fn value_use(&self) -> &'a CheckedValueUse {
        self.record(ValueUseKey::value(self.expression))
    }

    /// The operand of this cast, converted to the cast type.
    pub fn cast_operand(&self) -> &'a CheckedValueUse {
        self.record(ValueUseKey::cast_operand(self.expression.id()))
    }

    /// The error that a `?` on this operand returns, converted to the error slot of its function or `try` block.
    pub fn propagated_error(&self) -> &'a CheckedValueUse {
        self.record(ValueUseKey::propagated_error(self.expression.id()))
    }

    /// The reference change that this method receiver takes, as the checker recorded it.
    pub fn receiver_change(&self) -> Option<ReceiverCoercion> {
        match self.value_use().adaptation {
            Adaptation::Receiver(change) => Some(change),
            _ => None,
        }
    }

    /// The operation and type arguments that the checker selected for this callee.
    pub fn callee(&self) -> &'a CheckedCallee {
        self.function_value().unwrap_or_else(|| {
            panic!(
                "internal error: no selected callee at {:?}",
                self.expression.get_span()
            )
        })
    }

    /// The definition and type arguments of this function value, or None when the checker records none for it.
    pub fn function_value(&self) -> Option<&'a CheckedCallee> {
        self.bodies
            .callee(&ValueUseKey::value(self.expression.unwrap_parens()))
    }

    /// The equality that this comparison tests with, as the checker selected it, or None when it selects none.
    pub fn equality(&self) -> Option<&'a EqualityRecipe> {
        self.bodies.equality(self.expression.unwrap_parens().id())
    }

    /// The unit value that this `if let` produces when its pattern does not match, or None when it has an `else`.
    pub fn implicit_unit(&self) -> Option<ExpressionId> {
        self.bodies
            .implicit_units
            .get(&self.expression.id())
            .copied()
    }

    /// The operand of this binary expression that converts to the type of the other operand, or None when no operand converts.
    pub fn operand_conversion(&self) -> Option<OperandConversion> {
        self.bodies
            .operand_conversions
            .get(&self.expression.unwrap_parens().id())
            .copied()
    }

    fn record(&self, key: ValueUseKey) -> &'a CheckedValueUse {
        self.bodies.value_use(&key).unwrap_or_else(|| {
            panic!(
                "internal error: no checked value use at {:?}",
                self.expression.get_span()
            )
        })
    }
}
