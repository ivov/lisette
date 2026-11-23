use crate::_harness::infer::for_each_expression;
use crate::_harness::pipeline::{InferenceResult, TestPipeline};
use syntax::ast::{Expression, Pattern};
use syntax::program::{Adaptation, CallOperation, InterfaceConversion};
use syntax::types::{IterableKind, RangeKind, peel_alias, unqualified_name};

fn check(source: &str) -> InferenceResult {
    let result = TestPipeline::new(source).compile().run_inference();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    result
}

fn find<'a>(items: &'a [Expression], matches: &dyn Fn(&Expression) -> bool) -> &'a Expression {
    fn walk<'a>(
        expression: &'a Expression,
        matches: &dyn Fn(&Expression) -> bool,
    ) -> Option<&'a Expression> {
        if matches(expression) {
            return Some(expression);
        }
        expression
            .children()
            .into_iter()
            .find_map(|child| walk(child, matches))
    }
    items
        .iter()
        .find_map(|item| walk(item, matches))
        .expect("the test program has the expression")
}

#[test]
fn unset_fields_of_a_literal_through_a_generic_alias_are_substituted() {
    let result = check(
        r#"
struct Pair<T> { a: T, b: T, c: int }

type Ints = Pair<int>

fn make() -> Ints {
  Ints { a: 1, .. }
}
"#,
    );
    let literal = find(&result.ast, &|e| matches!(e, Expression::StructCall { .. }));
    let record = result.checked_bodies.occurrence(literal).struct_literal();
    let unset: Vec<(String, String)> = record
        .unset_fields
        .iter()
        .map(|(name, ty)| (name.to_string(), ty.to_string()))
        .collect();
    assert_eq!(
        unset,
        vec![
            ("b".to_string(), "int".to_string()),
            ("c".to_string(), "int".to_string())
        ]
    );
    assert!(record.via_alias);
}

#[test]
fn unset_field_of_a_literal_through_a_generic_alias_has_a_field_selection() {
    let result = check(
        r#"
struct Pair<T> { a: T, b: T, c: int }

type Ints = Pair<int>

fn make() -> Ints {
  Ints { a: 1, .. }
}
"#,
    );
    let literal = find(&result.ast, &|e| matches!(e, Expression::StructCall { .. }));
    let selection = result.checked_bodies.occurrence(literal).literal_field("b");
    assert!(
        selection.owner.as_str().ends_with(".Pair"),
        "{}",
        selection.owner
    );
    assert_eq!(selection.declared_ty.to_string(), "T");
    assert_eq!(selection.field_ty().to_string(), "int");
}

#[test]
fn literal_written_through_its_own_name_is_not_via_alias() {
    let result = check(
        r#"
struct Pair<T> { a: T, b: T }

fn make() -> Pair<int> {
  Pair { a: 1, b: 2 }
}
"#,
    );
    let literal = find(&result.ast, &|e| matches!(e, Expression::StructCall { .. }));
    let record = result.checked_bodies.occurrence(literal).struct_literal();
    assert!(record.unset_fields.is_empty());
    assert!(!record.via_alias);
}

#[test]
fn field_selection_of_a_dot_access_through_a_ref_to_a_generic_alias() {
    let result = check(
        r#"
struct Box<T> { value: T }

type IntBox = Box<int>

fn read(b: Ref<IntBox>) -> int {
  b.value
}
"#,
    );
    let access = find(&result.ast, &|e| matches!(e, Expression::DotAccess { .. }));
    let selection = result
        .checked_bodies
        .occurrence(access)
        .field_selection()
        .expect("a struct field access has a field selection");
    assert!(
        selection.owner.as_str().ends_with(".Box"),
        "{}",
        selection.owner
    );
    assert_eq!(selection.declared_ty.to_string(), "T");
    assert_eq!(selection.field_ty().to_string(), "int");
}

#[test]
fn field_selection_of_a_literal_field_through_a_generic_alias() {
    let result = check(
        r#"
struct Box<T> { value: T }

type Strs = Box<string>

fn make() -> Strs {
  Strs { value: "a" }
}
"#,
    );
    let literal = find(&result.ast, &|e| matches!(e, Expression::StructCall { .. }));
    let selection = result
        .checked_bodies
        .occurrence(literal)
        .literal_field("value");
    assert!(
        selection.owner.as_str().ends_with(".Box"),
        "{}",
        selection.owner
    );
    assert_eq!(selection.field_ty().to_string(), "string");
}

