use crate::java::tokens;
use crate::{Named, Position, Range, STATEMENTS, attrs_of, ident, range_of};
use std::collections::BTreeSet;

pub(crate) struct Tag<'a> {
    pub start: usize,
    pub end: usize,
    pub name: &'a str,
    pub closing: bool,
    pub empty: bool,
    pub attrs: Vec<(String, String, usize, usize)>,
    position: Position,
}

fn position_after(at: Position, text: &str) -> Position {
    let line = at.line + text.matches('\n').count() as u32;
    let character = match text.rfind('\n') {
        Some(i) => text[i + 1..].encode_utf16().count() as u32,
        None => at.character + text.encode_utf16().count() as u32,
    };
    Position::new(line, character)
}

impl Tag<'_> {
    pub fn attr(&self, text: &str, name: &str) -> Option<Named> {
        self.attrs.iter().find(|(n, ..)| n == name).map(|(_, value, start, end)| Named {
            name: value.clone(),
            range: Range::new(
                position_after(self.position, &text[self.start..*start]),
                position_after(self.position, &text[self.start..*end]),
            ),
        })
    }
}

pub(crate) fn tags(text: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut position_at = 0;
    let mut position = Position::new(0, 0);
    while let Some(i) = text[at..].find('<') {
        let start = at + i;
        position = position_after(position, &text[position_at..start]);
        position_at = start;
        let tail = &text[start..];
        let special = if tail.starts_with("<!--") {
            Some(("!comment", "-->"))
        } else if tail.starts_with("<![CDATA[") {
            Some(("!cdata", "]]>"))
        } else {
            None
        };
        if let Some((name, close)) = special {
            at = start + tail.find(close).map_or(tail.len(), |n| n + close.len());
            out.push(Tag { start, end: at, name, closing: false, empty: true, attrs: Vec::new(), position });
            continue;
        }
        let content = start + 1 + usize::from(tail.starts_with("</"));
        let name_end =
            text[content..].char_indices().find(|(_, c)| !ident(*c)).map_or(text.len(), |(i, _)| content + i);
        let mut quote = None;
        let mut end = text.len();
        for (i, c) in text[content..].char_indices() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '\'' || c == '"' {
                quote = Some(c);
            } else if c == '>' {
                end = content + i;
                break;
            }
        }
        let attrs =
            attrs_of(&text[content..end]).into_iter().map(|(n, v, s, e)| (n, v, content + s, content + e)).collect();
        at = (end + 1).min(text.len());
        out.push(Tag {
            start,
            end: at,
            name: &text[content..name_end],
            closing: tail.starts_with("</"),
            empty: text[content..end].trim_end().ends_with('/'),
            attrs,
            position,
        });
    }
    out
}

#[derive(Clone)]
pub(crate) struct Loop {
    pub item: Named,
    pub index: Option<Named>,
    pub collection: String,
}

#[derive(Clone)]
pub(crate) struct Expression {
    pub path: Vec<String>,
    pub statement: String,
    pub parameter_type: Option<String>,
    pub loops: Vec<Loop>,
}

pub(crate) struct Completion {
    pub expressions: Vec<Expression>,
    pub prefix: String,
    pub replace: Range,
}

pub(crate) struct MappingOwner {
    pub property: Option<String>,
    pub ty: Option<String>,
    pub collection: bool,
}

pub(crate) struct Mapping {
    pub ty: String,
    pub owners: Vec<MappingOwner>,
    pub path: Vec<String>,
}

struct Context {
    expression: Expression,
    attribute: Option<(usize, usize)>,
    fragment: Option<String>,
}

struct Frame<'a> {
    name: &'a str,
    statement: Option<(String, Option<String>)>,
    each: Option<Loop>,
    fragment: Option<String>,
}

fn frame<'a>(tag: &Tag<'a>, text: &str) -> Frame<'a> {
    let statement = if STATEMENTS.contains(&tag.name) {
        tag.attr(text, "id").map(|n| (n.name, tag.attr(text, "parameterType").map(|p| p.name)))
    } else {
        None
    };
    let each = if tag.name == "foreach" {
        tag.attr(text, "item").zip(tag.attr(text, "collection")).map(|(item, collection)| Loop {
            item,
            index: tag.attr(text, "index"),
            collection: collection.name,
        })
    } else {
        None
    };
    Frame {
        name: tag.name,
        statement,
        each,
        fragment: (tag.name == "sql").then(|| tag.attr(text, "id").map(|n| n.name)).flatten(),
    }
}

