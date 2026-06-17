use ecow::EcoString;

use syntax::ast::Expression;
use syntax::types::Type;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallTarget {
    pub module: EcoString,
    pub recv_type: Option<EcoString>,
    pub fn_name: EcoString,
}

impl CallTarget {
    pub fn is(&self, module: &str, fn_name: &str) -> bool {
        self.recv_type.is_none() && self.module == module && self.fn_name == fn_name
    }

    pub fn is_method(&self, module: &str, recv_type: &str, fn_name: &str) -> bool {
        self.recv_type.as_ref().is_some_and(|r| r == recv_type)
            && self.module == module
            && self.fn_name == fn_name
    }
}

/// Resolves calls of the form `pkg.Function(...)` or `recv.Method(...)` to
/// their post-inference identity. Alias-resilient: an aliased import like
/// `import s "go:strings"` still resolves to `module = "go:strings"`. Bare
/// identifier callees and parameter-typed receivers return `None`.
pub fn resolve_call(expr: &Expression) -> Option<CallTarget> {
    let Expression::Call {
        expression: callee, ..
    } = expr
    else {
        return None;
    };
    let Expression::DotAccess {
        expression: base,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    let base_ty = base.get_type().strip_refs();
    match base_ty {
        Type::ImportNamespace(module_id) => Some(CallTarget {
            module: module_id,
            recv_type: None,
            fn_name: member.clone(),
        }),
        Type::Nominal { id, .. } => {
            let module = id.without_last_segment()?;
            Some(CallTarget {
                module: EcoString::from(module),
                recv_type: Some(EcoString::from(id.last_segment())),
                fn_name: member.clone(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_matches_free_function() {
        let t = CallTarget {
            module: EcoString::from("go:strings"),
            recv_type: None,
            fn_name: EcoString::from("Contains"),
        };
        assert!(t.is("go:strings", "Contains"));
        assert!(!t.is("go:strings", "Other"));
        assert!(!t.is("go:other", "Contains"));
        assert!(!t.is_method("go:strings", "String", "Contains"));
    }

    #[test]
    fn is_method_matches_nominal_receiver() {
        let t = CallTarget {
            module: EcoString::from("go:time"),
            recv_type: Some(EcoString::from("Time")),
            fn_name: EcoString::from("Equal"),
        };
        assert!(t.is_method("go:time", "Time", "Equal"));
        assert!(!t.is_method("go:time", "Duration", "Equal"));
        assert!(!t.is("go:time", "Equal"));
    }
}
