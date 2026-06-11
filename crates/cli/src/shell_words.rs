//! POSIX-style word splitting for the `--go-flags` passthrough.

#[derive(Debug, PartialEq, Eq)]
pub enum SplitError {
    UnterminatedQuote(char),
}

pub fn split(input: &str) -> Result<Vec<String>, SplitError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(other) => current.push(other),
                        None => return Err(SplitError::UnterminatedQuote('\'')),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped @ ('"' | '\\' | '$' | '`')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err(SplitError::UnterminatedQuote('"')),
                        },
                        Some(other) => current.push(other),
                        None => return Err(SplitError::UnterminatedQuote('"')),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(other) => current.push(other),
                    None => current.push('\\'),
                }
            }
            other => {
                in_word = true;
                current.push(other);
            }
        }
    }

    if in_word {
        words.push(current);
    }

    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(input: &str) -> Vec<String> {
        split(input).expect("split should succeed")
    }

    #[test]
    fn empty_input_yields_no_words() {
        assert_eq!(parts(""), Vec::<String>::new());
        assert_eq!(parts("   \t "), Vec::<String>::new());
    }

    #[test]
    fn splits_bare_words_on_whitespace() {
        assert_eq!(parts("-race"), vec!["-race"]);
        assert_eq!(parts("  a   b  "), vec!["a", "b"]);
    }

    #[test]
    fn single_quotes_group_a_value_with_spaces() {
        assert_eq!(parts("-ldflags='-s -w'"), vec!["-ldflags=-s -w"]);
        assert_eq!(parts("-gcflags='all=-N -l'"), vec!["-gcflags=all=-N -l"]);
    }

    #[test]
    fn double_quotes_group_a_value_with_spaces() {
        assert_eq!(parts("\"-s -w\""), vec!["-s -w"]);
    }

    #[test]
    fn mixes_bare_and_quoted_words() {
        assert_eq!(
            parts("-trimpath -ldflags='-s -w'"),
            vec!["-trimpath", "-ldflags=-s -w"]
        );
    }

    #[test]
    fn backslash_escapes_a_space() {
        assert_eq!(parts("a\\ b"), vec!["a b"]);
    }

    #[test]
    fn unterminated_quote_is_an_error() {
        assert_eq!(
            split("'unterminated"),
            Err(SplitError::UnterminatedQuote('\''))
        );
        assert_eq!(
            split("\"unterminated"),
            Err(SplitError::UnterminatedQuote('"'))
        );
    }
}