#[test]
fn field_selection_of_a_pattern_field_through_a_generic_alias() {
    let result = check(
        r#"
enum Shape<T> {
  Dot(T),
  Rect { w: T, h: T },
}

type Two<A> = Shape<A>

fn width(s: Two<bool>) -> bool {
  match s {
    Two.Rect { w, h: _ } => w,
    Two.Dot(d) => d,
  }
}
"#,
    );
    let subject = find(&result.ast, &|e| matches!(e, Expression::Match { .. }));
    let Expression::Match { arms, .. } = subject else {
        unreachable!()
    };
    let Pattern::Struct { fields, .. } = &arms[0].pattern else {
        panic!("the first arm is a struct pattern");
    };
    let selection = result.checked_bodies.pattern_field(fields[0].name_span);
    assert!(
        selection.owner.as_str().ends_with(".Shape"),
        "{}",
        selection.owner
    );
    assert_eq!(selection.field_ty().to_string(), "bool");
}

#[test]
fn field_selections_of_a_tuple_variant_pattern_through_a_generic_alias() {
    let result = check(
        r#"
enum Shape<T> {
  Dot(T, int),
  Empty,
}

type Two<A> = Shape<A>

fn first(s: Two<bool>) -> bool {
  match s {
    Two.Dot(d, _) => d,
    Two.Empty => false,
  }
}
"#,
    );
    let subject = find(&result.ast, &|e| matches!(e, Expression::Match { .. }));
    let Expression::Match { arms, .. } = subject else {
        unreachable!()
    };
    let fields: Vec<(String, String)> = result
        .checked_bodies
        .variant_pattern_fields(arms[0].pattern.get_span())
        .into_iter()
        .map(|selection| {
            (
                selection.declared_ty.to_string(),
                selection.field_ty().to_string(),
            )
        })
        .collect();
    assert_eq!(
        fields,
        vec![
            ("T".to_string(), "bool".to_string()),
            ("int".to_string(), "int".to_string()),
        ]
    );
}

#[test]
fn iterable_kind_of_a_for_over_a_ref_to_a_map_alias() {
    let result = check(
        r#"
type Table = Map<string, int>

fn total(t: Ref<Table>) -> int {
  let mut sum = 0
  for (_, v) in t {
    sum = sum + v
  }
  sum
}
"#,
    );
    let for_loop = find(&result.ast, &|e| matches!(e, Expression::For { .. }));
    assert_eq!(
        result.checked_bodies.occurrence(for_loop).iterable_kind(),
        IterableKind::Map
    );
}

fn interface_conversion<'r>(result: &'r InferenceResult, name: &str) -> &'r InterfaceConversion {
    let value = find(
        &result.ast,
        &|e| matches!(e, Expression::Identifier { value, .. } if value == name),
    );
    let Adaptation::Interface(conversion) = &result
        .checked_bodies
        .occurrence(value)
        .value_use()
        .adaptation
    else {
        panic!("the value converts to an interface");
    };
    conversion
}

#[test]
fn witness_of_a_bound_receiver_selects_the_requirement_of_the_bound() {
    let result = check(
        r#"
pub interface Named {
  fn name() -> Partial<int, error>
}

pub interface Labeled {
  fn name() -> Partial<int, error>
}

type Tag = Named

fn label<T: Tag>(t: T) -> Labeled {
  t as Labeled
}
"#,
    );
    let cast = find(&result.ast, &|e| matches!(e, Expression::Cast { .. }));
    let Adaptation::Interface(conversion) = &result
        .checked_bodies
        .occurrence(cast)
        .cast_operand()
        .adaptation
    else {
        panic!("the cast operand converts to an interface");
    };
    let witness = conversion
        .witness
        .as_ref()
        .expect("the conversion has a witness");
    let method = &witness.methods[0];
    let selected = method
        .selected_requirement
        .as_ref()
        .expect("a bound method selects a requirement of the bound");
    assert!(selected.declaring_interface.as_str().ends_with(".Named"));
    assert_eq!(selected.name, "name");
}

