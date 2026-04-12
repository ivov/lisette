#[macro_export]
macro_rules! assert_lex_snapshot {
    ($input:expr) => {
        let lex_result = syntax::lex::Lexer::new($input, 0).lex();

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(lex_result);
        });
    };
}

#[macro_export]
macro_rules! assert_parse_snapshot {
    ($input:expr) => {
        let lex_result = syntax::lex::Lexer::new($input, 0).lex();
        let parse_result = syntax::parse::Parser::new(lex_result.tokens, $input).parse();

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(parse_result.ast);
        });
    };
}

#[macro_export]
macro_rules! assert_desugar_snapshot {
    ($input:expr) => {
        let lex_result = syntax::lex::Lexer::new($input, 0).lex();
        assert!(
            !lex_result.failed(),
            "Lexer failed: {:?}",
            lex_result.errors
        );

        let parse_result = syntax::parse::Parser::new(lex_result.tokens, $input).parse();
        assert!(
            !parse_result.failed(),
            "Parser failed: {:?}",
            parse_result.errors
        );

        let desugar_result = syntax::desugar::desugar(parse_result.ast);
        assert!(
            desugar_result.errors.is_empty(),
            "Desugaring failed: {:?}",
            desugar_result.errors
        );

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(desugar_result.ast);
        });
    };
}

#[macro_export]
macro_rules! assert_lex_error_snapshot {
    ($source:expr) => {
        use syntax::lex::Lexer;

        let lex_result = Lexer::new($source, 0).lex();
        if lex_result.errors.is_empty() {
            panic!("Expected lexer errors but lexing succeeded");
        }

        let output = $crate::_harness::formatting::format_parse_error_for_snapshot(
            &lex_result.errors[0],
            $source,
            "test.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_parse_error_snapshot {
    ($source:expr) => {
        use syntax::lex::Lexer;
        use syntax::parse::Parser;

        let lex_result = Lexer::new($source, 0).lex();
        if lex_result.failed() {
            panic!("Lexing failed in parse error test");
        }
        let parse_result = Parser::new(lex_result.tokens, $source).parse();
        if parse_result.errors.is_empty() {
            panic!("Expected parser errors but parsing succeeded");
        }

        let output = $crate::_harness::formatting::format_parse_error_for_snapshot(
            &parse_result.errors[0],
            $source,
            "test.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_desugar_error_snapshot {
    ($source:expr) => {
        use syntax::lex::Lexer;
        use syntax::parse::Parser;
        use syntax::desugar;

        let lex_result = Lexer::new($source, 0).lex();
        if lex_result.failed() {
            panic!("Lexing failed in desugar error test");
        }
        let parse_result = Parser::new(lex_result.tokens, $source).parse();
        if parse_result.failed() {
            panic!("Parsing failed in desugar error test");
        }
        let desugar_result = desugar::desugar(parse_result.ast);
        if desugar_result.errors.is_empty() {
            panic!("Expected desugar errors but desugaring succeeded");
        }

        let output = $crate::_harness::formatting::format_parse_error_for_snapshot(
            &desugar_result.errors[0],
            $source,
            "test.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_infer_error_snapshot {
    ($source:expr) => {
        let result = $crate::_harness::infer::infer($source);
        if result.errors.is_empty() {
            panic!("Expected errors but inference succeeded");
        }

        let output = $crate::_harness::formatting::format_diagnostic_for_snapshot(
            &result.errors[0],
            $source,
            "test.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_multimodule_infer_error_snapshot {
    ($result:expr, $source:expr) => {
        if $result.errors.is_empty() {
            panic!("Expected errors but inference succeeded");
        }

        let output = $crate::_harness::formatting::format_diagnostic_for_snapshot(
            &$result.errors[0],
            $source,
            "main.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_emit_snapshot {
    ($input:expr) => {
        let emit_result = $crate::_harness::emit::emit($input);
        let go_code = emit_result.go_code();

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(go_code);
        });
    };
}

#[macro_export]
macro_rules! assert_emit_snapshot_with_go_typedefs {
    ($input:expr, $typedefs:expr) => {
        let emit_result = $crate::_harness::emit::emit_with_go_typedefs($input, $typedefs);
        let go_code = emit_result.go_code();

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(go_code);
        });
    };
}

#[macro_export]
macro_rules! assert_lint_snapshot {
    ($source:expr) => {
        let warnings = $crate::_harness::lint::lint($source);
        if warnings.is_empty() {
            panic!("Expected lint warnings but none were produced");
        }

        let output = $crate::_harness::formatting::format_diagnostic_for_snapshot(
            &warnings[0],
            $source,
            "test.lis",
        );

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_no_lint_warnings {
    ($source:expr) => {
        let warnings = $crate::_harness::lint::lint($source);
        if !warnings.is_empty() {
            let warnings_str: Vec<String> = warnings.iter().map(|w| format!("{:?}", w)).collect();
            panic!(
                "Expected no lint warnings but got:\n{}",
                warnings_str.join("\n")
            );
        }
    };
}

#[macro_export]
macro_rules! assert_build_snapshot {
    ($fs:expr, $go_module:expr) => {
        let output = $crate::_harness::build::compile_project($fs, $go_module);

        insta::with_settings!({
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(output);
        });
    };
}

#[macro_export]
macro_rules! assert_format_snapshot {
    ($input:expr) => {
        use format::format_source;

        let formatted = format_source($input).expect("formatting should succeed");

        let formatted_twice =
            format_source(&formatted).expect("formatting formatted output should succeed");
        assert_eq!(
            formatted, formatted_twice,
            "format is not idempotent:\n--- first format ---\n{}\n--- second format ---\n{}",
            formatted, formatted_twice
        );

        insta::with_settings!({
            description => format!("input: {}", $input),
            prepend_module_to_snapshot => false,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(formatted);
        });
    };
}
