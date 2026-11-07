//! mybatis-tools for OpenIDEA: a mapper interface (`com.acme.UserMapper`)
//! and the XML with `<mapper namespace="com.acme.UserMapper">` point at each
//! other: gutter arrows and Go to Definition on XML statement ids,
//! completion of statement ids and `#{parameters}`.
//!
//! Editor calls die past their wall-clock deadline (the plugin is then stopped
//! as crashed), so the namespace → XML index is never built in one go: the
//! `UserMapper.java` ↔ `UserMapper.xml` naming convention answers outright,
//! and everything else waits for a scan that every call advances one budgeted
//! slice of, keeping its progress in the plugin instance.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Project, Range, host, register_plugin};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

mod java;
mod navigation;
mod xml;

/// How long a remembered fact is trusted: a completed index, a resolved
/// interface path, a namespace confirmed absent.
const REMEMBER: Duration = Duration::from_secs(600);
/// Wall time one provider call may spend on the scan; editor calls are
/// interrupted (and the plugin reported as crashed) past their deadline, so
/// every slice stays well inside it and the instance, with its progress,
/// survives.
const SCAN_SLICE: Duration = Duration::from_millis(35);
/// How many same-named XML files the naming convention reads before giving up.
const GUESS_LIMIT: u32 = 32;
/// Completeness cap of a full scan (the host caps any `find_files` at 10_000).
const MAX_XML: u32 = 10_000;
const STATEMENTS: [&str; 4] = ["select", "insert", "update", "delete"];

/// A named thing and where its name is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    pub range: Range,
}

/// A mapper XML file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapperXml {
    pub namespace: Option<String>,
    pub namespace_ref: Option<Named>,
    /// `<select|insert|update|delete id="…">` (the id value's range).
    pub statements: Vec<Named>,
    /// `<resultMap id="…">` and `<sql id="…">` definitions.
    pub result_maps: Vec<Named>,
    pub sqls: Vec<Named>,
    /// `resultMap="…"` on a statement, association, collection or case.
    pub map_refs: Vec<Named>,
    /// `refid="…"` on `<include>`.
    pub sql_refs: Vec<Named>,
    /// A Java class name written in `resultType`, `parameterType`, `type`,
    /// `javaType` or `ofType`.
    pub classes: Vec<Named>,
}

/// A Java interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapperJava {
    /// Fully qualified name.
    pub name: Option<String>,
    pub methods: Vec<(Named, Vec<String>)>,
}

fn range_of(text: &str, start: usize, end: usize) -> Range {
    Range::new(position(text, start), position(text, end))
}

pub fn parse_xml(text: &str) -> MapperXml {
    let mut out = MapperXml::default();
    for tag in xml::tags(text) {
        if tag.closing {
            continue;
        }
        let name = tag.name;
        let attr = |what: &str| tag.attr(text, what);
        match name {
            "mapper" => {
                out.namespace_ref = attr("namespace");
                out.namespace = out.namespace_ref.as_ref().map(|n| n.name.clone());
            }
            "sql" => out.sqls.extend(attr("id")),
            "resultMap" => {
                out.result_maps.extend(attr("id"));
                out.classes.extend(attr("type"));
            }
            "include" => out.sql_refs.extend(attr("refid")),
            _ => {
                if STATEMENTS.contains(&name) {
                    out.statements.extend(attr("id"));
                }
                // Where class names and map references are written; selectKey
                // sits inside a statement, association/collection/case inside
                // a resultMap.
                if STATEMENTS.contains(&name) || matches!(name, "selectKey" | "association" | "collection" | "case") {
                    out.map_refs.extend(attr("resultMap"));
                    for a in ["resultType", "parameterType", "javaType", "ofType"] {
                        out.classes.extend(attr(a));
                    }
                }
            }
        }
    }
    out
}

