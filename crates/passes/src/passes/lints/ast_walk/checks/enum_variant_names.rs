use crate::passes::lints::ast_walk::casing::to_snake_case;
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_enum_variant_names(expression: &Expression, ctx: &NodeCtx) {
    if ctx.is_d_lis {
        return;
    }

    let Expression::Enum {
        name,
        name_span,
        variants,
        ..
    } = expression
    else {
        return;
    };

    if variants.len() < 2 {
        return;
    }

    let enum_words = snake_words(name);
    if enum_words.is_empty() {
        return;
    }

    let variant_words: Vec<Vec<String>> = variants.iter().map(|v| snake_words(&v.name)).collect();

    let is_prefix = variant_words
        .iter()
        .all(|words| words.len() > enum_words.len() && words[..enum_words.len()] == enum_words[..]);

    let is_suffix = !is_prefix
        && variant_words.iter().all(|words| {
            words.len() > enum_words.len()
                && words[words.len() - enum_words.len()..] == enum_words[..]
        });

    if !is_prefix && !is_suffix {
        return;
    }

    let cut: usize = enum_words.iter().map(|word| word.chars().count()).sum();
    let first = variants[0].name.as_str();
    let first_len = first.chars().count();
    let example_new: String = if is_prefix {
        first.chars().skip(cut).collect()
    } else {
        first.chars().take(first_len - cut).collect()
    };

    ctx.sink.push(diagnostics::lint::enum_variant_names(
        name_span,
        name,
        is_prefix,
        first,
        &example_new,
    ));
}

fn snake_words(name: &str) -> Vec<String> {
    to_snake_case(name)
        .split('_')
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}
