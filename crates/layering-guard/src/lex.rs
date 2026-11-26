//! Blanks the parts of Rust source that cannot hold a path reference.
//!
//! Comments (line, block, nested block, and doc comments) and the contents of string and character
//! literals are replaced by spaces, so a `crate::` inside a doc link, a log message, or a test's
//! source-introspection string is not mistaken for a reference. Newlines are kept, so a position in
//! the blanked text has the same line number as in the original.

/// Returns `source` with every comment and literal blanked, one output char per input char.
pub(crate) fn blank_non_code(source: &str) -> Vec<char> {
    let mut chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let next = chars.get(i + 1).copied();
        let end = match (chars[i], next) {
            ('/', Some('/')) => line_comment_end(&chars, i),
            ('/', Some('*')) => block_comment_end(&chars, i),
            ('"', _) => quoted_end(&chars, i, '"'),
            ('\'', _) => match char_literal_end(&chars, i) {
                Some(end) => end,
                None => {
                    i += 1;
                    continue;
                }
            },
            ('r' | 'b' | 'c', _) if !is_ident_char_before(&chars, i) => match raw_string(&chars, i)
            {
                Some((start, end)) => {
                    blank(&mut chars, start, end);
                    i = end;
                    continue;
                }
                None => {
                    i += 1;
                    continue;
                }
            },
            _ => {
                i += 1;
                continue;
            }
        };
        blank(&mut chars, i, end);
        i = end;
    }
    chars
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_ident_char_before(chars: &[char], i: usize) -> bool {
    i > 0 && is_ident_char(chars[i - 1])
}

fn blank(chars: &mut [char], start: usize, end: usize) {
    for c in &mut chars[start..end] {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

fn line_comment_end(chars: &[char], start: usize) -> usize {
    chars[start..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |offset| start + offset)
}

fn block_comment_end(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1).copied()) {
            ('/', Some('*')) => {
                depth += 1;
                i += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    chars.len()
}

/// The end (exclusive) of a literal that opens at `start` with `quote` and honours backslash
/// escapes.
fn quoted_end(chars: &[char], start: usize, quote: char) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

/// The end of a character literal at `start`, or `None` when the quote opens a lifetime or label.
fn char_literal_end(chars: &[char], start: usize) -> Option<usize> {
    match (chars.get(start + 1), chars.get(start + 2)) {
        (Some('\\'), _) => Some(quoted_end(chars, start, '\'')),
        (Some(_), Some('\'')) => Some(start + 3),
        _ => None,
    }
}

/// The span of a raw string literal (`r"…"`, `r#"…"#`, `br#"…"#`, `cr"…"`) starting at `start`.
fn raw_string(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    if matches!(chars[i], 'b' | 'c') {
        i += 1;
    }
    if chars.get(i) != Some(&'r') {
        return None;
    }
    i += 1;
    let hashes = chars[i..].iter().take_while(|&&c| c == '#').count();
    i += hashes;
    if chars.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    while i < chars.len() {
        if chars[i] == '"'
            && chars[i + 1..]
                .iter()
                .take(hashes)
                .filter(|&&c| c == '#')
                .count()
                == hashes
        {
            return Some((start, i + 1 + hashes));
        }
        i += 1;
    }
    Some((start, chars.len()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn blanked(source: &str) -> String {
        blank_non_code(source).into_iter().collect()
    }

    #[test]
    fn line_and_doc_comments() {
        let source = "a // crate::x\n/// [`crate::y`]\n//! crate::z\nb";
        assert_eq!(
            blanked(source).split_whitespace().collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(blanked(source).lines().count(), 4);
    }

    #[test]
    fn nested_block_comments() {
        let source = "a /* outer /* crate::inner */ still comment */ b";
        assert_eq!(
            blanked(source).split_whitespace().collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn block_comment_keeps_newlines() {
        assert_eq!(blanked("/*\ncrate::x\n*/y"), "  \n        \n  y");
    }

    #[test]
    fn strings_with_escapes() {
        let source = r#"f("crate::a \" crate::b"); g"#;
        assert_eq!(
            blanked(source).split_whitespace().collect::<Vec<_>>(),
            ["f(", ");", "g"]
        );
    }

    #[test]
    fn raw_strings() {
        let source = r###"x = r#"crate::a "quoted" "#; y = br"crate::b"; z"###;
        assert_eq!(
            blanked(source).split_whitespace().collect::<Vec<_>>(),
            ["x", "=", ";", "y", "=", ";", "z"]
        );
    }

    #[test]
    fn identifiers_ending_in_r_are_not_raw_strings() {
        assert_eq!(blanked("for x in bar {}"), "for x in bar {}");
    }

    #[test]
    fn char_literals_and_lifetimes() {
        let source = "fn f<'a>(x: &'a str) { let q = '\"'; let s = '\\''; 'outer: loop {} }";
        let out = blanked(source);
        assert!(out.contains("<'a>"));
        assert!(out.contains("&'a str"));
        assert!(out.contains("'outer: loop"));
        assert!(!out.contains('"'));
    }

    #[test]
    fn unicode_is_one_char_per_char() {
        let source = "let s = \"é→\"; crate::x";
        assert_eq!(blank_non_code(source).len(), source.chars().count());
        assert!(blanked(source).ends_with("crate::x"));
    }
}