/// The `name="value"` (or single-quoted) attributes of a tag's text, with
/// each value's start and end as byte offsets into `tag`.
fn attrs_of(tag: &str) -> Vec<(String, String, usize, usize)> {
    let b = tag.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !(b[i] as char).is_ascii_alphanumeric() && !matches!(b[i], b'_' | b'-') {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || matches!(b[i], b'_' | b'-')) {
            i += 1;
        }
        let name = &tag[name_start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || (b[i] != b'"' && b[i] != b'\'') {
            continue;
        }
        let quote = b[i];
        i += 1;
        let value_start = i;
        while i < b.len() && b[i] != quote {
            i += 1;
        }
        if i >= b.len() {
            break; // unterminated value: the tag is malformed
        }
        out.push((name.to_owned(), tag[value_start..i].to_owned(), value_start, i));
        i += 1;
    }
    out
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Index of the `(` opening a method's parameter list inside a declaration
/// fragment. `(`s that belong to annotation arguments — `@Anno(…)` — are
/// skipped so `@DataPermission(column = "id") int count()` finds the paren
/// after `count`, not the one after `DataPermission`.
fn method_paren(stmt: &str) -> Option<usize> {
    let mut rest = stmt;
    let mut base = 0;
    while let Some(i) = rest.find('(') {
        // Is the token right before this `(` an annotation name (`@Anno`, `@a.b.Anno`)?
        let before = rest[..i].trim_end();
        let annotation = before.rfind('@').is_some_and(|a| before[a + 1..].chars().all(|c| ident(c) || c == '.'));
        if !annotation {
            return Some(base + i);
        }
        // Skip the annotation's argument list (it may nest and hold literals).
        let mut depth = 1usize;
        let mut end = None;
        let mut chars = rest[i + 1..].char_indices().peekable();
        while let Some((j, c)) = chars.next() {
            match c {
                '"' | '\'' => {
                    for (_, d) in chars.by_ref() {
                        if d == c {
                            break;
                        }
                    }
                }
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1 + j + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let skip = end?;
        base += skip;
        rest = &rest[skip..];
    }
    None
}

/// The interface's name and methods (with their parameter names).
pub fn parse_java(text: &str) -> MapperJava {
    let package =
        text.lines().find_map(|l| l.trim().strip_prefix("package ").map(|p| p.trim_end_matches(';').trim().to_owned()));
    let Some(i) = text.find("interface ") else { return MapperJava::default() };
    let simple: String = text[i + 10..].trim_start().chars().take_while(|c| ident(*c)).collect();
    let name = Some(match package {
        Some(p) => format!("{p}.{simple}"),
        None => simple,
    });
    let body_start = text[i..].find('{').map_or(text.len(), |b| i + b + 1);
    let mut methods = Vec::new();
    let mut depth = 0;
    let mut stmt_start = body_start;
    let bytes: Vec<(usize, char)> = text[body_start..].char_indices().map(|(k, c)| (body_start + k, c)).collect();
    for (k, c) in bytes {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => break,
            '}' => {
                depth -= 1;
                stmt_start = k + 1;
            }
            ';' if depth == 0 => {
                let stmt = &text[stmt_start..k];
                if let Some(paren) = method_paren(stmt) {
                    let before = stmt[..paren].trim_end();
                    let name_start = before.rfind(|c: char| !ident(c)).map_or(0, |p| p + 1);
                    let method = &before[name_start..];
                    if !method.is_empty() && !before.contains(" default ") {
                        let s = stmt_start + name_start;
                        let params = stmt[paren + 1..stmt.rfind(')').unwrap_or(stmt.len())]
                            .split(',')
                            .filter_map(|p| {
                                // `@Param("id") long id` → the @Param name wins.
                                if let Some(q) = p.find("@Param(\"") {
                                    return p[q + 8..].split('"').next().map(str::to_owned);
                                }
                                p.split_whitespace().last().map(|n| n.trim_matches(|c: char| !ident(c)).to_owned())
                            })
                            .filter(|n| !n.is_empty())
                            .collect();
                        methods.push((
                            Named { name: method.to_owned(), range: range_of(text, s, s + method.len()) },
                            params,
                        ));
                    }
                }
                stmt_start = k + 1;
            }
            _ => {}
        }
    }
    MapperJava { name, methods }
}

fn project_of(path: &str) -> Option<Project> {
    host::projects()
        .ok()?
        .into_iter()
        .filter(|p| p.root().is_ok_and(|r| path.starts_with(&r)))
        .max_by_key(|p| p.root().map(|r| r.len()).unwrap_or(0))
}

/// A `**/*.xml` scan in progress: the paths to read, the cursor, what it
/// found. Lives in the plugin instance, so it survives between calls.
struct Scan {
    root: String,
    paths: Vec<String>,
    at: usize,
    map: BTreeMap<String, String>,
}

#[derive(Default)]
pub struct Navigator {
    /// project root → (built at, namespace → XML path) of a completed scan.
    /// While fresh it also answers absence: the namespace has no XML.
    xml_index: BTreeMap<String, (Instant, BTreeMap<String, String>)>,
    /// The scan the next provider call continues, one `SCAN_SLICE` at a time.
    scanning: Option<Scan>,
    /// "root\u{0}namespace" confirmed without XML, and when.
    no_xml: BTreeMap<String, Instant>,
    /// namespace → (resolved at, `**/a/b/Name.java`); the file is re-read on
    /// every use, so edits keep their targets. `None`: no such interface.
    java_path: BTreeMap<String, (Instant, Option<String>)>,
    /// class name → (resolved at, `**/a/b/C.java` paths), for the class
    /// attributes: fully-qualified names by package path, simple names by
    /// wherever a same-named file is (a MyBatis alias, or a duplicated name:
    /// the picker then shows every hit).
    class_path: BTreeMap<String, (Instant, Vec<String>)>,
    type_paths: navigation::Paths,
}

/// Reads `path` and returns it with its statements when it carries `namespace`.
fn read_matching(project: &Project, path: &str, namespace: &str) -> Option<(String, MapperXml)> {
    let text = project.read_file(path).ok()?;
    let xml = parse_xml(&text);
    (xml.namespace.as_deref() == Some(namespace)).then_some((path.to_owned(), xml))
}

impl Navigator {
    /// The XML mapping `namespace`. The naming convention answers almost
    /// every project outright; everything else waits for the scan below,
    /// which a fresh, completed index then answers both ways.
    fn xml_for(&mut self, project: &Project, namespace: &str) -> Option<(String, MapperXml)> {
        let root = project.root().ok()?;
        let key = format!("{root}\u{0}{namespace}");
        if let Some((at, map)) = self.xml_index.get(&root)
            && at.elapsed() < REMEMBER
            && let Some(path) = map.get(namespace).cloned()
        {
            if let Some(hit) = read_matching(project, &path, namespace) {
                return Some(hit);
            }
            // The recorded file no longer carries the namespace: rebuild.
            self.xml_index.remove(&root);
            self.scanning = None;
        }
        if let Some(hit) = self.guess_xml(project, namespace) {
            if let Some((_, map)) = self.xml_index.get_mut(&root) {
                map.insert(namespace.to_owned(), hit.0.clone());
            }
            self.no_xml.remove(&key);
            return Some(hit);
        }
        if self.xml_index.get(&root).is_some_and(|(at, _)| at.elapsed() < REMEMBER)
            || self.no_xml.get(&key).is_some_and(|at| at.elapsed() < REMEMBER)
        {
            return None; // a fresh, completed index (or an earlier scan) knows it is not there
        }
        let done = self.scan_step(project, &root, SCAN_SLICE);
        let found = self
            .xml_index
            .get(&root)
            .and_then(|(_, map)| map.get(namespace).cloned())
            .and_then(|path| read_matching(project, &path, namespace));
        if done && found.is_none() {
            self.no_xml.insert(key, Instant::now());
        }
        found
    }

    /// `com.acme.UserMapper` → the `**/UserMapper.xml` carrying the
    /// namespace: the layout every project uses, a file or two to read.
    fn guess_xml(&self, project: &Project, namespace: &str) -> Option<(String, MapperXml)> {
        let simple = namespace.rsplit('.').next()?;
        let pattern = format!("**/{simple}.xml");
        for path in project.find_files(&pattern, GUESS_LIMIT).ok()? {
            if let Some(hit) = read_matching(project, &path, namespace) {
                return Some(hit);
            }
        }
        None
    }

    /// Reads the next slice of the `**/*.xml` walk (the paths were listed
    /// once when the scan started) and installs the result as the root's
    /// index when it completes. Always at least one file per call, so the
    /// scan moves forward even with no budget left. Returns whether it
    /// completed.
    fn scan_step(&mut self, project: &Project, root: &str, budget: Duration) -> bool {
        if !self.scanning.as_ref().is_some_and(|s| s.root == root) {
            let paths = project.find_files("**/*.xml", MAX_XML).unwrap_or_default();
            self.scanning = Some(Scan { root: root.to_owned(), paths, at: 0, map: BTreeMap::new() });
        }
        let scan = self.scanning.as_mut().expect("just set");
        let start = Instant::now();
        loop {
            let Some(path) = scan.paths.get(scan.at).cloned() else { break };
            scan.at += 1;
            if let Ok(text) = project.read_file(&path)
                && text.contains("<mapper")
                && let Some(ns) = parse_xml(&text).namespace
            {
                scan.map.insert(ns, path);
            }
            if start.elapsed() >= budget {
                return false;
            }
        }
        let Scan { map, .. } = self.scanning.take().expect("in progress");
        self.xml_index.insert(root.to_owned(), (Instant::now(), map));
        true
    }

    /// The interface `namespace` names (`com.acme.UserMapper` → `**/com/acme/UserMapper.java`).
    fn java_for(&mut self, project: &Project, namespace: &str) -> Option<(String, MapperJava)> {
        let path = match self.java_path.get(namespace) {
            Some((at, path)) if at.elapsed() < REMEMBER => path.clone(),
            _ => {
                let rel = format!("**/{}.java", namespace.replace('.', "/"));
                let found = project.find_files(&rel, 1).ok()?.into_iter().next();
                self.java_path.insert(namespace.to_owned(), (Instant::now(), found.clone()));
                found
            }
        }?;
        let java = parse_java(&project.read_file(&path).ok()?);
        (java.name.as_deref() == Some(namespace)).then_some((path, java))
    }

    /// The references an XML document holds, each with its jump targets:
    /// `resultMap="…"` and `refid="…"` to their definitions (here, or in the
    /// namespace's file), class attributes to the Java files naming them.
    fn xml_refs(&mut self, doc: &Document) -> Vec<(Named, Vec<Location>)> {
        let xml = parse_xml(&doc.text);
        let project = project_of(&doc.path);
        let mut out = Vec::new();
        for r in &xml.map_refs {
            out.push((r.clone(), self.ref_targets(project.as_ref(), &doc.path, &xml, &r.name, false)));
        }
        for r in &xml.sql_refs {
            out.push((r.clone(), self.ref_targets(project.as_ref(), &doc.path, &xml, &r.name, true)));
        }
        if let Some(project) = &project {
            for c in &xml.classes {
                out.push((c.clone(), self.class_targets(project, &c.name)));
            }
        }
        out
    }

    /// Where `reference` (`id`, or `namespace.id`) points: the `<resultMap>`
    /// (`sql: false`) or `<sql>` (`sql: true`) with that id, in this document
    /// or in the namespace's.
    fn ref_targets(
        &mut self,
        project: Option<&Project>,
        doc_path: &str,
        xml: &MapperXml,
        reference: &str,
        sql: bool,
    ) -> Vec<Location> {
        let (ns, id) = match reference.rsplit_once('.') {
            Some((ns, id)) if !id.is_empty() => {
                if xml.namespace.as_deref() == Some(ns) { (None, id) } else { (Some(ns), id) }
            }
            _ => (None, reference),
        };
        if let Some(ns) = ns {
            let Some(project) = project else { return Vec::new() };
            return self
                .xml_for(project, ns)
                .and_then(|(path, x)| defs_of(&x, sql).iter().find(|d| d.name == id).map(|d| Location { path, range: d.range }))
                .into_iter()
                .collect();
        }
        defs_of(xml, sql).iter().find(|d| d.name == id).map(|d| vec![Location { path: doc_path.to_owned(), range: d.range }]).unwrap_or_default()
    }

    /// The Java files naming `name`. Primitives and MyBatis' own aliases
    /// (`map`, `int`, `string`, …) start lowercase and name no file here; a
    /// simple name resolves wherever a same-named file is — the alias table
    /// lives in configuration the plugin cannot see, so several hits are all
    /// offered and the picker decides. A qualified name also tries its parent
    /// (`com.x.Outer.Inner` → `com/x/Outer.java`).
    fn class_targets(&mut self, project: &Project, name: &str) -> Vec<Location> {
        // A simple name starting lowercase is a primitive or one of MyBatis'
        // own aliases (`int`, `map`, …); a dotted name is a package path,
        // whatever case it starts with, and simply finds no file when wrong.
        if !name.contains('.') && !name.starts_with(|c: char| c.is_ascii_uppercase()) {
            return Vec::new();
        }
        let key = format!("{}\u{0}{name}", project.root().unwrap_or_default());
        if let Some((at, paths)) = self.class_path.get(&key)
            && at.elapsed() < REMEMBER
        {
            return self.locate_classes(project, paths);
        }
        let mut globs = vec![format!("**/{}.java", name.replace('.', "/"))];
        if let Some((head, _)) = name.rsplit_once('.') {
            globs.push(format!("**/{}.java", head.replace('.', "/")));
        }
        let mut paths = Vec::new();
        for glob in globs {
            paths = project.find_files(&glob, 8).unwrap_or_default();
            if !paths.is_empty() {
                break;
            }
        }
        self.class_path.insert(key, (Instant::now(), paths.clone()));
        self.locate_classes(project, &paths)
    }

    /// Each path's declaration range (`class X` / `interface X` / `enum X` /
    /// `record X`), re-read per call so edits keep their targets.
    fn locate_classes(&self, project: &Project, paths: &[String]) -> Vec<Location> {
        paths
            .iter()
            .filter_map(|p| {
                let text = project.read_file(p).ok()?;
                let simple = p.rsplit('/').next()?.trim_end_matches(".java");
                Some(Location { path: p.clone(), range: decl_range(&text, simple) })
            })
            .collect()
    }
    fn links(&mut self, doc: &Document) -> Vec<(Named, Location, String)> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        if doc.language == "xml" {
            let xml = parse_xml(&doc.text);
            let Some(ns) = xml.namespace else { return Vec::new() };
            let Some((path, java)) = self.java_for(&project, &ns) else { return Vec::new() };
            let simple = ns.rsplit('.').next().unwrap_or(&ns).to_owned();
            xml.statements
                .into_iter()
                .filter_map(|s| {
                    let (m, _) = java.methods.iter().find(|(m, _)| m.name == s.name)?;
                    let tip = format!("{simple}.{}()", m.name);
                    Some((s, Location { path: path.clone(), range: m.range }, tip))
                })
                .collect()
        } else {
            let java = parse_java(&doc.text);
            let Some(name) = java.name else { return Vec::new() };
            let Some((path, xml)) = self.xml_for(&project, &name) else { return Vec::new() };
            let file = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
            java.methods
                .into_iter()
                .filter_map(|(m, _)| {
                    let s = xml.statements.iter().find(|s| s.name == m.name)?;
                    Some((m, Location { path: path.clone(), range: s.range }, format!("{file}: {}", s.name)))
                })
                .collect()
        }
    }
}

fn contains(r: Range, p: Position) -> bool {
    r.start <= p && p <= r.end
}

/// The `<resultMap>` (`sql: false`) or `<sql>` (`sql: true`) definitions.
fn defs_of(x: &MapperXml, sql: bool) -> &Vec<Named> {
    if sql { &x.sqls } else { &x.result_maps }
}

/// Where `simple`'s declaration is in `text`: right after the `class`,
/// `interface`, `enum` or `record` keyword. The file top when there is none.
fn decl_range(text: &str, simple: &str) -> Range {
    for (i, _) in text.match_indices(simple) {
        let before = text[..i].trim_end();
        let word = before.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or_default();
        if matches!(word, "class" | "interface" | "enum" | "record") {
            return range_of(text, i, i + simple.len());
        }
    }
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

impl Providers for Navigator {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let icon = if doc.language == "xml" { "implementing" } else { "implemented" };
        self.links(doc)
            .into_iter()
            .map(|(n, target, tip)| LineMarker {
                line: n.range.start.line,
                icon: Some(icon.into()),
                tooltip: format!("Go to {tip}"),
                target: Some(target),
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        // Java declarations must keep the IDE's native Show Usages behavior.
        if doc.language == "java" {
            return Vec::new();
        }
        if doc.language == "xml" {
            let xml = parse_xml(&doc.text);
            if let Some(project) = project_of(&doc.path) {
                if let Some(statement) = xml.statements.iter().find(|s| contains(s.range, at))
                    && let Some(namespace) = &xml.namespace
                {
                    let Some((path, java)) = self.java_for(&project, namespace) else { return Vec::new() };
                    return java
                        .methods
                        .into_iter()
                        .find(|(m, _)| m.name == statement.name)
                        .map(|(m, _)| Location { path, range: m.range })
                        .into_iter()
                        .collect();
                }
                if let Some(cursor) = offset(&doc.text, at)
                    && let Some(mapping) = xml::mapping_at(&doc.text, cursor)
                {
                    return navigation::mapping_definition(&project, &mut self.type_paths, &mapping);
                }
                if let Some(namespace) = &xml.namespace_ref
                    && contains(namespace.range, at)
                {
                    return self.class_targets(&project, &namespace.name);
                }
                if let Some(cursor) = offset(&doc.text, at)
                    && let Some(expressions) = xml::expressions_at(&doc.text, cursor)
                    && let Some(namespace) = &xml.namespace
                {
                    return navigation::definitions(&project, &mut self.type_paths, namespace, &doc.path, &expressions);
                }
            }
        }
        let mut found = self
            .links(doc)
            .into_iter()
            .filter(|(n, ..)| contains(n.range, at))
            .map(|(_, l, _)| l)
            .collect::<Vec<_>>();
        // References inside the XML: resultMap and include targets, classes.
        found.extend(
            self.xml_refs(doc).into_iter().filter(|(n, _)| contains(n.range, at)).flat_map(|(_, l)| l),
        );
        found
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if doc.language != "xml" {
            return Vec::new();
        }
        let Some(cursor) = offset(&doc.text, at) else { return Vec::new() };
        let before = &doc.text[..cursor];
        let xml = parse_xml(&doc.text);
        let Some(ns) = xml.namespace.clone() else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        if let Some(completion) = xml::completion_at(&doc.text, cursor) {
            return navigation::completions(&project, &mut self.type_paths, &ns, &doc.path, &completion);
        }
        let Some((_, java)) = self.java_for(&project, &ns) else { return Vec::new() };
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let line = &before[line_start..];
        // Inside `id="…` of a statement tag: the interface's unmapped methods.
        if let Some(i) = line.rfind("id=\"")
            && !line[i + 4..].contains('"')
            && STATEMENTS.iter().any(|t| line.contains(&format!("<{t}")))
        {
            let typed = &line[i + 4..];
            let start = Position::new(at.line, at.character - typed.encode_utf16().count() as u32);
            return java
                .methods
                .iter()
                .filter(|(m, _)| m.name.starts_with(typed) && !xml.statements.iter().any(|s| s.name == m.name))
                .map(|(m, _)| CompletionItem {
                    replace: Some(Range::new(start, at)),
                    detail: Some(ns.rsplit('.').next().unwrap_or(&ns).into()),
                    ..CompletionItem::new(&m.name, CompletionKind::Method)
                })
                .collect();
        }
        Vec::new()
    }
}

impl Plugin for Navigator {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Navigator, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const JAVA: &str = "package com.acme;\n\nimport org.apache.ibatis.annotations.Param;\n\npublic interface UserMapper {\n    User findById(@Param(\"id\") long id);\n\n    int insert(User user);\n\n    default void helper() {}\n\n    List<User> search(String name, int limit);\n}\n";
    const XML: &str = "<?xml version=\"1.0\"?>\n<mapper namespace=\"com.acme.UserMapper\">\n  <select id=\"findById\" resultType=\"User\">\n    select * from users where id = #{id}\n  </select>\n  <insert id=\"insert\">\n    insert into users values (#{user})\n  </insert>\n  <select id=\"\">\n  </select>\n</mapper>\n";

    #[test]
    fn parses_both_sides() {
        let java = parse_java(JAVA);
        assert_eq!(java.name.as_deref(), Some("com.acme.UserMapper"));
        let names: Vec<_> = java.methods.iter().map(|(m, p)| (m.name.as_str(), p.clone())).collect();
        assert_eq!(
            names,
            [
                ("findById", vec!["id".to_owned()]),
                ("insert", vec!["user".into()]),
                ("search", vec!["name".into(), "limit".into()])
            ]
        );
        assert_eq!(java.methods[0].0.range, Range::new(Position::new(5, 9), Position::new(5, 17)));
        let xml = parse_xml(XML);
        assert_eq!(xml.namespace.as_deref(), Some("com.acme.UserMapper"));
        assert_eq!(xml.statements.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["findById", "insert", ""]);
        assert_eq!(xml.statements[0].range, Range::new(Position::new(2, 14), Position::new(2, 22)));
    }

    #[test]
    fn xml_attribute_ranges_preserve_utf16_after_multiline_tags_comments_and_cdata() {
        let text = "<mapper namespace=\"app.Mapper\">\r\n<!--中文😀 <fake/>-->\r\n<![CDATA[😀 <fake/>]]>\r\n  <select\r\n    note=\"😀\" id=\"first\">text</select><select id=\"last\"/>\r\n</mapper>";
        for tag in xml::tags(text) {
            for (name, value, start, end) in &tag.attrs {
                let attribute = tag.attr(text, name).unwrap();
                assert_eq!(attribute.name, *value);
                assert_eq!(attribute.range, range_of(text, *start, *end), "{name}={value}");
            }
        }
    }

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserMapper.java", JAVA);
        host.add_file("src/main/resources/mapper/UserMapper.xml", XML);
        host.add_file("pom.xml", "<project/>");
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    #[test]
    fn java_definitions_leave_native_navigation_to_the_ide() {
        let _host = setup();
        let mut n = Navigator::default();
        let java = doc("/project/src/main/java/com/acme/UserMapper.java", "java", JAVA);
        for (method, _) in parse_java(JAVA).methods {
            assert!(
                n.definition(&java, method.range.start).is_empty(),
                "{} must keep native Java navigation",
                method.name
            );
        }
    }

    #[test]
    fn navigates_both_ways() {
        let _host = setup();
        let mut n = Navigator::default();
        let java = doc("/project/src/main/java/com/acme/UserMapper.java", "java", JAVA);
        let markers = n.line_markers(&java);
        assert_eq!(markers.iter().map(|m| m.line).collect::<Vec<_>>(), [5, 7], "search has no statement");
        assert_eq!(markers[0].tooltip, "Go to UserMapper.xml: findById");
        let target = markers[0].target.as_ref().unwrap();
        assert_eq!(
            (target.path.as_str(), target.range.start),
            ("/project/src/main/resources/mapper/UserMapper.xml", Position::new(2, 14))
        );

        let xml = doc("/project/src/main/resources/mapper/UserMapper.xml", "xml", XML);
        let to_java = n.definition(&xml, Position::new(5, 15));
        assert_eq!(to_java.len(), 1);
        assert_eq!((to_java[0].path.ends_with("UserMapper.java"), to_java[0].range.start), (true, Position::new(7, 8)));
        assert!(n.definition(&xml, Position::new(3, 5)).is_empty(), "not on an id");
        assert_eq!(n.line_markers(&xml)[0].tooltip, "Go to UserMapper.findById()");
    }

    /// A mapper XML not named after the interface is found by the fallback
    /// scan, which the first provider call completes a slice of.
    #[test]
    fn unconventional_xml_is_found_by_the_scan() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserMapper.java", JAVA);
        host.add_file("src/main/resources/sql/Queries.xml", XML);
        host.add_file("pom.xml", "<project/>");
        let mut n = Navigator::default();
        let java = doc("/project/src/main/java/com/acme/UserMapper.java", "java", JAVA);
        let markers = n.line_markers(&java);
        assert_eq!(markers.iter().map(|m| m.line).collect::<Vec<_>>(), [5, 7]);
        assert!(markers[0].target.as_ref().unwrap().path.ends_with("Queries.xml"));
    }

    /// The scan always reads at least one file per call — a slice budget of
    /// zero still advances — and installs its map when it runs out of paths.
    #[test]
    fn scan_advances_one_file_per_call() {
        let host = MockHost::install();
        host.add_file("a.xml", XML);
        host.add_file("b.xml", XML);
        host.add_file("c.xml", "<project/>");
        let mut n = Navigator::default();
        let project = project_of("/project/a.xml").unwrap();
        let root = project.root().unwrap();
        let mut calls = 0;
        while !n.scan_step(&project, &root, Duration::from_millis(0)) {
            calls += 1;
            assert!(calls < 10, "the scan does not finish");
        }
        assert_eq!(calls, 3, "one file per zero-budget call, three files");
        assert_eq!(n.xml_index[&root].1.keys().collect::<Vec<_>>(), ["com.acme.UserMapper"]);
    }

    /// An interface without XML is answered from memory on the next ask —
    /// opening unrelated interfaces must not rescan — until the memory
    /// expires; the naming convention still answers files added later.
    #[test]
    fn absent_namespaces_are_remembered() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/OrderDao.java", &JAVA.replace("UserMapper", "OrderDao"));
        host.add_file("src/main/resources/mapper/Other.xml", &XML.replace("com.acme.UserMapper", "com.acme.Other"));
        host.add_file("pom.xml", "<project/>");
        let mut n = Navigator::default();
        let dao = doc("/project/src/main/java/com/acme/OrderDao.java", "java", &JAVA.replace("UserMapper", "OrderDao"));
        assert!(n.line_markers(&dao).is_empty());
        let key = format!("/project\u{0}com.acme.OrderDao");
        assert!(n.no_xml.contains_key(&key), "the miss is remembered");
        assert!(n.scanning.is_none(), "the scan completed");
        assert!(n.line_markers(&dao).is_empty(), "answered from memory");
    }

    const REFS_XML: &str = concat!(
        "<?xml version=\"1.0\"?>\n",
        "<mapper namespace=\"com.acme.UserMapper\">\n",
        "  <resultMap id=\"userMap\" type=\"com.acme.User\">\n",
        "    <id property=\"id\" column=\"id\"/>\n",
        "  </resultMap>\n",
        "  <sql id=\"cols\">id, name</sql>\n",
        "  <select id=\"findById\" resultMap=\"userMap\" parameterType=\"long\" resultType=\"com.acme.User\">\n",
        "    select <include refid=\"cols\"/> from users where id = #{id}\n",
        "  </select>\n",
        "  <select id=\"other\" resultMap=\"com.acme.OtherMapper.otherMap\" resultType=\"User\"/>\n",
        "</mapper>\n"
    );
    const OTHER_XML: &str =
        "<?xml version=\"1.0\"?>\n<mapper namespace=\"com.acme.OtherMapper\">\n  <resultMap id=\"otherMap\" type=\"User\"/>\n</mapper>\n";
    const USER_JAVA: &str = "package com.acme;\n\npublic class User {\n    private long id;\n}\n";

    /// Column of `what` inside `line` of a fixture, for asking positions.
    fn at(text: &str, line: usize, what: &str) -> Position {
        let l = text.lines().nth(line).unwrap();
        Position::new(line as u32, l.find(what).unwrap() as u32)
    }

    fn refs_setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/UserMapper.java", JAVA);
        host.add_file("src/main/java/com/acme/User.java", USER_JAVA);
        host.add_file("src/main/resources/mapper/Refs.xml", REFS_XML);
        host.add_file("src/main/resources/mapper/Other.xml", OTHER_XML);
        host.add_file("pom.xml", "<project/>");
        host
    }

    #[test]
    fn statement_id_navigation_does_not_resolve_unrelated_class_attributes() {
        let _host = refs_setup();
        let mut n = Navigator::default();
        let xml = doc("/project/src/main/resources/mapper/Refs.xml", "xml", REFS_XML);
        let targets = n.definition(&xml, at(REFS_XML, 6, "findById"));
        assert_eq!(targets.len(), 1);
        assert!(targets[0].path.ends_with("UserMapper.java"));
        assert_eq!(targets[0].range.start, at(JAVA, 5, "findById"));
        assert!(n.class_path.is_empty(), "a statement id must not resolve every class in the XML");
    }

    #[test]
    fn parses_definitions_and_references() {
        let xml = parse_xml(REFS_XML);
        assert_eq!(xml.result_maps.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["userMap"]);
        assert_eq!(xml.sqls.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["cols"]);
        assert_eq!(
            xml.map_refs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["userMap", "com.acme.OtherMapper.otherMap"]
        );
        assert_eq!(xml.sql_refs.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["cols"]);
        assert_eq!(
            xml.classes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            ["com.acme.User", "com.acme.User", "long", "User"]
        );
        assert_eq!(xml.result_maps[0].range.start, at(REFS_XML, 2, "userMap"));
    }

    /// Every reference kind jumps: local `resultMap` and `refid` to their
    /// definitions here, a namespace-qualified one to the other file, class
    /// attributes (qualified and simple) to the Java class; `parameterType`
    /// of a primitive goes nowhere.
    #[test]
    fn jumps_references_and_classes() {
        let _host = refs_setup();
        let mut n = Navigator::default();
        let doc = doc("/project/src/main/resources/mapper/Refs.xml", "xml", REFS_XML);

        let map = n.definition(&doc, at(REFS_XML, 6, "userMap"));
        assert_eq!((map.len(), map[0].path.as_str(), map[0].range.start), (1, doc.path.as_str(), at(REFS_XML, 2, "userMap")));

        let sql = n.definition(&doc, at(REFS_XML, 7, "cols"));
        assert_eq!((sql.len(), sql[0].range.start), (1, at(REFS_XML, 5, "cols")));

        let other = n.definition(&doc, at(REFS_XML, 9, "com.acme.OtherMapper.otherMap"));
        assert_eq!((other.len(), other[0].path.ends_with("Other.xml"), other[0].range.start), (1, true, at(OTHER_XML, 2, "otherMap")));

        let fq = n.definition(&doc, at(REFS_XML, 6, "com.acme.User"));
        assert_eq!((fq.len(), fq[0].path.ends_with("User.java"), fq[0].range.start), (1, true, at(USER_JAVA, 2, "User")));

        let simple = n.definition(&doc, at(REFS_XML, 9, "User"));
        assert_eq!(simple.len(), 1, "the unique User.java");
        assert!(simple[0].path.ends_with("User.java"));

        assert!(n.definition(&doc, at(REFS_XML, 6, "long")).is_empty(), "a primitive names no class");
    }

    /// A simple name several files carry is ambiguous: every hit is offered
    /// and the picker decides, since the alias table is configuration.
    #[test]
    fn ambiguous_names_offer_every_hit() {
        let host = MockHost::install();
        let text = "<?xml version=\"1.0\"?>\n<mapper namespace=\"com.acme.Mapper\">\n  <select id=\"q\" resultType=\"Foo\"/>\n</mapper>\n";
        host.add_file("src/a/Foo.java", "package a;\n\npublic class Foo {}\n");
        host.add_file("src/b/Foo.java", "package b;\n\npublic class Foo {}\n");
        host.add_file("src/main/resources/mapper/Ambiguous.xml", text);
        let mut n = Navigator::default();
        let doc = doc("/project/src/main/resources/mapper/Ambiguous.xml", "xml", text);
        assert_eq!(n.definition(&doc, at(text, 2, "Foo")).len(), 2);
    }

    #[test]
    fn method_annotations_do_not_swallow_the_method_name() {
        // A method-level annotation with arguments: `stmt.find('(')` would land
        // inside `@DataPermission(…)` and read `DataPermission` as the name.
        let java = parse_java(
            "package com.acme;\n\npublic interface SecuredMapper {\n    @DataPermission(column = \"id\", scope = (@Scope(\"self\")))\n    List<User> findAll(@Param(\"org\") String org);\n\n    @org.acme.Check(value = \"(nested)\")\n    int count();\n}\n",
        );
        let names: Vec<_> = java.methods.iter().map(|(m, p)| (m.name.as_str(), p.clone())).collect();
        assert_eq!(names, [("findAll", vec!["org".to_owned()]), ("count", Vec::<String>::new())]);
    }

    #[test]
    fn completes_ids_and_parameters() {
        let _host = setup();
        let mut n = Navigator::default();
        let xml = doc("/project/src/main/resources/mapper/UserMapper.xml", "xml", XML);
        let ids = n.completion(&xml, Position::new(8, 14), None);
        assert_eq!(ids.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["search"], "unmapped methods only");
        let text = XML.replace("#{id}", "#{");
        let xml = doc("/project/src/main/resources/mapper/UserMapper.xml", "xml", &text);
        let params = n.completion(&xml, Position::new(3, 38), None);
        assert_eq!(params.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["id"]);
    }
}
