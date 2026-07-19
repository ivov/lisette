use crate::spec::infer::*;

#[test]
fn channel_new_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn slice_new_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let s = Slice.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn slice_make_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let s = Slice.make(4);
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn slice_make_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<string>(4);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let s: Slice<bool> = Slice.make(2);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_zero_length_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<int>(0);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_option_ref_element_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<Option<Ref<int>>>(3);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_ref_element_errors() {
    infer(
        r#"
    fn test() {
      let refs = Slice.make<Ref<int>>(4);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_annotation_ref_element_errors() {
    infer(
        r#"
    fn test() {
      let refs: Slice<Ref<int>> = Slice.make(4);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_element_resolved_later_errors() {
    infer(
        r#"
    fn test() {
      let mut chans = Slice.make(1);
      chans = chans.append(Channel.new<int>());
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_struct_with_ref_field_errors() {
    infer(
        r#"
    struct Holder { r: Ref<int> }

    fn test() {
      let holders = Slice.make<Holder>(1);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_type_parameter_element_errors() {
    infer(
        r#"
    fn make<T>(n: int) -> Slice<T> {
      Slice.make<T>(n)
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn map_new_without_type_args_errors() {
    infer(
        r#"
    fn test() {
      let m = Map.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn ok_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let r = Ok(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn err_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let r = Err("failed");
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn some_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let o = Some(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn none_variant_succeeds() {
    infer(
        r#"
    fn test() -> Option<int> {
      return None;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_new_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.new<string>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_new_with_type_args_succeeds() {
    infer(
        r#"
    fn test() {
      let m = Map.new<string, int>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let ch: Channel<int> = Channel.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let s: Slice<bool> = Slice.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let m: Map<string, int> = Map.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_type_from_return_type_succeeds() {
    infer(
        r#"
    fn make_channel() -> Channel<int> {
      return Channel.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negative_literal_buffered_errors() {
    infer("fn test() { let c = Channel.buffered<int>(-2); let _ = c }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_literal_reserve_errors() {
    infer("fn test() { let mut s = [1]; s = s.reserve(-3); let _ = s }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_literal_reserve_ufcs_errors() {
    infer("fn test() { let mut s = [1]; s = Slice.reserve(s, -4); let _ = s }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_zero_size_literal_is_legal() {
    infer("fn test() { let a = Slice.make<byte>(-0); let _ = a }").assert_no_errors();
}

#[test]
fn computed_negative_size_stays_runtime() {
    infer("fn test() { let a = Slice.make<byte>(0 - 1); let _ = a }").assert_no_errors();
}

#[test]
fn slice_make_guard_sees_through_parens() {
    infer(
        r#"
    fn test() {
      let refs: Slice<Ref<int>> = (Slice.make)(4);
      let _ = refs;
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn user_static_named_make_is_a_legal_value() {
    infer(
        r#"
    struct Builder {}

    impl Builder {
      fn make() -> Slice<int> {
        [1, 2]
      }
    }

    fn test() {
      let factory = Builder.make;
      let made = factory();
      let _ = made;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_make_recovery_infers_spread() {
    let result = crate::_harness::infer::infer(
        r#"
    fn test(xs: Slice<int>) {
      let m = Map.make<string, int>(xs...);
      let _ = m;
    }
        "#,
    );
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.no_make_constructor"],
        "the recovery must infer the spread without cascading"
    );
}

#[test]
fn empty_literal_in_generic_call_errors() {
    infer(
        r#"
    fn count<T>(items: Slice<T>) -> int {
      items.length()
    }

    fn test() -> int {
      count([])
    }
        "#,
    )
    .assert_infer_code("empty_slice_no_element_type");
}

#[test]
fn empty_literal_unresolved_binding_reports_once() {
    let result = crate::_harness::infer::infer("fn test() { let xs = []; let _ = xs }");
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.type_not_inferred"],
        "an unresolved empty-literal binding must report only the binding diagnostic"
    );
}

#[test]
fn empty_literal_concrete_param_succeeds() {
    infer(
        r#"
    fn take(xs: Slice<int>) -> int {
      xs.length()
    }

    fn test() -> int {
      take([])
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let xs: Slice<string> = [];
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_resolved_by_later_append_succeeds() {
    infer(
        r#"
    fn test() {
      let mut xs = [];
      xs = xs.append(1);
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_in_generic_return_position_succeeds() {
    infer(
        r#"
    fn empty_of<T>() -> Slice<T> {
      []
    }

    fn test() {
      let xs: Slice<string> = empty_of();
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_against_non_slice_param_reports_only_mismatch() {
    let result = crate::_harness::infer::infer(
        r#"
    fn take(x: int) -> int {
      x
    }

    fn test() -> int {
      take([])
    }
        "#,
    );
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.type_mismatch"],
        "a context mismatch must not cascade into an empty-literal report"
    );
}

#[test]
fn empty_literal_in_uninferred_generic_call_reports_only_call() {
    let result = crate::_harness::infer::infer(
        r#"
    fn keep<T>(items: Slice<T>) -> Slice<T> {
      items
    }

    fn test() {
      let _ = keep([]);
    }
        "#,
    );
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.missing_type_argument"],
        "an uninferred generic call must not cascade into an empty-literal report"
    );
}

#[test]
fn empty_literal_with_unrelated_var_reports_inside_uninferred_call() {
    let result = crate::_harness::infer::infer(
        r#"
    fn count<U>(items: Slice<U>) -> int {
      items.length()
    }

    fn outer<T>(n: int) -> Slice<T> {
      []
    }

    fn test() {
      let _ = outer(count([]));
    }
        "#,
    );
    let mut errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    errors.sort_unstable();
    assert_eq!(
        errors,
        vec![
            "infer.empty_slice_no_element_type",
            "infer.missing_type_argument"
        ],
        "the literal's unresolved element is independent of the outer call's type argument, so both must report"
    );
}

#[test]
fn empty_literal_in_tuple_destructuring_errors() {
    infer(
        r#"
    fn test() {
      let (a, b) = ([], [1]);
      let _ = (a, b);
    }
        "#,
    )
    .assert_infer_code("empty_slice_no_element_type");
}

#[test]
fn impl_generic_constraint_satisfies_generic_return_type() {
    infer(
        r#"
struct Foo<E: error> {}

impl<E: error> Foo<E> {
  fn new() -> Foo<E> {
    Foo {}
  }

  fn bar(self) -> int {
    42
  }
}

fn main() {
  let foo = Foo.new<error>()
  let _ = foo.bar()
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn function_generic_constraint_satisfies_generic_return_type() {
    infer(
        r#"
struct Foo<E: error> {}

struct Factory {}

impl Factory {
  fn new<E: error>() -> Foo<E> {
    Foo {}
  }
}

impl<E: error> Foo<E> {
  fn bar(self) -> int {
    42
  }
}

fn main() {
  let foo = Factory.new<error>()
  let _ = foo.bar()
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unconstrained_generic_return_type_with_prelude_bound_errors() {
    infer(
        r#"
struct Foo<E: error> {}

impl<E: error> Foo<E> {
  fn bar(self) -> int {
    42
  }
}

fn make<E>() -> Foo<E> {
  Foo {}
}

fn main() {
  let foo = make<error>()
  let _ = foo.bar()
}
        "#,
    )
    .assert_infer_code("missing_constraint_on_return_type");
}

#[test]
fn struct_bounded_param_uninferrable_errors() {
    infer(
        r#"
struct Bar<E: error> {}

fn main() {
  let bar = Bar {}
  let _ = bar
}
        "#,
    )
    .assert_infer_code("cannot_infer_struct_type_argument");
}

#[test]
fn struct_bounded_param_via_interface_arg_errors() {
    infer(
        r#"
interface Foo<E: error> {}

struct Bar<E: error> {}

impl<E> Bar<E> {
  fn with_foo(self, _foo: Foo<E>) -> Bar<E> {
    self
  }
}

fn run<E: error>(bar: Bar<E>) -> Bar<E> {
  bar.with_foo(Bar {})
}

fn main() {
  let bar: Bar<error> = Bar {}
  let _ = run(bar)
}
        "#,
    )
    .assert_infer_code("cannot_infer_struct_type_argument");
}

#[test]
fn struct_bounded_param_with_annotation_succeeds() {
    infer(
        r#"
struct Bar<E: error> {}

fn main() {
  let bar: Bar<error> = Bar {}
  let _ = bar
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn struct_unbounded_param_uninferrable_succeeds() {
    infer(
        r#"
struct Box<T> {}

fn main() {
  let b = Box {}
  let _ = b
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn struct_empty_interface_bound_uninferrable_succeeds() {
    infer(
        r#"
interface Marker {}

struct Bar<E: Marker> {}

fn main() {
  let bar = Bar {}
  let _ = bar
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn struct_bounded_param_resolved_by_later_use_succeeds() {
    infer(
        r#"
struct MyErr {}

impl MyErr {
  fn Error(self) -> string {
    "boom"
  }
}

struct Bar<E: error> {}

fn takes(_b: Bar<MyErr>) {}

fn main() {
  let bar = Bar {}
  takes(bar)
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_struct_variant_bounded_param_uninferrable_errors() {
    infer(
        r#"
enum Wrap<E: error> {
  Full { x: int }
}

fn main() {
  let w = Wrap.Full { x: 1 }
  let _ = w
}
        "#,
    )
    .assert_infer_code("cannot_infer_struct_type_argument");
}

#[test]
fn enum_struct_variant_bounded_param_from_field_succeeds() {
    infer(
        r#"
struct MyErr {}

impl MyErr {
  fn Error(self) -> string {
    "boom"
  }
}

enum Wrap<E: error> {
  Full { e: E }
}

fn main() {
  let w = Wrap.Full { e: MyErr {} }
  let _ = w
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn struct_concrete_type_argument_violating_bound_errors() {
    infer(
        r#"
struct Bar<E: error> {}

fn main() {
  let bar: Bar<int> = Bar {}
  let _ = bar
}
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn struct_concrete_type_argument_in_return_annotation_reports_once() {
    infer(
        r#"
struct Bar<E: error> {}

fn f() -> Bar<int> {
  Bar {}
}

fn main() {
  let _ = f()
}
        "#,
    )
    .assert_infer_code_count("interface_not_implemented", 1);
}

#[test]
fn struct_concrete_type_argument_in_field_errors() {
    infer(
        r#"
struct Bar<E: error> {}

struct Holder {
  bar: Bar<int>
}

fn main() {}
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn struct_concrete_type_argument_satisfying_bound_succeeds() {
    infer(
        r#"
struct MyErr {}

impl MyErr {
  fn Error(self) -> string {
    "boom"
  }
}

struct Bar<E: error> {}

fn main() {
  let bar: Bar<MyErr> = Bar {}
  let _ = bar
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn struct_type_argument_bounded_parameter_succeeds() {
    infer(
        r#"
struct Bar<E: error> {}

fn f<T: error>(x: Bar<T>) -> Bar<T> {
  x
}

fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_tuple_variant_bounded_param_uninferrable_errors() {
    infer(
        r#"
enum W<E: error> {
  A(int)
}

fn main() {
  let w = W.A(1)
  let _ = w
}
        "#,
    )
    .assert_infer_code("cannot_infer_struct_type_argument");
}

#[test]
fn enum_bare_variant_bounded_param_uninferrable_errors() {
    infer(
        r#"
enum W<E: error> {
  A,
  B
}

fn main() {
  let w = W.A
  let _ = w
}
        "#,
    )
    .assert_infer_code("cannot_infer_struct_type_argument");
}

#[test]
fn enum_tuple_variant_bounded_param_from_arg_succeeds() {
    infer(
        r#"
struct MyErr {}

impl MyErr {
  fn Error(self) -> string {
    "boom"
  }
}

enum W<E: error> {
  A(E)
}

fn main() {
  let w = W.A(MyErr {})
  let _ = w
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_unbounded_variant_uninferrable_succeeds() {
    infer(
        r#"
enum Box<T> {
  Full(int),
  Empty
}

fn main() {
  let b = Box.Empty
  let _ = b
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn transitive_bound_required_on_struct_field_parameter() {
    infer(
        r#"
struct Bar<E: error> { e: E }
struct Holder<T> { b: Bar<T> }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_required_on_enum_variant_parameter() {
    infer(
        r#"
struct Bar<E: error> { e: E }
enum Wrap<T> { W(Bar<T>) }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_required_on_function_parameter() {
    infer(
        r#"
struct Bar<E: error> { e: E }
fn f<T>(x: Bar<T>) { let _ = x }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_required_on_type_alias_body_parameter() {
    infer(
        r#"
struct Bar<E: error> { e: E }
type Alias<T> = Bar<T>
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_required_on_forward_referenced_struct_field() {
    infer(
        r#"
struct Holder<T> { b: Bar<T> }
struct Bar<E: error> { e: E }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn concrete_bound_violation_through_type_alias_errors() {
    infer(
        r#"
struct Bar<E: error> { e: E }
type Alias = Bar<int>
fn use_alias(x: Alias) { let _ = x }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn concrete_bound_violation_forward_referenced_struct_field_errors() {
    infer(
        r#"
struct Holder { b: Bar<int> }
struct Bar<E: error> { e: E }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn transitive_bound_satisfied_on_struct_field_succeeds() {
    infer(
        r#"
struct MyErr {}
impl MyErr { fn Error(self) -> string { "e" } }
struct Bar<E: error> { e: E }
struct Holder<T: error> { b: Bar<T> }
fn main() {
  let h = Holder { b: Bar { e: MyErr {} } }
  let _ = h
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn transitive_bound_required_on_method_parameter() {
    infer(
        r#"
struct Bar<E: error> { e: E }
struct Host {}
impl Host {
  fn m<T>(self, x: Bar<T>) { let _ = x }
}
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_satisfied_on_method_via_impl_generic_succeeds() {
    infer(
        r#"
struct Bar<E: error> { e: E }
struct Host<E: error> {}
impl<E: error> Host<E> {
  fn m(self, x: Bar<E>) { let _ = x }
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn empty_interface_bound_needs_no_transitive_bound_succeeds() {
    infer(
        r#"
interface Empty {}
struct Bar<E: Empty> { e: E }
struct Holder<T> { b: Bar<T> }
"#,
    )
    .assert_no_errors();
}

#[test]
fn two_level_transitive_bound_required() {
    infer(
        r#"
struct Bar<E: error> { e: E }
struct Mid<X: error> { b: Bar<X> }
struct Holder<T> { m: Mid<T> }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_required_in_interface_method_signature() {
    infer(
        r#"
struct Bar<E: error> { e: E }
interface Foo<T> { fn get() -> Bar<T>; }
"#,
    )
    .assert_infer_code("missing_transitive_bound");
}

#[test]
fn transitive_bound_satisfied_in_interface_method_signature_succeeds() {
    infer(
        r#"
struct MyErr {}
impl MyErr { fn Error(self) -> string { "e" } }
struct Bar<E: error> { e: E }
impl<E: error> Bar<E> { fn inner(self) -> E { self.e } }
interface Foo<T: error> { fn get() -> Bar<T>; }
struct Holder { b: Bar<MyErr> }
impl Holder { fn get(self) -> Bar<MyErr> { self.b } }
fn use_foo(f: Foo<MyErr>) { let _ = f.get().inner() }
fn main() {
  let h = Holder { b: Bar { e: MyErr {} } }
  use_foo(h)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn inferred_record_struct_argument_must_satisfy_bound() {
    infer(
        r#"
interface Display { fn show() -> string }
struct Box<T: Display> { value: T }

fn main() {
  let boxed = Box { value: 42 }
  let _ = boxed
}
"#,
    )
    .assert_infer_code_once("interface_not_implemented");
}

#[test]
fn inferred_tuple_struct_argument_must_satisfy_bound() {
    infer(
        r#"
interface Display { fn show() -> string }
struct Box<T: Display>(T)

fn main() {
  let boxed = Box(42)
  let _ = boxed
}
"#,
    )
    .assert_infer_code_once("interface_not_implemented");
}

#[test]
fn inferred_enum_argument_must_satisfy_bound() {
    infer(
        r#"
interface Display { fn show() -> string }
enum Box<T: Display> { Full(T) }

fn main() {
  let boxed = Box.Full(42)
  let _ = boxed
}
"#,
    )
    .assert_infer_code_once("interface_not_implemented");
}

#[test]
fn generic_return_propagates_declared_type_bound_without_methods() {
    infer(
        r#"
struct Box<T: error> {}

fn make<T>() -> Box<T> {
  Box {}
}

fn main() {
  let boxed = make<int>()
  let _ = boxed
}
"#,
    )
    .assert_infer_code_once("missing_constraint_on_return_type");
}

#[test]
fn shadowed_parameter_does_not_inherit_constructor_bound() {
    infer(
        r#"
interface Display { fn show() -> string }
struct Owner<T: Display> { value: T }
struct Box<T: Display> { value: T }

impl<T: Display> Owner<T> {
  fn replace<T>(value: T) {
    let boxed = Box { value: value }
    let _ = boxed
  }
}
"#,
    )
    .assert_infer_code_once("missing_bound_on_param");
}
