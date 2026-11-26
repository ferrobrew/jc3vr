//! Extracts the paths a source file names through `crate::` or another layer's crate.
//!
//! A path is read as a use tree, so `crate::a::{b, c::{self, d}}` expands to `a::b`, `a::c`, and
//! `a::c::d`. Expression paths (`crate::a::B::new()`) are read up to the first segment that is not
//! followed by `::`; the table lookup takes the longest known prefix, so the trailing item does not
//! matter. Only `crate::` and `jc3vr_<layer>::` roots are read: `self::` and `super::` stay inside
//! the file's own module, which CONTRIBUTING.md requires for everything but `use super::*` in tests.

use crate::lex::{blank_non_code, is_ident_char};

/// Where a reference's path starts.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Root {
    /// `crate::` (or `$crate::` in a macro), resolved through the module table.
    Local,
    /// `jc3vr_<name>::`, a crate that is already cut out, carrying the name as written.
    Extern(String),
}

/// One path named by a source file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Reference {
    pub(crate) line: usize,
    pub(crate) root: Root,
    pub(crate) segments: Vec<String>,
}

impl Reference {
    /// The path as it would be written in source, for messages.
    pub(crate) fn display(&self) -> String {
        let root = match &self.root {
            Root::Local => "crate",
            Root::Extern(name) => name.as_str(),
        };
        std::iter::once(root)
            .chain(self.segments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("::")
    }
}

/// Every `crate::` and `jc3vr_*::` path in `source`, outside comments and literals.
pub(crate) fn references(source: &str) -> Vec<Reference> {
    let chars = blank_non_code(source);
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if !is_ident_char(c) || (i > 0 && is_ident_char(chars[i - 1])) {
            i += 1;
            continue;
        }
        let ident = read_ident(&chars, &mut i);
        let root = if ident == "crate" {
            Root::Local
        } else if ident.starts_with("jc3vr_") {
            Root::Extern(ident)
        } else {
            continue;
        };
        let mut cursor = i;
        skip_whitespace(&chars, &mut cursor);
        if !starts_with(&chars, cursor, "::") {
            continue;
        }
        cursor += 2;
        let mut paths = Vec::new();
        parse_tree(&chars, &mut cursor, Vec::new(), &mut paths);
        out.extend(
            paths
                .into_iter()
                .filter(|segments| !segments.is_empty())
                .map(|segments| Reference {
                    line,
                    root: root.clone(),
                    segments,
                }),
        );
        line += chars[i..cursor].iter().filter(|&&c| c == '\n').count();
        i = cursor;
    }
    out
}

fn read_ident(chars: &[char], i: &mut usize) -> String {
    let start = *i;
    if starts_with(chars, *i, "r#") {
        *i += 2;
    }
    while *i < chars.len() && is_ident_char(chars[*i]) {
        *i += 1;
    }
    chars[start..*i]
        .iter()
        .collect::<String>()
        .trim_start_matches("r#")
        .to_owned()
}

fn skip_whitespace(chars: &[char], i: &mut usize) {
    while *i < chars.len() && chars[*i].is_whitespace() {
        *i += 1;
    }
}

fn starts_with(chars: &[char], i: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(k, p)| chars.get(i + k) == Some(&p))
}

/// Reads one use tree (or expression path) after `prefix::`, pushing every full path it names.
fn parse_tree(chars: &[char], i: &mut usize, mut prefix: Vec<String>, out: &mut Vec<Vec<String>>) {
    loop {
        skip_whitespace(chars, i);
        match chars.get(*i) {
            Some('{') => {
                *i += 1;
                parse_group(chars, i, &prefix, out);
                return;
            }
            Some('*') => {
                *i += 1;
                out.push(prefix);
                return;
            }
            Some(&c) if is_ident_char(c) => {
                let segment = read_ident(chars, i);
                if segment == "self" {
                    out.push(prefix);
                    return;
                }
                prefix.push(segment);
                let mut after = *i;
                skip_whitespace(chars, &mut after);
                if starts_with(chars, after, "::") && chars.get(after + 2) != Some(&'<') {
                    *i = after + 2;
                } else {
                    out.push(prefix);
                    return;
                }
            }
            _ => {
                out.push(prefix);
                return;
            }
        }
    }
}

