use crate::spec::infer::*;

#[test]
fn zero_of_int() {
    infer("zero<int>()").assert_last_type(int_type());
}

#[test]
fn zero_of_string() {
    infer("zero<string>()").assert_last_type(string_type());
}

#[test]
fn zero_of_slice() {
    infer("zero<Slice<int>>()").assert_last_type(slice_type(int_type()));
}

#[test]
fn zero_of_struct() {
    infer("struct Point { x: int, y: int }\nfn f() { let _ = zero<Point>() }").assert_no_errors();
}

#[test]
fn zero_takes_its_type_from_the_annotation() {
    infer("let n: int = zero(); n").assert_last_type(int_type());
}

#[test]
fn zero_of_type_parameter_under_bound() {
    infer("fn empty<T: Zeroable>() -> T { zero<T>() }").assert_no_errors();
}

#[test]
fn zero_of_type_parameter_without_bound() {
    infer("fn empty<T>() -> T { zero<T>() }").assert_infer_code("missing_bound_on_param");
}

#[test]
fn zero_forwarded_to_a_bounded_callee_needs_its_own_bound() {
    infer("fn empty<T: Zeroable>() -> T { zero<T>() }\nfn forward<T>() -> T { empty<T>() }")
        .assert_infer_code("missing_bound_on_param");
}

#[test]
fn zero_of_ref_is_rejected() {
    infer("zero<Ref<int>>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_map_is_rejected() {
    infer("zero<Map<string, int>>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_function_is_rejected() {
    infer("zero<fn(int) -> int>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_unknown_is_rejected() {
    infer("zero<Unknown>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_error_is_rejected() {
    infer("zero<error>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_interface_is_rejected() {
    infer("interface Speaker { fn Speak() -> string }\nfn f() { let _ = zero<Speaker>() }")
        .assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_of_enum_is_rejected() {
    infer("enum Color { Red, Green }\nfn f() { let _ = zero<Color>() }")
        .assert_infer_code("not_zeroable_bound");
}

#[test]
fn a_bad_type_argument_does_not_cascade_a_zero_error() {
    // The reversed type arguments are the only mistake; a bound error buries it.
    infer("fn f() { let _ = zero<Array<3, int>>() }")
        .assert_infer_code_count("not_zeroable_bound", 0);
}

#[test]
fn zero_as_a_bare_value_is_rejected_like_other_prelude_functions() {
    // Generic, so as a bare value it would reach emit as `zero[int]`.
    infer("fn f() { let g: fn() -> int = zero\nlet _ = g }")
        .assert_infer_code("native_constructor_value");
}

#[test]
fn zero_of_enum_with_a_default_variant_is_accepted() {
    infer("enum Color { Red, #[default] Green }\nfn f() { let _ = zero<Color>() }")
        .assert_no_errors();
}

#[test]
fn zeroable_bound_admits_an_enum_with_a_default_variant() {
    infer(
        "enum Color { Red, #[default] Green }\nfn make<T: Zeroable>() -> T { zero<T>() }\nfn f() { let _ = make<Color>() }",
    )
    .assert_no_errors();
}

#[test]
fn zero_of_channel_is_rejected() {
    infer("zero<Channel<int>>()").assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_rejection_reaches_through_a_struct_field() {
    infer("struct HasFn { f: fn(int) -> int }\nfn f() { let _ = zero<HasFn>() }")
        .assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_rejection_reaches_through_a_map_field() {
    infer("struct HasMap { m: Map<string, int> }\nfn f() { let _ = zero<HasMap>() }")
        .assert_infer_code("not_zeroable_bound");
}

#[test]
fn zero_without_a_type_argument_is_rejected() {
    // Two diagnostics for one mistake, pre-existing: `fn make<T: Comparable>() -> T`
    // called as `make()` does the same.
    infer("fn f() { let _ = zero() }")
        .assert_infer_code_count("missing_type_argument", 1)
        .assert_infer_code_count("unconstrained_type_param", 1);
}

#[test]
fn zero_of_option_is_none() {
    infer("zero<Option<int>>()").assert_no_errors();
}

#[test]
fn zero_of_option_of_ref_is_none() {
    infer("zero<Option<Ref<int>>>()").assert_no_errors();
}

#[test]
fn zero_infers_a_bounded_parameter_from_a_later_assignment() {
    infer("fn f<T: Zeroable>(v: T) -> T { let mut x = zero()\n  x = v\n  x }").assert_no_errors();
}

#[test]
fn zero_infers_an_unbounded_parameter_from_a_later_assignment_and_rejects_it() {
    infer("fn f<T>(v: T) -> T { let mut x = zero()\n  x = v\n  x }")
        .assert_infer_code("missing_bound_on_param");
}

#[test]
fn zero_of_unit_is_allowed() {
    infer("fn f() { let u = zero<()>()\n  let _ = u }").assert_no_errors();
}
