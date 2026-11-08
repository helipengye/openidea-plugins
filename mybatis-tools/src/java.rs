use crate::{LineIndex, Named, ident};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Type {
    pub name: String,
    pub args: Vec<Type>,
    pub array: bool,
}

impl Type {
    pub fn label(&self) -> String {
        let mut label = self.name.rsplit('.').next().unwrap_or(&self.name).to_owned();
        if !self.args.is_empty() {
            label.push('<');
            label.push_str(&self.args.iter().map(Self::label).collect::<Vec<_>>().join(", "));
            label.push('>');
        }
        if self.array {
            label.push_str("[]");
        }
        label
    }

    pub fn parse(text: &str) -> Self {
        read_type(&tokens(text), &mut 0).unwrap_or_default()
    }

    pub fn substitute(&self, bindings: &BTreeMap<String, Type>) -> Self {
        if let Some(ty) = bindings.get(&self.name) {
            let mut ty = ty.clone();
            ty.array |= self.array;
            return ty;
        }
        Self {
            name: self.name.clone(),
            args: self.args.iter().map(|t| t.substitute(bindings)).collect(),
            array: self.array,
        }
    }

    pub fn element(&self) -> Option<Self> {
        if self.array {
            return Some(Self { array: false, ..self.clone() });
        }
        let simple = self.name.rsplit('.').next()?;
        match simple {
            "List" | "Collection" | "Iterable" | "Set" | "ArrayList" | "LinkedList" | "HashSet" => {
                self.args.first().cloned()
            }
            "Map" | "HashMap" | "LinkedHashMap" => self.args.get(1).cloned(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Parameter {
    pub name: Named,
    pub alias: Option<String>,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub(crate) struct Method {
    pub name: Named,
    pub params: Vec<Parameter>,
    pub returns: Type,
    pub is_static: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Member {
    pub name: Named,
    pub ty: Type,
    pub is_static: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Java {
    pub package: String,
    pub imports: Vec<String>,
    pub variables: Vec<String>,
    pub parents: Vec<Type>,
    pub methods: Vec<Method>,
    pub fields: Vec<Member>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
}

pub(crate) fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let tail = &text[i..];
        if tail.starts_with("//") {
            i += tail.find('\n').unwrap_or(tail.len());
            continue;
        }
        if tail.starts_with("/*") {
            i += tail.find("*/").map_or(tail.len(), |n| n + 2);
            continue;
        }
        let c = tail.chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        i += c.len_utf8();
        if c == '"' || c == '\'' {
            let mut escaped = false;
            for d in text[i..].chars() {
                i += d.len_utf8();
                if d == c && !escaped {
                    break;
                }
                escaped = d == '\\' && !escaped;
            }
        } else if ident(c) {
            while i < text.len() {
                let d = text[i..].chars().next().unwrap();
                if !ident(d) {
                    break;
                }
                i += d.len_utf8();
            }
        }
        out.push(Token { text: &text[start..i], start, end: i });
    }
    out
}

fn word(text: &str) -> bool {
    text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
}

fn read_type(ts: &[Token<'_>], at: &mut usize) -> Option<Type> {
    if ts.get(*at)?.text == "?" {
        *at += 1;
        if ts.get(*at).is_some_and(|t| matches!(t.text, "extends" | "super")) {
            *at += 1;
        } else {
            return Some(Type::parse("java.lang.Object"));
        }
    }
    let first = ts.get(*at)?;
    if !word(first.text) {
        return None;
    }
    let mut ty = Type { name: first.text.into(), ..Type::default() };
    *at += 1;
    while ts.get(*at).is_some_and(|t| t.text == ".") && ts.get(*at + 1).is_some_and(|t| word(t.text)) {
        ty.name.push('.');
        ty.name.push_str(ts[*at + 1].text);
        *at += 2;
    }
    if ts.get(*at).is_some_and(|t| t.text == "<") {
        *at += 1;
        while *at < ts.len() && ts[*at].text != ">" {
            ty.args.push(read_type(ts, at)?);
            if ts.get(*at).is_some_and(|t| t.text == ",") {
                *at += 1;
            } else {
                break;
            }
        }
        if ts.get(*at).is_some_and(|t| t.text == ">") {
            *at += 1;
        }
    }
    if ts.get(*at).is_some_and(|t| t.text == "[")
        || ts.get(*at..*at + 3).is_some_and(|s| s.iter().all(|t| t.text == "."))
    {
        ty.array = true;
        *at += if ts[*at].text == "[" { 2 } else { 3 };
    }
    Some(ty)
}

fn matching(ts: &[Token<'_>], start: usize, open: &str, close: &str) -> usize {
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate().skip(start) {
        if t.text == open {
            depth += 1;
        } else if t.text == close {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    ts.len().saturating_sub(1)
}

fn split<'a, 'b>(ts: &'a [Token<'b>]) -> Vec<&'a [Token<'b>]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (i, t) in ts.iter().enumerate() {
        match t.text {
            "<" | "(" | "[" | "{" => depth += 1,
            ">" | ")" | "]" | "}" => depth -= 1,
            "," if depth == 0 => {
                out.push(&ts[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&ts[start..]);
    out
}

fn without_annotations<'a>(ts: &[Token<'a>]) -> (Vec<Token<'a>>, Option<String>) {
    let mut out = Vec::new();
    let mut alias = None;
    let mut i = 0;
    while i < ts.len() {
        if ts[i].text != "@" {
            if ts[i].text == "(" {
                out.extend_from_slice(&ts[i..]);
                break;
            }
            out.push(ts[i]);
            i += 1;
            continue;
        }
        i += 1;
        let mut annotation = ts.get(i).map_or("", |t| t.text);
        i += usize::from(i < ts.len());
        while ts.get(i).is_some_and(|t| t.text == ".") && ts.get(i + 1).is_some_and(|t| word(t.text)) {
            annotation = ts[i + 1].text;
            i += 2;
        }
        if ts.get(i).is_some_and(|t| t.text == "(") {
            let end = matching(ts, i, "(", ")");
            if annotation == "Param" {
                alias = ts[i + 1..end]
                    .iter()
                    .find(|t| t.text.starts_with('"'))
                    .map(|t| t.text.trim_matches('"').to_owned());
            }
            i = end + 1;
        }
    }
    (out, alias)
}

fn modifiers<'a, 'b>(ts: &'a [Token<'b>]) -> &'a [Token<'b>] {
    let n = ts
        .iter()
        .take_while(|t| {
            matches!(
                t.text,
                "public"
                    | "protected"
                    | "private"
                    | "static"
                    | "final"
                    | "abstract"
                    | "default"
                    | "synchronized"
                    | "native"
                    | "transient"
                    | "volatile"
                    | "strictfp"
            )
        })
        .count();
    &ts[n..]
}

fn named(positions: &LineIndex<'_>, token: Token<'_>) -> Named {
    Named { name: token.text.into(), range: positions.range(token.start, token.end) }
}

pub(crate) fn parse(text: &str) -> Java {
    let positions = LineIndex::new(text);
    let ts = tokens(text);
    let mut out = Java::default();
    for (i, t) in ts.iter().enumerate() {
        if matches!(t.text, "package" | "import") {
            let value: String = ts[i + 1..].iter().take_while(|t| t.text != ";").map(|t| t.text).collect();
            if t.text == "package" {
                out.package = value;
            } else if !value.starts_with("static") {
                out.imports.push(value);
            }
        }
    }
    let Some(decl) = ts.iter().position(|t| matches!(t.text, "class" | "interface" | "record" | "enum")) else {
        return out;
    };
    let Some(body) = ts.iter().enumerate().skip(decl + 2).find_map(|(i, t)| (t.text == "{").then_some(i)) else {
        return out;
    };
    let mut i = decl + 2;
    if ts.get(i).is_some_and(|t| t.text == "<") {
        let end = matching(&ts, i, "<", ">");
        out.variables = split(&ts[i + 1..end]).iter().filter_map(|s| s.first().map(|t| t.text.into())).collect();
        i = end + 1;
    }
    while i < body {
        if matches!(ts[i].text, "extends" | "implements" | ",") {
            i += 1;
            if let Some(parent) = read_type(&ts[..body], &mut i) {
                out.parents.push(parent);
            }
        } else {
            i += 1;
        }
    }
    i = body + 1;
    while i < ts.len() && ts[i].text != "}" {
        let start = i;
        let mut parens = 0;
        while i < ts.len() {
            match ts[i].text {
                "(" => parens += 1,
                ")" => parens -= 1,
                ";" | "{" | "}" if parens == 0 => break,
                _ => {}
            }
            i += 1;
        }
        let (header, _) = without_annotations(&ts[start..i]);
        let is_static = header.iter().take_while(|t| t.text != "(").any(|t| t.text == "static");
        let header = modifiers(&header);
        let paren = header.iter().position(|t| t.text == "(").filter(|p| !header[..*p].iter().any(|t| t.text == "="));
        if let Some(paren) = paren {
            if paren > 0
                && let Some(returns) = read_type(&header[..paren - 1], &mut 0)
            {
                let end = matching(header, paren, "(", ")");
                let params = split(&header[paren + 1..end])
                    .into_iter()
                    .filter_map(|p| {
                        let (p, alias) = without_annotations(p);
                        let p = modifiers(&p);
                        let mut at = 0;
                        let ty = read_type(p, &mut at)?;
                        Some(Parameter { name: named(&positions, *p.get(at)?), alias, ty })
                    })
                    .collect();
                out.methods.push(Method { name: named(&positions, header[paren - 1]), params, returns, is_static });
            }
        } else {
            let mut at = 0;
            if let Some(ty) = read_type(header, &mut at) {
                for field in split(&header[at..]) {
                    if let Some(t) = field.first().filter(|t| word(t.text)) {
                        out.fields.push(Member { name: named(&positions, *t), ty: ty.clone(), is_static });
                    }
                }
            }
        }
        if ts.get(i).is_some_and(|t| t.text == "{") {
            i = matching(&ts, i, "{", "}") + 1;
        } else if ts.get(i).is_some_and(|t| t.text == ";") {
            i += 1;
        } else {
            break;
        }
    }
    out
}
