use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// URI used by the Lisette language server.
///
/// LSP document identifiers are opaque strings except where Lisette needs to
/// translate a `file` URI to or from a local path. Keeping that narrow contract
/// avoids pulling a web-oriented URL parser and its IDNA tables into the CLI.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Url(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidUri;

impl fmt::Display for InvalidUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid URI or file path")
    }
}

impl std::error::Error for InvalidUri {}

impl Url {
    pub fn parse(value: &str) -> Result<Self, InvalidUri> {
        let Some(colon) = value.find(':') else {
            return Err(InvalidUri);
        };
        if colon == 0
            || !value[..colon].bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphabetic()
                    || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
            })
        {
            return Err(InvalidUri);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn path(&self) -> &str {
        let rest = self
            .0
            .split_once(':')
            .map_or(self.0.as_str(), |(_, rest)| rest);
        if let Some(authority_and_path) = rest.strip_prefix("//") {
            authority_and_path
                .find('/')
                .map_or("/", |index| &authority_and_path[index..])
        } else {
            rest
        }
    }

    pub fn from_file_path(path: impl AsRef<Path>) -> Result<Self, InvalidUri> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(InvalidUri);
        }

        #[cfg(windows)]
        let raw = {
            use std::path::{Component, Prefix};

            let rendered = path.to_string_lossy().replace('\\', "/");
            if matches!(
                path.components().next(),
                Some(Component::Prefix(prefix))
                    if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
            ) {
                format!("file:{}", percent_encode_path(&rendered))
            } else {
                format!(
                    "file:///{}",
                    percent_encode_path(rendered.trim_start_matches('/'))
                )
            }
        };

        #[cfg(not(windows))]
        let raw = format!("file://{}", percent_encode_path(&path.to_string_lossy()));

        Ok(Self(raw))
    }

    pub fn to_file_path(&self) -> Result<PathBuf, InvalidUri> {
        let Some(rest) = self.0.strip_prefix("file:") else {
            return Err(InvalidUri);
        };
        let decoded = percent_decode(rest).ok_or(InvalidUri)?;

        #[cfg(windows)]
        {
            let path = if let Some(path) = decoded.strip_prefix("///") {
                path.replace('/', "\\")
            } else if let Some(unc) = decoded.strip_prefix("//") {
                format!("\\\\{}", unc.replace('/', "\\"))
            } else {
                return Err(InvalidUri);
            };
            Ok(PathBuf::from(path))
        }

        #[cfg(not(windows))]
        {
            let path = decoded.strip_prefix("//").ok_or(InvalidUri)?;
            if !path.starts_with('/') {
                return Err(InvalidUri);
            }
            Ok(PathBuf::from(path))
        }
    }
}

impl fmt::Display for Url {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for Url {
    type Err = InvalidUri;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            encoded.push(char::from(byte));
        } else {
            use fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            decoded.push(hex(high)? * 16 + hex(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_round_trip_preserves_spaces_and_unicode() {
        let path = std::env::temp_dir().join("lisette uri ☃.lis");
        let uri = Url::from_file_path(&path).expect("absolute path");

        assert_eq!(uri.to_file_path(), Ok(path));
    }

    #[test]
    fn rejects_relative_file_path() {
        assert!(Url::from_file_path("src/main.lis").is_err());
    }
}
