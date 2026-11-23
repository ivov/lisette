use crate::_harness::pipeline::{InferenceResult, TestPipeline};
use syntax::program::Definition;
use syntax::types::{self, Type};

const SOURCE: &str = r#"
interface Named { fn name() -> string }
enum Shape { Dot, Line(int) }

type UnitAlias = ()
type UnitRef = Ref<UnitAlias>
type OptionAlias = Option<int>
type OptionRef = Ref<OptionAlias>
type ResultAlias = Result<int, string>
type ResultRef = Ref<ResultAlias>
type PartialAlias = Partial<int, error>
type PartialRef = Ref<PartialAlias>
type PairAlias = (int, string)
type PairRef = Ref<PairAlias>
type ShapeAlias = Shape
type ShapeRef = Ref<ShapeAlias>
type ErrorAlias = error
type NamedAlias = Named
type FnAlias = fn(int) -> int
type FnRef = Ref<FnAlias>
type SliceAlias = Slice<int>
type SliceRef = Ref<SliceAlias>
type SliceRefRef = Ref<SliceRef>
"#;

fn program() -> InferenceResult {
    let result = TestPipeline::new(SOURCE).compile().run_inference();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    result
}

fn named(result: &InferenceResult, name: &str) -> Type {
    let suffix = format!(".{name}");
    let id = result
        .definitions
        .keys()
        .find(|id| id.ends_with(&suffix))
        .unwrap_or_else(|| panic!("the program defines {name}"));
    Type::Nominal {
        id: id.clone(),
        params: Vec::new(),
        writable: false,
    }
}

type ShapeQuery<'a> = (&'a str, Option<&'a str>, &'a dyn Fn(&Type) -> bool);

fn lookup<'a>(result: &'a InferenceResult) -> impl Fn(&str) -> Option<&'a Definition> + 'a {
    move |id| result.definitions.get(id)
}

#[test]
fn each_shape_query_sees_through_an_alias_but_not_through_a_reference_to_it() {
    let result = program();
    let lookup = lookup(&result);
    let ty = |name: &str| named(&result, name);
    let queries: [ShapeQuery<'_>; 11] = [
        ("UnitAlias", Some("UnitRef"), &|t| {
            types::resolves_to_unit(t, &lookup)
        }),
        ("OptionAlias", Some("OptionRef"), &|t| {
            types::is_option(t, &lookup)
        }),
        ("ResultAlias", Some("ResultRef"), &|t| {
            types::is_option_or_result(t, &lookup)
        }),
        ("PartialAlias", Some("PartialRef"), &|t| {
            types::is_partial(t, &lookup)
        }),
        ("PairAlias", Some("PairRef"), &|t| {
            types::is_tuple(t, &lookup)
        }),
        ("PairAlias", Some("PairRef"), &|t| {
            types::tuple_element_types(t, &lookup).len() == 2
        }),
        ("ShapeAlias", Some("ShapeRef"), &|t| {
            types::is_lisette_enum(t, &lookup)
        }),
        ("ErrorAlias", None, &|t| types::is_prelude_error(t, &lookup)),
        ("NamedAlias", None, &|t| {
            types::as_interface(t, &lookup).is_some()
        }),
        ("FnAlias", Some("FnRef"), &|t| {
            types::resolve_to_function_type(t, &lookup).is_some()
        }),
        ("ResultAlias", None, &|t| {
            types::err_payload(t, &lookup).is_string()
        }),
    ];
    for (alias, reference, query) in queries {
        assert!(query(&ty(alias)), "{alias}");
        if let Some(reference) = reference {
            assert!(!query(&ty(reference)), "{reference}");
        }
    }
    assert!(
        types::ok_payload(&ty("OptionAlias"), &lookup) == Type::int(),
        "OptionAlias"
    );
}

#[test]
fn reference_queries_see_through_an_alias_of_a_reference_to_an_alias() {
    let result = program();
    let lookup = lookup(&result);
    let ty = |name: &str| named(&result, name);
    assert!(types::is_reference(&ty("SliceRef"), &lookup));
    assert!(!types::is_reference(&ty("SliceAlias"), &lookup));
    assert!(types::points_to_ref(&ty("SliceRefRef"), &lookup));
    assert!(!types::points_to_ref(&ty("SliceRef"), &lookup));
    for name in ["SliceAlias", "SliceRef", "SliceRefRef"] {
        let shape = types::receiver_shape(&ty(name), &lookup);
        assert!(shape.is_slice(), "{name}: {shape:?}");
    }
}