#[test]
fn witness_of_a_generic_receiver_instantiates_the_declared_signature() {
    let result = check(
        r#"
interface Reader {
  fn read() -> int
}

struct Cell<T> { value: T }

impl<T> Cell<T> {
  fn read(self) -> T {
    self.value
  }
}

fn reader(c: Cell<int>) -> Reader {
  c
}
"#,
    );
    let conversion = interface_conversion(&result, "c");
    let witness = conversion
        .witness
        .as_ref()
        .expect("the conversion has a witness");
    let method = &witness.methods[0];
    assert!(method.selected_requirement.is_none());
    let instantiated = method
        .instantiated_declared_signature
        .as_function_type()
        .expect("a method signature is a function");
    assert_eq!(instantiated.return_type.to_string(), "int");
    let declared = method
        .declared_signature
        .as_function_type()
        .expect("a method signature is a function");
    assert_eq!(declared.return_type.to_string(), "T");
}

#[test]
fn interface_conversion_through_aliases_records_the_peeled_target_and_source() {
    let result = check(
        r#"
interface Reader {
  fn read() -> int
}

type Source = Reader

struct Cell { value: int }

type Slot = Cell

impl Cell {
  fn read(self) -> int {
    self.value
  }
}

fn reader(c: Slot) -> Source {
  c
}
"#,
    );
    let conversion = interface_conversion(&result, "c");
    let target = conversion
        .target_interface
        .as_ref()
        .expect("the target is an interface");
    assert!(target.to_string().ends_with("Reader"), "{target}");
    assert!(
        conversion.source_shape.to_string().ends_with("Cell"),
        "{}",
        conversion.source_shape
    );
}

#[test]
fn tuple_struct_constructor_through_a_generic_alias_names_the_struct() {
    let result = check(
        r#"
struct Pair<T>(T, T)

type Same<A> = Pair<A>

struct Meters(int)

type Length = Meters

fn make() -> Same<int> {
  Same(3, 4)
}

fn length() -> Length {
  Length(5)
}
"#,
    );
    let callees: Vec<&Expression> = ["Same", "Length"]
        .into_iter()
        .map(|name| {
            find(
                &result.ast,
                &|e| matches!(e, Expression::Identifier { value, .. } if value == name),
            )
        })
        .collect();
    let operations: Vec<(String, bool)> = callees
        .into_iter()
        .map(|callee| {
            let checked = result
                .checked_bodies
                .occurrence(callee)
                .function_value()
                .expect("a constructor call has a checked callee");
            let CallOperation::TupleStructConstructor { owner, is_newtype } = &checked.operation
            else {
                panic!("the call constructs a tuple struct");
            };
            (unqualified_name(owner).to_string(), *is_newtype)
        })
        .collect();
    assert_eq!(
        operations,
        vec![("Pair".to_string(), false), ("Meters".to_string(), true)]
    );
}

#[test]
fn positional_fields_of_a_tuple_struct_call_through_a_generic_alias_are_substituted() {
    let result = check(
        r#"
struct Pair<A, B>(A, B)

type Keyed<T> = Pair<string, T>

fn make() -> Keyed<Ref<int>> {
  let n = 1
  Keyed("a", &n)
}
"#,
    );
    let call = find(&result.ast, &|e| matches!(e, Expression::Call { .. }));
    let fields: Vec<(String, String, String)> = (0..2)
        .map(|index| {
            let selection = result
                .checked_bodies
                .occurrence(call)
                .positional_field(index);
            (
                unqualified_name(&selection.owner).to_string(),
                selection.declared_ty.to_string(),
                selection.field_ty().to_string(),
            )
        })
        .collect();
    assert_eq!(
        fields,
        vec![
            ("Pair".to_string(), "A".to_string(), "string".to_string()),
            ("Pair".to_string(), "B".to_string(), "Ref<int>".to_string()),
        ]
    );
}

