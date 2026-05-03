use tower_lsp::lsp_types::*;

use syntax::ast::Expression;
use syntax::types::unqualified_name;

use crate::traversal::find_enclosing_call;
pub(crate) fn handle(items: &[Expression], offset: u32) -> Option<SignatureHelp> {
    let call_expression = find_enclosing_call(items, offset)?;

    let Expression::Call {
        expression, args, ..
    } = call_expression
    else {
        return None;
    };

    let func_ty = expression.get_type();
    let func_ty_inner = match &func_ty {
        syntax::types::Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let syntax::types::Type::Function {
        params,
        return_type,
        ..
    } = func_ty_inner
    else {
        return None;
    };

    let func_name = match expression.as_ref() {
        Expression::Identifier { value, .. } => unqualified_name(value),
        Expression::DotAccess { member, .. } => member.as_str(),
        _ => "fn",
    };

    let display_params = params.as_slice();

    let param_strs: Vec<String> = display_params.iter().map(|p| p.to_string()).collect();
    let signature = format!("fn {func_name}({}) -> {return_type}", param_strs.join(", "));

    let raw_active = args
        .iter()
        .filter(|a| {
            let s = a.get_span();
            s.byte_offset + s.byte_length <= offset
        })
        .count() as u32;

    let active_param = raw_active.min((params.len() as u32).saturating_sub(1));

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