/// Reads the items of a `{ … }` group whose opening brace is already consumed.
fn parse_group(chars: &[char], i: &mut usize, prefix: &[String], out: &mut Vec<Vec<String>>) {
    loop {
        skip_whitespace(chars, i);
        match chars.get(*i) {
            None => return,
            Some('}') => {
                *i += 1;
                return;
            }
            Some(',') => *i += 1,
            Some(_) => {
                let before = *i;
                parse_tree(chars, i, prefix.to_vec(), out);
                skip_whitespace(chars, i);
                if starts_with(chars, *i, "as")
                    && !chars.get(*i + 2).is_some_and(|&c| is_ident_char(c))
                {
                    *i += 2;
                    skip_whitespace(chars, i);
                    read_ident(chars, i);
                }
                if *i == before {
                    // Not a use tree after all; skip the character so malformed input terminates.
                    *i += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn paths(source: &str) -> Vec<String> {
        references(source).iter().map(Reference::display).collect()
    }

    #[test]
    fn plain_use() {
        assert_eq!(
            paths("use crate::vr::render_params;"),
            ["crate::vr::render_params"]
        );
    }

    #[test]
    fn nested_groups_expand() {
        let source =
            "use crate::{\n    controllers::{self, bindings::{stage, hold::Want}},\n    hud,\n};";
        assert_eq!(
            paths(source),
            [
                "crate::controllers",
                "crate::controllers::bindings::stage",
                "crate::controllers::bindings::hold::Want",
                "crate::hud",
            ]
        );
    }

    #[test]
    fn aliases_and_globs() {
        let source = "use crate::a::{B as C, d::*, e as _};\nuse crate::f::*;";
        assert_eq!(
            paths(source),
            ["crate::a::B", "crate::a::d", "crate::a::e", "crate::f"]
        );
    }

    #[test]
    fn expression_paths_and_turbofish() {
        let source = "let x = crate::vr::State::new(); let y = crate::util::parse::<u32>(s);";
        assert_eq!(
            paths(source),
            ["crate::vr::State::new", "crate::util::parse"]
        );
    }

    #[test]
    fn macro_crate_and_visibility() {
        let source =
            "macro_rules! m { () => { $crate::config::get() } }\npub(in crate::hud) fn f() {}";
        assert_eq!(paths(source), ["crate::config::get", "crate::hud"]);
    }

    #[test]
    fn pub_crate_is_not_a_path() {
        assert_eq!(paths("pub(crate) fn f() {}"), Vec::<String>::new());
    }

    #[test]
    fn ignores_comments_and_strings() {
        let source =
            "/// See [`crate::hud`].\n// crate::vr\nlet s = \"crate::stereo\";\nuse crate::fsr;";
        assert_eq!(paths(source), ["crate::fsr"]);
    }

    #[test]
    fn identifiers_containing_crate_are_not_roots() {
        assert_eq!(
            paths("let my_crate = 1; notcrate::x();"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn extern_layer_crates() {
        let source =
            "use jc3vr_core::{lifecycle, hash::Hash};\nlet t = jc3vr_render::vr::State::new();";
        assert_eq!(
            paths(source),
            [
                "jc3vr_core::lifecycle",
                "jc3vr_core::hash::Hash",
                "jc3vr_render::vr::State::new"
            ]
        );
    }

    #[test]
    fn line_numbers_survive_multiline_groups() {
        let source = "\n\nuse crate::{\n    a,\n    b,\n};\nuse crate::c;";
        let lines: Vec<_> = references(source)
            .iter()
            .map(|r| (r.display(), r.line))
            .collect();
        assert_eq!(
            lines,
            [
                ("crate::a".to_owned(), 3),
                ("crate::b".to_owned(), 3),
                ("crate::c".to_owned(), 7)
            ]
        );
    }

    #[test]
    fn unterminated_group_terminates() {
        assert_eq!(paths("use crate::{a, b"), ["crate::a", "crate::b"]);
    }
}