#[test]
fn receiver_generics_of_an_impl_use_the_impl_names_with_the_declared_bounds() {
    let result = check(
        r#"
interface Shown {
  fn shown() -> string
}

struct Labeled<K, V: Shown> { key: K, value: V }

impl<U: Shown> Labeled<int, U> {
  fn label(self) -> string {
    self.value.shown()
  }
}
"#,
    );
    let impl_block = find(&result.ast, &|e| matches!(e, Expression::ImplBlock { .. }));
    let generics: Vec<(String, Vec<String>)> = result
        .checked_bodies
        .occurrence(impl_block)
        .receiver_generics()
        .iter()
        .map(|(name, bounds)| {
            (
                name.to_string(),
                bounds
                    .iter()
                    .map(|bound| unqualified_name(&bound.to_string()).to_string())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(generics, vec![("U".to_string(), vec!["Shown".to_string()])]);
}

#[test]
fn panicking_try_block_through_an_alias_publishes_a_resolved_ok_type() {
    let result = check(
        r#"
type R = Result<int, string>

fn risky() -> R { Ok(1) }

fn total() -> int {
  let r: R = try {
    let _ = risky()?;
    panic("fatal")
  }
  match r {
    Ok(n) => n,
    Err(_) => 0,
  }
}
"#,
    );
    let block = find(&result.ast, &|e| matches!(e, Expression::TryBlock { .. }));
    let peeled = peel_alias(&block.get_type(), |id| result.definitions.get(id));
    assert_eq!(peeled.ok_type().to_string(), "int", "{peeled}");
}

#[test]
fn map_into_a_go_named_map_records_a_newtype_adaptation() {
    let result = TestPipeline::new(
        r#"
import "go:example.com/funcs"

fn register() {
  let mut table = Map.new<string, Unknown>()
  table["n"] = 1
  funcs.Use(table)
}
"#,
    )
    .with_go_typedef(
        "go:example.com/funcs",
        r#"
pub struct FuncMap(mut Map<string, Unknown>)

pub fn Use(m: FuncMap)
"#,
    )
    .compile()
    .run_inference();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let call = find(
        &result.ast,
        &|e| matches!(e, Expression::Call { expression, .. } if matches!(expression.as_ref(), Expression::DotAccess { member, .. } if member == "Use")),
    );
    let Expression::Call { args, .. } = call else {
        unreachable!()
    };
    let adaptation = &result
        .checked_bodies
        .occurrence(&args[0])
        .value_use()
        .adaptation;
    assert!(matches!(adaptation, Adaptation::Newtype), "{adaptation:?}");
}

#[test]
fn propagated_error_through_an_alias_spelling_records_no_conversion() {
    let result = check(
        r#"
type Fault = error

fn risky() -> Result<int, error> { Ok(1) }

fn total() -> Result<int, Fault> {
  let n = risky()?
  Ok(n)
}
"#,
    );
    let propagate = find(&result.ast, &|e| matches!(e, Expression::Propagate { .. }));
    let Expression::Propagate { expression, .. } = propagate else {
        unreachable!()
    };
    let recorded = result
        .checked_bodies
        .occurrence(expression)
        .propagated_error()
        .expect("a propagated error record");
    assert!(!recorded.adaptation.converts(), "{:?}", recorded.adaptation);
}

#[test]
fn stored_range_through_an_alias_records_its_range_kind() {
    let result = check(
        r#"
type Span = RangeInclusive<int>
type Tail = RangeFrom<int>

fn cut(items: Slice<int>, word: string, s: Span, t: Tail) -> (Slice<int>, string) {
  (items[s], word.substring(t))
}
"#,
    );
    let index = find(&result.ast, &|e| {
        matches!(e, Expression::IndexedAccess { .. })
    });
    let Expression::IndexedAccess { index, .. } = index else {
        unreachable!()
    };
    assert_eq!(
        result.checked_bodies.occurrence(index).range_kind(),
        Some(RangeKind::RangeInclusive)
    );
    let call = find(
        &result.ast,
        &|e| matches!(e, Expression::Call { expression, .. } if matches!(expression.as_ref(), Expression::DotAccess { member, .. } if member == "substring")),
    );
    let Expression::Call { args, .. } = call else {
        unreachable!()
    };
    assert_eq!(
        result.checked_bodies.occurrence(&args[0]).range_kind(),
        Some(RangeKind::RangeFrom)
    );
}

#[test]
fn propagated_operand_of_the_failure_type_records_a_forwarded_failure() {
    let result = check(
        r#"
type Outcome = Result<(), string>

fn step() -> Result<(), string> { Ok(()) }

fn count() -> Result<int, string> { Ok(1) }

fn run() -> Outcome {
  step()?
  let _ = count()?
  let inner: Result<int, string> = try {
    let n = count()?
    step()?
    n
  }
  let _ = inner
  Ok(())
}
"#,
    );
    let mut forwards = Vec::new();
    for_each_expression(&result.ast, |expression| {
        if let Expression::Propagate { expression, .. } = expression {
            forwards.push(
                result
                    .checked_bodies
                    .occurrence(expression)
                    .forwards_failure(),
            );
        }
    });
    assert_eq!(forwards, [true, false, true, false]);
}
