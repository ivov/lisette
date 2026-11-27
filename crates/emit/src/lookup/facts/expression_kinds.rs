macro_rules! body_expression {
    () => {
        Expression::Literal { .. }
            | Expression::Lambda { .. }
            | Expression::Block { .. }
            | Expression::Let { .. }
            | Expression::Identifier { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Tuple { .. }
            | Expression::StructCall { .. }
            | Expression::DotAccess { .. }
            | Expression::Assignment { .. }
            | Expression::Return { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
            | Expression::Binary { .. }
            | Expression::Unary { .. }
            | Expression::Paren { .. }
            | Expression::RawGo { .. }
            | Expression::Loop { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::Reference { .. }
            | Expression::IndexedAccess { .. }
            | Expression::Task { .. }
            | Expression::Defer { .. }
            | Expression::Assert { .. }
            | Expression::Select { .. }
            | Expression::Unit { .. }
            | Expression::Range { .. }
            | Expression::Cast { .. }
    };
}

macro_rules! declaration_expression {
    () => {
        Expression::Function { .. }
            | Expression::ImplBlock { .. }
            | Expression::Const { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::PackageImport { .. }
            | Expression::Interface { .. }
    };
}
