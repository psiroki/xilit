//! Parsing of "object references" (`"Name"` / `#N`) and the small amount of
//! quoting/tokenizing machinery shared by the interactive shell, `-c` and
//! `-s` invocation modes.
//!
//! The exact syntax/behavior of the shell is intentionally left open by the
//! spec ("can be implemented sensibly and refined later"), so this module
//! implements a small, predictable JSON-flavored quoting scheme:
//!
//! * Bare words are split on ASCII whitespace.
//! * `"..."` opens a JSON-style quoted string (supporting the usual escapes:
//!   `\" \\ \/ \n \t \r \b \f \uXXXX`) which may contain whitespace, `/`,
//!   `;` etc. A quoted segment butted up directly against more non-space
//!   characters (e.g. `"My Mesh"/0`) is treated as one token, matching
//!   ordinary shell behavior.
//! * `;` and newlines separate commands, except when they occur inside a
//!   quoted string.

use anyhow::{anyhow, bail, Result};

/// A reference to a glTF object: either a name or a zero-based index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjRef {
    Index(usize),
    Name(String),
}

impl std::fmt::Display for ObjRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjRef::Index(i) => write!(f, "#{i}"),
            ObjRef::Name(n) => write!(f, "{n:?}"),
        }
    }
}

/// Split a raw command string (the contents of `-c`, a script file, or a
/// single line typed into the interactive shell) into individual raw
/// command strings, on top-level `;` or newline. Quoted regions are
/// respected so that a `;` inside a JSON string does not split the command.
pub fn split_commands(input: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;

    while let Some(c) = chars.next() {
        if in_quotes {
            current.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else if c == '"' {
                in_quotes = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_quotes = true;
                current.push(c);
            }
            ';' | '\n' | '\r' => {
                if !current.trim().is_empty() {
                    commands.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        commands.push(current);
    }

    commands
}

/// Split a single raw command string into raw (still-quoted) whitespace
/// separated tokens, respecting quoted regions.
pub fn tokenize_raw(input: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;

    while let Some(c) = chars.next() {
        if in_quotes {
            current.push(c);
            if c == '\\' {
                match chars.next() {
                    Some(next) => current.push(next),
                    None => bail!("unterminated escape sequence in quoted string"),
                }
            } else if c == '"' {
                in_quotes = false;
            }
            continue;
        }

        if c.is_whitespace() {
            if has_token {
                tokens.push(std::mem::take(&mut current));
                has_token = false;
            }
            continue;
        }

        if c == '"' {
            in_quotes = true;
            has_token = true;
            current.push(c);
            continue;
        }

        has_token = true;
        current.push(c);
    }

    if in_quotes {
        bail!("unterminated quoted string");
    }

    if has_token {
        tokens.push(current);
    }

    Ok(tokens)
}

/// Unescape a JSON-style quoted string. `s` must start and end with `"`.
fn unescape_json_string(s: &str) -> Result<String> {
    let inner = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| anyhow!("expected a JSON-quoted string, got: {s}"))?;

    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = (0..4)
                    .map(|_| chars.next().ok_or_else(|| anyhow!("truncated \\u escape")))
                    .collect::<Result<String>>()?;
                let code = u32::from_str_radix(&hex, 16)
                    .map_err(|_| anyhow!("invalid \\u escape: {hex}"))?;
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            Some(other) => bail!("invalid escape sequence: \\{other}"),
            None => bail!("truncated escape sequence"),
        }
    }
    Ok(out)
}

/// Unquote/unescape a single raw token that may be a bare word, a fully
/// quoted string, or a mix (a quoted segment immediately followed by more
/// literal characters, as produced by `tokenize_raw`).
pub fn unquote_token(tok: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = tok;
    while !rest.is_empty() {
        if rest.starts_with('"') {
            let (quoted, remainder) = take_quoted(rest)?;
            out.push_str(&unescape_json_string(quoted)?);
            rest = remainder;
        } else {
            // Consume up to the next quote (if any).
            let next_quote = rest.find('"').unwrap_or(rest.len());
            out.push_str(&rest[..next_quote]);
            rest = &rest[next_quote..];
        }
    }
    Ok(out)
}

