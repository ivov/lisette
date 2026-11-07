use crate::protocol::*;

use syntax::ast::{Expression, Span};
use syntax::lex::{Lexer, Token, TokenKind};
use syntax::types::unqualified_name;

use crate::traversal::find_enclosing_call;
use syntax::types::Type;
pub(crate) fn handle(items: &[Expression], source: &str, offset: u32) -> Option<SignatureHelp> {
    let call_expression = find_enclosing_call(items, offset)?;

    let Expression::Call {
        expression,
        args,
        spread,
        span,
        ..
    } = call_expression
    else {
        return None;
    };

    let func_ty = expression.get_type();
    let func_ty_inner = match &func_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let Type::Function(f) = func_ty_inner else {
        return None;
    };
    let params = &f.params;
    let return_type = &f.return_type;

    let func_name = match expression.as_ref() {
        Expression::Identifier { value, .. } => unqualified_name(value),
        Expression::DotAccess { member, .. } => member.as_str(),
        _ => "fn",
    };

    let param_strs: Vec<String> = params
        .iter()
        .map(|param| match &param.name {
            Some(name) => format!("{name}: {}", param.ty),
            None => param.ty.to_string(),
        })
        .collect();
    let signature = format!("fn {func_name}({}) -> {return_type}", param_strs.join(", "));

    let active_param = active_parameter(source, *span, expression, args, spread.as_deref(), offset)
        .min((params.len() as u32).saturating_sub(1));

    let param_infos: Vec<ParameterInformation> = param_strs
        .into_iter()
        .map(|label| ParameterInformation {
            label: ParameterLabel::Simple(label),
            documentation: None,
        })
        .collect();

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: signature,
            documentation: None,
            parameters: Some(param_infos),
            active_parameter: Some(active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

fn active_parameter(
    source: &str,
    call_span: Span,
    callee: &Expression,
    args: &[Expression],
    spread: Option<&Expression>,
    offset: u32,
) -> u32 {
    let callee_start = callee.get_span().byte_offset;
    let piped = args
        .iter()
        .filter(|arg| arg.get_span().byte_offset < callee_start)
        .count() as u32;
    let written: Vec<Span> = args
        .iter()
        .chain(spread)
        .map(Expression::get_span)
        .filter(|span| span.byte_offset >= callee_start)
        .collect();

    let Some(text) = source.get(call_span.byte_offset as usize..call_span.end() as usize) else {
        return piped;
    };
    let tokens: Vec<Token<'_>> = Lexer::new(text, 0)
        .lex()
        .tokens
        .into_iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Comment
                    | TokenKind::DocComment
                    | TokenKind::FileComment
                    | TokenKind::Semicolon
                    | TokenKind::EOF
            )
        })
        .collect();
    let callee_end = callee.get_span().end() - call_span.byte_offset;
    let Some((open, close)) = argument_list(&tokens, callee_end) else {
        return piped;
    };

    let mut slot = 0;
    let mut has_placeholder = false;
    let gaps = tokens[open + 1..=close].windows(2).filter(|pair| {
        let at = call_span.byte_offset + pair[0].byte_offset;
        !written
            .iter()
            .any(|span| span.byte_offset <= at && at < span.end())
    });
    for pair in gaps {
        let [token, next] = pair else { continue };
        match token.kind {
            TokenKind::Comma if call_span.byte_offset + token.byte_offset < offset => slot += 1,
            TokenKind::Identifier if token.text == "_" && next.kind != TokenKind::Ellipsis => {
                has_placeholder = true;
            }
            _ => {}
        }
    }
    piped.saturating_sub(u32::from(has_placeholder)) + slot
}

fn argument_list(tokens: &[Token<'_>], callee_end: u32) -> Option<(usize, usize)> {
    let mut close = tokens.len().checked_sub(1)?;
    loop {
        if tokens[close].kind != TokenKind::RightParen {
            return None;
        }
        let open = matching_left_paren(&tokens[..=close])?;
        if tokens[open].byte_offset >= callee_end {
            return Some((open, close));
        }
        close = close.checked_sub(1)?;
    }
}

fn matching_left_paren(tokens: &[Token<'_>]) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().rev() {
        match token.kind {
            TokenKind::RightParen => depth += 1,
            TokenKind::LeftParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