/// Only the property chain ending at the cursor is needed. Literals, SQL,
/// placeholder options and OGNL operators never become Java references.
fn path_at(text: &str, cursor: usize) -> Option<Vec<String>> {
    let masked = text.replace("&quot;", "\"     ").replace("&apos;", "'     ");
    let ts = tokens(&masked);
    let mut i = 0;
    while i < ts.len() {
        if !ts[i].text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
            || i > 0 && ts[i - 1].text == "."
            || matches!(
                ts[i].text,
                "null"
                    | "true"
                    | "false"
                    | "and"
                    | "or"
                    | "not"
                    | "eq"
                    | "ne"
                    | "lt"
                    | "le"
                    | "gt"
                    | "ge"
                    | "instanceof"
            )
        {
            i += 1;
            continue;
        }
        let mut path = vec![ts[i].text.to_owned()];
        loop {
            if ts[i].start <= cursor && cursor < ts[i].end {
                return Some(path);
            }
            if ts.get(i + 1).is_some_and(|t| t.text == ".")
                && ts
                    .get(i + 2)
                    .is_some_and(|t| t.text.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$'))
            {
                i += 2;
                path.push(ts[i].text.to_owned());
            } else {
                break;
            }
        }
        i += 1;
    }
    None
}

fn context_at(text: &str, cursor: usize) -> Option<Context> {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut attribute = None;
    for tag in tags(text) {
        if tag.start > cursor {
            break;
        }
        if cursor < tag.end {
            if tag.name == "!comment" {
                return None;
            }
            let attr = match tag.name {
                "if" | "when" => Some("test"),
                "foreach" => Some("collection"),
                _ => None,
            };
            if let Some(attr) = attr {
                attribute = tag
                    .attrs
                    .iter()
                    .find(|(n, _, s, e)| n == attr && *s <= cursor && cursor <= *e)
                    .map(|(_, _, s, e)| (*s, *e));
            }
            break;
        }
        if tag.closing {
            if let Some(i) = stack.iter().rposition(|f| f.name == tag.name) {
                stack.truncate(i);
            }
        } else if !tag.empty {
            stack.push(frame(&tag, text));
        }
    }
    let (statement, parameter_type) = stack.iter().rev().find_map(|f| f.statement.clone()).unwrap_or_default();
    let fragment = stack.iter().rev().find_map(|f| f.fragment.clone());
    Some(Context {
        expression: Expression {
            path: Vec::new(),
            statement,
            parameter_type,
            loops: stack.into_iter().filter_map(|f| f.each).collect(),
        },
        attribute,
        fragment,
    })
}

fn placeholder_at(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let before = &text[..cursor];
    let start = before.rfind("#{").into_iter().chain(before.rfind("${")).max()? + 2;
    let end = text[start..].find('}').map_or(text.len(), |i| start + i);
    if cursor > end || text[start..cursor].contains([',', '}']) {
        return None;
    }
    let end = text[start..end].find(',').map_or(end, |i| start + i);
    Some((start, end))
}

pub(crate) fn expressions_at(text: &str, cursor: usize) -> Option<Vec<Expression>> {
    let mut context = context_at(text, cursor)?;
    let (start, end) = context.attribute.or_else(|| placeholder_at(text, cursor))?;
    context.expression.path = path_at(&text[start..end], cursor - start)?;
    expression_scopes(text, context)
}

pub(crate) fn mapping_at(text: &str, cursor: usize) -> Option<Mapping> {
    let mut stack: Vec<Tag<'_>> = Vec::new();
    for tag in tags(text) {
        if tag.start > cursor {
            break;
        }
        if cursor < tag.end {
            if !matches!(tag.name, "id" | "result" | "association" | "collection") {
                return None;
            }
            let (_, _, start, end) =
                tag.attrs.iter().find(|(n, _, s, e)| n == "property" && *s <= cursor && cursor < *e)?;
            let path = path_at(&text[*start..*end], cursor - start)?;
            let root = stack.iter().rposition(|t| t.name == "resultMap")?;
            let ty = stack[root].attr(text, "type")?.name;
            let owners = stack[root + 1..]
                .iter()
                .filter_map(|parent| {
                    let (attribute, collection) = match parent.name {
                        "association" => ("javaType", false),
                        "collection" => ("ofType", true),
                        "case" => ("resultType", false),
                        _ => return None,
                    };
                    let ty = parent.attr(text, attribute).map(|n| n.name);
                    if parent.name == "case" && ty.is_none() {
                        return None;
                    }
                    Some(MappingOwner { property: parent.attr(text, "property").map(|n| n.name), ty, collection })
                })
                .collect();
            return Some(Mapping { ty, owners, path });
        }
        if tag.closing {
            if let Some(i) = stack.iter().rposition(|t| t.name == tag.name) {
                stack.truncate(i);
            }
        } else if !tag.empty {
            stack.push(tag);
        }
    }
    None
}

struct Include {
    target: String,
    owner: String,
    statement: bool,
    parameter_type: Option<String>,
    loops: Vec<Loop>,
}

/// A shared <sql> has the parameter scopes of its including statements.
fn fragment_expressions(text: &str, fragment: String, expression: Expression) -> Vec<Expression> {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut includes = Vec::new();
    let mut namespace = String::new();
    for tag in tags(text) {
        if tag.closing {
            if let Some(i) = stack.iter().rposition(|f| f.name == tag.name) {
                stack.truncate(i);
            }
            continue;
        }
        if tag.name == "mapper" {
            namespace = tag.attr(text, "namespace").map(|n| n.name).unwrap_or_default();
        }
        if tag.name == "include"
            && let Some(reference) = tag.attr(text, "refid")
        {
            let target = match reference.name.rsplit_once('.') {
                Some((ns, id)) if ns == namespace => Some(id.to_owned()),
                Some(_) => None,
                None => Some(reference.name),
            };
            let owner = stack.iter().rev().find_map(|f| {
                f.statement
                    .clone()
                    .map(|(name, ty)| (name, true, ty))
                    .or_else(|| f.fragment.clone().map(|name| (name, false, None)))
            });
            if let Some((target, (owner, statement, parameter_type))) = target.zip(owner) {
                includes.push(Include {
                    target,
                    owner,
                    statement,
                    parameter_type,
                    loops: stack.iter().filter_map(|f| f.each.clone()).collect(),
                });
            }
        }
        if !tag.empty {
            stack.push(frame(&tag, text));
        }
    }
    let mut pending = vec![(fragment, expression.loops)];
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    while let Some((fragment, loops)) = pending.pop() {
        if seen.len() >= 32 || !seen.insert(fragment.clone()) {
            continue;
        }
        for include in includes.iter().filter(|i| i.target == fragment) {
            let loops: Vec<_> = include.loops.iter().chain(&loops).cloned().collect();
            if include.statement {
                out.push(Expression {
                    path: expression.path.clone(),
                    statement: include.owner.clone(),
                    parameter_type: include.parameter_type.clone(),
                    loops,
                });
            } else {
                pending.push((include.owner.clone(), loops));
            }
        }
    }
    out
}

pub(crate) fn completion_at(text: &str, cursor: usize) -> Option<Completion> {
    let mut context = context_at(text, cursor)?;
    let (start, end) = context.attribute.or_else(|| placeholder_at(text, cursor))?;
    let value = &text[start..end];
    let masked = value.replace("&quot;", "\"     ").replace("&apos;", "'     ");
    if tokens(&masked)
        .iter()
        .any(|t| t.text.starts_with(['\'', '"']) && t.start < cursor - start && cursor - start < t.end)
    {
        return None;
    }
    let before = &text[start..cursor];
    let path_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !ident(*c) && *c != '.')
        .map_or(start, |(i, c)| start + i + c.len_utf8());
    let mut parts: Vec<_> = text[path_start..cursor].split('.').map(str::to_owned).collect();
    let prefix = parts.pop()?;
    if parts.iter().any(|p| p.is_empty() || !p.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')) {
        return None;
    }
    let replace_start = cursor - prefix.len();
    let replace_end = cursor + text[cursor..end].chars().take_while(|c| ident(*c)).map(char::len_utf8).sum::<usize>();
    context.expression.path = parts;
    let expressions = expression_scopes(text, context)?;
    Some(Completion { expressions, prefix, replace: range_of(text, replace_start, replace_end) })
}

fn expression_scopes(text: &str, context: Context) -> Option<Vec<Expression>> {
    if let Some(fragment) = context.fragment {
        Some(fragment_expressions(text, fragment, context.expression))
    } else if !context.expression.statement.is_empty() {
        Some(vec![context.expression])
    } else {
        None
    }
}
