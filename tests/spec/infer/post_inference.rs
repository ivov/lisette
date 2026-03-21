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
