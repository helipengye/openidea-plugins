use serde::Serialize;
use std::collections::BTreeMap;

const VERBS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"];

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Endpoint {
    pub path: String,
    pub module: String,
    pub controller: String,
    pub controller_label: String,
    pub name: String,
    pub label: String,
    pub method: String,
    pub url: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    literal: Option<String>,
    offset: usize,
}

fn tokens(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    if bytes.get(i..i + 2) == Some(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if bytes.get(i..i + 2) == Some(b"*/") {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'"' | b'\'' | b'`' => {
                let quote = bytes[i];
                let triple = quote == b'"' && bytes.get(i..i + 3) == Some(b"\"\"\"");
                let width = if triple { 3 } else { 1 };
                i += width;
                let mut value = String::new();
                while i < bytes.len() {
                    if triple && bytes.get(i..i + 3) == Some(b"\"\"\"") || !triple && bytes[i] == quote {
                        i += width;
                        break;
                    }
                    let c = source[i..].chars().next().unwrap();
                    i += c.len_utf8();
                    if c == '\\' && !triple && quote != b'`' && i < bytes.len() {
                        let escaped = source[i..].chars().next().unwrap();
                        i += escaped.len_utf8();
                        value.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        value.push(c);
                    }
                }
                out.push(Token {
                    text: if quote == b'`' { value.clone() } else { source[start..i].into() },
                    literal: (quote == b'"').then_some(value),
                    offset: start,
                });
            }
            _ => {
                let c = source[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c.is_alphanumeric() || c == '_' || c == '$' {
                    while let Some(c) = source[i..].chars().next() {
                        if !c.is_alphanumeric() && c != '_' && c != '$' {
                            break;
                        }
                        i += c.len_utf8();
                    }
                }
                out.push(Token { text: source[start..i].into(), literal: None, offset: start });
            }
        }
    }
    out
}

fn pairs(t: &[Token]) -> Vec<usize> {
    let mut out = vec![t.len(); t.len()];
    let mut stack = Vec::new();
    for (i, token) in t.iter().enumerate() {
        if token.literal.is_some() {
            continue;
        }
        match token.text.as_str() {
            "(" | "[" | "{" => stack.push(i),
            ")" | "]" | "}" => {
                if let Some(&open) = stack.last() {
                    let expected = match t[open].text.as_str() {
                        "(" => ")",
                        "[" => "]",
                        _ => "}",
                    };
                    if token.text == expected {
                        out[open] = i;
                        stack.pop();
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Default)]
struct Annotation {
    name: String,
    args: BTreeMap<String, Vec<Token>>,
}

fn annotation(t: &[Token], pair: &[usize], i: &mut usize) -> Annotation {
    *i += 1;
    let mut a = Annotation::default();
    if *i >= t.len() {
        return a;
    }
    a.name = t[*i].text.clone();
    *i += 1;
    while *i + 1 < t.len() && t[*i].text == "." {
        a.name = t[*i + 1].text.clone();
        *i += 2;
    }
    if *i < t.len() && t[*i].text == "(" {
        let end = pair[*i].min(t.len());
        *i += 1;
        while *i < end {
            let start = *i;
            while *i < end && t[*i].text != "," {
                if matches!(t[*i].text.as_str(), "(" | "[" | "{") {
                    *i = pair[*i].min(end);
                }
                *i += 1;
            }
            let segment = &t[start..(*i).min(end)];
            if segment.len() >= 2 && segment[1].text == "=" {
                a.args.insert(segment[0].text.clone(), segment[2..].to_vec());
            } else if !segment.is_empty() {
                a.args.insert("value".into(), segment.to_vec());
            }
            *i += 1;
        }
        *i = end.saturating_add(1);
    }
    a
}

// Only literal strings and literal concatenations are evaluated. An unresolved
// constant must never silently become a route at the project root.
fn strings(expr: &[Token]) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut current = None::<String>;
    let mut plus = false;
    for token in expr {
        if let Some(value) = &token.literal {
            if current.is_some() && !plus {
                return None;
            }
            current.get_or_insert_default().push_str(value);
            plus = false;
        } else {
            match token.text.as_str() {
                "{" | "}" | "[" | "]" | "(" | ")" | "arrayOf" => {}
                "+" => plus = true,
                "," => {
                    result.push(current.take()?);
                    plus = false;
                }
                _ => return None,
            }
        }
    }
    if plus {
        return None;
    }
    if let Some(current) = current {
        result.push(current);
    }
    (!result.is_empty()).then_some(result)
}

fn paths(a: &Annotation, key: &str) -> Option<Vec<String>> {
    match a.args.get(key).or_else(|| (key == "path").then(|| a.args.get("value")).flatten()) {
        Some(expr) => strings(expr),
        None => Some(vec![String::new()]),
    }
}

fn verbs(a: &Annotation) -> Option<Vec<String>> {
    match a.args.get("method") {
        None => Some(vec!["ANY".into()]),
        Some(expr) => {
            let methods: Vec<_> =
                expr.iter().filter(|t| VERBS.contains(&t.text.as_str())).map(|t| t.text.clone()).collect();
            (!methods.is_empty()).then_some(methods)
        }
    }
}

fn spring(a: &Annotation) -> bool {
    matches!(
        a.name.as_str(),
        "RequestMapping" | "GetMapping" | "PostMapping" | "PutMapping" | "DeleteMapping" | "PatchMapping"
    )
}

fn label(annotations: &[Annotation], keys: &[(&str, &str)]) -> String {
    keys.iter()
        .find_map(|(name, key)| {
            annotations
                .iter()
                .find(|a| a.name == *name)?
                .args
                .get(*key)
                .and_then(|expr| strings(expr))?
                .into_iter()
                .next()
        })
        .unwrap_or_default()
}

fn routes(annotations: &[Annotation], parents: &[Annotation]) -> Vec<(String, String)> {
    let mapping = annotations.iter().find(|a| spring(a));
    let (bases, own, methods) = if let Some(a) = mapping {
        let parent = parents.iter().find(|a| a.name == "RequestMapping");
        let feign = parents.iter().find(|a| a.name == "FeignClient");
        let bases = if let Some(parent) = parent {
            paths(parent, "path")
        } else if let Some(expr) = feign.and_then(|a| a.args.get("path")) {
            strings(expr)
        } else {
            Some(vec![String::new()])
        };
        let methods = if a.name == "RequestMapping" {
            if a.args.contains_key("method") { verbs(a) } else { parent.map_or_else(|| verbs(a), verbs) }
        } else {
            Some(vec![a.name.trim_end_matches("Mapping").to_ascii_uppercase()])
        };
        (bases, paths(a, "path"), methods)
    } else if let Some(verb) = annotations.iter().find(|a| VERBS.contains(&a.name.as_str())) {
        let bases =
            parents.iter().find(|a| a.name == "Path").map_or_else(|| Some(vec![String::new()]), |a| paths(a, "value"));
        let own = annotations
            .iter()
            .find(|a| a.name == "Path")
            .map_or_else(|| Some(vec![String::new()]), |a| paths(a, "value"));
        (bases, own, Some(vec![verb.name.clone()]))
    } else {
        return Vec::new();
    };
    let (Some(bases), Some(own), Some(methods)) = (bases, own, methods) else { return Vec::new() };
    let mut result = Vec::new();
    for base in &bases {
        for own in &own {
            let url = format!("/{}/{}", base.trim_matches('/'), own.trim_matches('/'));
            let url = url.replace("//", "/");
            let url = if url.len() > 1 { url.trim_end_matches('/') } else { &url };
            for method in &methods {
                if url.len() <= 4096 {
                    result.push((method.clone(), url.to_owned()));
                }
            }
        }
    }
    result
}

pub fn scan(path: &str, source: &str) -> Vec<Endpoint> {
    let t = tokens(source);
    let pair = pairs(&t);
    let lines: Vec<usize> = std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let mut result = Vec::new();
    declarations(path, source, &t, &pair, &lines, 0, t.len(), "", &[], &mut result);
    result.sort_by(|a, b| (&a.line, &a.method, &a.url).cmp(&(&b.line, &b.method, &b.url)));
    result.dedup();
    result
}

#[allow(clippy::too_many_arguments)]
fn declarations(
    path: &str,
    source: &str,
    t: &[Token],
    pair: &[usize],
    lines: &[usize],
    mut i: usize,
    end: usize,
    controller: &str,
    parents: &[Annotation],
    result: &mut Vec<Endpoint>,
) {
    let mut annotations = Vec::new();
    while i < end {
        match t[i].text.as_str() {
            "@" => annotations.push(annotation(t, pair, &mut i)),
            "class" | "interface" | "object" | "record" if i + 1 < end => {
                let name = t[i + 1].text.clone();
                i += 2;
                while i < end && !matches!(t[i].text.as_str(), "{" | ";") {
                    if t[i].text == "(" {
                        i = pair[i].min(end);
                    }
                    i += 1;
                }
                if i < end && t[i].text == "{" {
                    let close = pair[i].min(end);
                    declarations(path, source, t, pair, lines, i + 1, close, &name, &annotations, result);
                    i = close.saturating_add(1);
                }
                annotations.clear();
            }
            "(" => {
                if i > 0 && !controller.is_empty() {
                    let name = &t[i - 1];
                    for (method, url) in routes(&annotations, parents) {
                        let line = lines.partition_point(|start| *start <= name.offset) - 1;
                        let column = source[lines[line]..name.offset].encode_utf16().count() as u32;
                        result.push(Endpoint {
                            path: path.into(),
                            module: String::new(),
                            controller: controller.into(),
                            controller_label: label(parents, &[("Tag", "name"), ("Api", "tags"), ("Api", "value")]),
                            name: name.text.clone(),
                            label: label(&annotations, &[("Operation", "summary"), ("ApiOperation", "value")]),
                            method,
                            url,
                            line: line as u32,
                            column,
                        });
                    }
                }
                annotations.clear();
                i = pair[i].min(end).saturating_add(1);
            }
            "{" | "[" => {
                i = pair[i].min(end).saturating_add(1);
                annotations.clear();
            }
            ";" | "=" => {
                annotations.clear();
                i += 1;
            }
            _ => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_arrays_verbs_and_nested_classes() {
        let source = r#"
            @RestController @RequestMapping(path = {"/v1", "/v2"}) class Users {
                @RequestMapping(value = {"/users", "/people"}, method = {RequestMethod.GET, RequestMethod.POST})
                public List<User> list(@RequestParam String q) { return call(); }
                @GetMapping("/one/" + "{id}") public User get() { return null; }
                class Other { @DeleteMapping("/x") void remove() {} }
            }
            @RequestMapping("/separate") class Other { @PatchMapping void update() {} }
        "#;
        let endpoints = scan("/project/Users.java", source);
        assert_eq!(endpoints.len(), 12);
        assert!(endpoints.iter().any(|e| e.method == "POST" && e.url == "/v2/people" && e.name == "list"));
        assert!(endpoints.iter().any(|e| e.url == "/v1/one/{id}"));
        assert!(endpoints.iter().any(|e| e.url == "/x" && e.controller == "Other"));
        assert!(endpoints.iter().any(|e| e.url == "/separate" && e.method == "PATCH"));
    }

    #[test]
    fn kotlin_feign_jaxrs_comments_and_utf16() {
        let source = "// @GetMapping(\"/fake\")\n\
            @FeignClient(name = \"users\", path = \"/remote\") interface Client {\n\
            @GetMapping(path = [\"/a\", \"/b\"]) fun list(): List<User>\n}\n\
            @jakarta.ws.rs.Path(\"/jax\") class Resource {\n\
            /* @GET @Path(\"fake\") fun fake() {} */\n\
            @GET @Path(\"/{id}\") fun 用户() = \"@POST\"\n}\n";
        let endpoints = scan("/project/Routes.kt", source);
        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].url, "/remote/a");
        let last = endpoints.last().unwrap();
        assert_eq!((last.url.as_str(), last.name.as_str()), ("/jax/{id}", "用户"));
        let line = source.lines().nth(last.line as usize).unwrap();
        assert_eq!(last.column as usize, line[..line.find("用户").unwrap()].encode_utf16().count());
    }

    #[test]
    fn unresolved_paths_are_skipped_and_method_bodies_do_not_leak() {
        let endpoints = scan(
            "A.java",
            r#"
            class A {
                @GetMapping(PATH) void unresolved() {}
                @PostMapping("/real") void real() { String s = "@GetMapping(\"/fake\")"; }
                @GetMapping("/ok") void ok() {}
            }
            @RequestMapping(BASE) class B { @GetMapping("/bad") void skipped() {} }
        "#,
        );
        assert_eq!(endpoints.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), ["/real", "/ok"]);
    }

    #[test]
    fn request_mapping_without_verb_is_explicitly_any() {
        let endpoints = scan("A.kt", "class A { @RequestMapping fun root() {} }");
        assert_eq!((endpoints[0].method.as_str(), endpoints[0].url.as_str()), ("ANY", "/"));
    }

    #[test]
    fn feign_service_name_is_not_a_path_prefix() {
        let endpoints =
            scan("A.java", r#"@FeignClient("users") interface Client { @GetMapping("/users") List list(); }"#);
        assert_eq!(endpoints[0].url, "/users");
    }

    #[test]
    fn scans_more_than_1000_routes_in_one_file_and_reads_swagger_labels() {
        let methods = (0..1200)
            .map(|n| format!("@Operation(summary=\"接口 {n}\") @GetMapping(\"/{n}\") void m{n}() {{}}\n"))
            .collect::<String>();
        let endpoints = scan("C.java", &format!("@Tag(name=\"会员控制器\") class C {{\n{methods}}}"));
        assert_eq!(endpoints.len(), 1200);
        assert_eq!(endpoints[1199].label, "接口 1199");
        assert_eq!(endpoints[0].controller_label, "会员控制器");
    }
}
