//! CamelCase for OpenIDEA: cycles identifier cases in every selection.

use openidea_plugin::text::{offset, position};
use openidea_plugin::{Error, Invocation, Plugin, Range, TextEdit, TextEditor, register_plugin};

#[derive(Default)]
pub struct CamelCase;

impl Plugin for CamelCase {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if id != "toggle" {
            return Err(Error::NotFound(format!("command {id}")));
        }
        let editor = ctx.editor().ok_or("CamelCase works on the focused editor")?;
        toggle(&editor)?;
        Ok(None)
    }
}

register_plugin!(CamelCase);

fn toggle(editor: &TextEditor) -> Result<u64, Error> {
    let selections = editor.selections()?;
    for _ in 0..2 {
        let doc = editor.document()?;
        let edits = selections
            .iter()
            .filter_map(|selection| {
                let range = word_range(&doc.text, *selection);
                let old = doc.text_in(range)?;
                let new = next_case(old)?;
                (new != old).then(|| TextEdit { range, text: new })
            })
            .collect::<Vec<_>>();
        match editor.apply_edits(doc.version, &edits) {
            Err(Error::Stale) => continue,
            other => return other,
        }
    }
    Err(Error::Stale)
}

fn word_range(text: &str, selection: Range) -> Range {
    if !selection.is_empty() {
        return selection;
    }
    let Some(at) = offset(text, selection.start) else { return selection };
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let start = text[..at].char_indices().rev().take_while(|(_, c)| word(*c)).last().map_or(at, |(i, _)| i);
    let end = text[at..].char_indices().find(|(_, c)| !word(*c)).map_or(text.len(), |(i, _)| at + i);
    Range::new(position(text, start), position(text, end))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Kebab,
    ScreamingSnake,
    Pascal,
    Camel,
    Snake,
    Space,
}

impl Case {
    fn next(self) -> Self {
        match self {
            Self::Kebab => Self::ScreamingSnake,
            Self::ScreamingSnake => Self::Pascal,
            Self::Pascal => Self::Camel,
            Self::Camel => Self::Snake,
            Self::Snake => Self::Space,
            Self::Space => Self::Kebab,
        }
    }

    fn render(self, words: &[String]) -> String {
        match self {
            Self::Kebab => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("-"),
            Self::ScreamingSnake => words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"),
            Self::Pascal => words.iter().map(|w| capitalized(w)).collect(),
            Self::Camel => {
                words.iter().enumerate().map(|(i, w)| if i == 0 { w.to_lowercase() } else { capitalized(w) }).collect()
            }
            Self::Snake => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"),
            Self::Space => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join(" "),
        }
    }
}

fn next_case(input: &str) -> Option<String> {
    let chars: Vec<char> = input.chars().collect();
    if !chars.iter().any(|c| c.is_alphabetic()) {
        return None;
    }

    let letters_are_upper = chars.iter().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
    let letters_are_lower = chars.iter().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase());
    let has_space = chars.contains(&' ');
    let has_dash = chars.contains(&'-');
    let has_underscore = chars.contains(&'_');
    let case = if has_space && letters_are_lower {
        Case::Space
    } else if has_dash && letters_are_lower {
        Case::Kebab
    } else if has_underscore && letters_are_upper {
        Case::ScreamingSnake
    } else if has_underscore && letters_are_lower {
        Case::Snake
    } else if !has_space && !has_dash && !has_underscore && chars[0].is_uppercase() {
        Case::Pascal
    } else if !has_space && !has_dash && !has_underscore && chars[0].is_lowercase() {
        Case::Camel
    } else {
        return None;
    };

    Some(case.next().render(&words(input)))
}

fn words(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }

        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let boundary = c.is_uppercase()
            && prev.is_some_and(|p| {
                p.is_lowercase() || p.is_ascii_digit() || (p.is_uppercase() && next.is_some_and(char::is_lowercase))
            });
        if boundary && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn capitalized(word: &str) -> String {
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|first| first.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn cycles_through_all_cases() {
        let mut value = "foo-bar-baz".to_owned();
        for expected in ["FOO_BAR_BAZ", "FooBarBaz", "fooBarBaz", "foo_bar_baz", "foo bar baz", "foo-bar-baz"] {
            value = next_case(&value).unwrap();
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn preserves_acronyms_and_skips_non_identifiers() {
        assert_eq!(next_case("HTTPServer"), Some("httpServer".to_owned()));
        assert_eq!(next_case("foo_BAR"), None);
        assert_eq!(next_case("42"), None);
    }

    #[test]
    fn edits_selections_and_caret_words() {
        let host = MockHost::install();
        host.open_editor("/p/a.rs", "rust", "let fooBar = baz_qux;\n", &[(0, 4, 0, 10), (0, 15, 0, 15)]);
        CamelCase.run_command("toggle", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "let foo_bar = baz qux;\n");
        assert_eq!(host.editor_version(), 2, "one undoable multi-cursor change");
    }

    #[test]
    fn caret_without_a_word_makes_no_edit() {
        let host = MockHost::install();
        host.open_editor("/p/a.rs", "rust", "let  foo = bar;\n", &[(0, 3, 0, 3)]);
        CamelCase.run_command("toggle", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "let  foo = bar;\n");
        assert_eq!(host.editor_version(), 1);
    }
}