/// Given a string starting with `"`, return `(quoted_including_quotes,
/// remainder_after_closing_quote)`.
fn take_quoted(s: &str) -> Result<(&str, &str)> {
    debug_assert!(s.starts_with('"'));
    let bytes = s.as_bytes();
    let mut i = 1;
    let mut escaped = false;
    while i < bytes.len() {
        let c = bytes[i];
        if escaped {
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
        } else if c == b'"' {
            return Ok((&s[..=i], &s[i + 1..]));
        }
        i += 1;
    }
    bail!("unterminated quoted string in token: {s}")
}

/// Parse a raw (still possibly quoted) token as an [`ObjRef`].
pub fn parse_objref(tok: &str) -> Result<ObjRef> {
    if let Some(rest) = tok.strip_prefix('#') {
        let idx: usize = rest
            .parse()
            .map_err(|_| anyhow!("invalid object index: #{rest}"))?;
        return Ok(ObjRef::Index(idx));
    }
    if tok.starts_with('"') {
        let (quoted, remainder) = take_quoted(tok)?;
        if !remainder.is_empty() {
            bail!("unexpected trailing characters after quoted name: {remainder}");
        }
        return Ok(ObjRef::Name(unescape_json_string(quoted)?));
    }
    if tok.is_empty() {
        bail!("expected an object reference (a name or #index), got nothing");
    }
    if tok.contains('/') || tok.chars().any(|c| c.is_whitespace()) {
        bail!(
            "object name {tok:?} contains '/' or whitespace and must be JSON-quoted, e.g. \"{tok}\""
        );
    }
    Ok(ObjRef::Name(tok.to_string()))
}

/// Parse a raw token of the form `<objref>/<primitiveIndex>` as used by
/// `mesh export`, e.g. `#0/0` or `"My Mesh"/0`.
pub fn parse_objref_with_index(tok: &str) -> Result<(ObjRef, usize)> {
    let (name_part, remainder) = if tok.starts_with('"') {
        take_quoted(tok)?
    } else if let Some(rest) = tok.strip_prefix('#') {
        let digits_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits_len == 0 {
            bail!("expected digits after '#' in {tok:?}");
        }
        let split_at = 1 + digits_len;
        (&tok[..split_at], &tok[split_at..])
    } else {
        let slash = tok
            .find('/')
            .ok_or_else(|| anyhow!("expected <mesh>/<primitiveIndex>, got: {tok}"))?;
        (&tok[..slash], &tok[slash..])
    };

    let remainder = remainder
        .strip_prefix('/')
        .ok_or_else(|| anyhow!("expected '/<primitiveIndex>' after {name_part:?} in {tok:?}"))?;
    if remainder.is_empty() || !remainder.chars().all(|c| c.is_ascii_digit()) {
        bail!("expected a numeric primitive index after '/', got: {remainder:?}");
    }
    let prim_index: usize = remainder.parse()?;
    let objref = parse_objref(name_part)?;
    Ok((objref, prim_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_basic() {
        let toks = tokenize_raw(r#"mesh export #0/0 cube.mdx"#).unwrap();
        assert_eq!(toks, vec!["mesh", "export", "#0/0", "cube.mdx"]);
    }

    #[test]
    fn tokenize_quoted_name() {
        let toks = tokenize_raw(r#"mesh export "My Mesh"/0 cube.mdx"#).unwrap();
        assert_eq!(toks, vec!["mesh", "export", "\"My Mesh\"/0", "cube.mdx"]);
    }

    #[test]
    fn split_commands_respects_quotes() {
        let cmds = split_commands(r#"mesh list; mesh export "a;b"/0 x.mdx"#);
        assert_eq!(cmds.len(), 2);
        assert!(cmds[1].contains("a;b"));
    }

    #[test]
    fn objref_index() {
        assert_eq!(parse_objref("#3").unwrap(), ObjRef::Index(3));
    }

    #[test]
    fn objref_with_index_quoted() {
        let (r, i) = parse_objref_with_index("\"My Mesh\"/2").unwrap();
        assert_eq!(r, ObjRef::Name("My Mesh".to_string()));
        assert_eq!(i, 2);
    }

    #[test]
    fn objref_with_index_hash() {
        let (r, i) = parse_objref_with_index("#0/0").unwrap();
        assert_eq!(r, ObjRef::Index(0));
        assert_eq!(i, 0);
    }
}
