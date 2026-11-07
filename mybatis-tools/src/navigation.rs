use crate::java::{self, Java, Parameter, Type};
use crate::xml::Expression;
use crate::{Location, Project, REMEMBER};
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub(crate) type Paths = BTreeMap<(String, String), (Instant, Option<String>)>;

#[derive(Clone)]
struct Value {
    ty: Type,
    location: Option<Location>,
}

struct Signature {
    path: String,
    params: Vec<Parameter>,
    implicit: Option<Type>,
}

struct Scope {
    names: BTreeMap<String, Option<Value>>,
    implicit: Option<Value>,
    visible: BTreeSet<String>,
}

struct Resolver<'a> {
    project: &'a Project,
    root: String,
    paths: &'a mut Paths,
    units: BTreeMap<String, Java>,
    started: Instant,
}

impl Resolver<'_> {
    fn path(&mut self, name: &str) -> Option<String> {
        if self.started.elapsed() > Duration::from_millis(200) {
            return None;
        }
        let key = (self.root.clone(), name.to_owned());
        if let Some((at, path)) = self.paths.get(&key)
            && at.elapsed() < REMEMBER
        {
            return path.clone();
        }
        let paths = self.project.find_files(&format!("**/{}.java", name.replace('.', "/")), 2).unwrap_or_default();
        let path = (paths.len() == 1).then(|| paths[0].clone());
        self.paths.insert(key, (Instant::now(), path.clone()));
        path
    }

    fn source(&mut self, ty: &Type) -> Option<(String, Java)> {
        let path = self.path(&ty.name)?;
        if let Some(unit) = self.units.get(&path) {
            return Some((path, unit.clone()));
        }
        let unit = java::parse(&self.project.read_file(&path).ok()?);
        self.units.insert(path.clone(), unit.clone());
        Some((path, unit))
    }

    fn qualify(&mut self, unit: &Java, ty: &Type, bindings: &BTreeMap<String, Type>) -> Type {
        if bindings.contains_key(&ty.name) {
            return ty.substitute(bindings);
        }
        let mut name = ty.name.clone();
        let head = name.split('.').next().unwrap_or_default();
        if !head.is_empty() && head.starts_with(char::is_uppercase) {
            if let Some(import) = unit.imports.iter().find(|s| s.rsplit('.').next() == Some(head)) {
                name = format!("{import}{}", &name[head.len()..]);
            } else {
                let local = if unit.package.is_empty() { name.clone() } else { format!("{}.{}", unit.package, name) };
                if self.path(&local).is_some() {
                    name = local;
                } else {
                    let candidates: Vec<_> = unit
                        .imports
                        .iter()
                        .filter_map(|s| s.strip_suffix(".*"))
                        .map(|p| format!("{p}.{name}"))
                        .filter(|n| self.path(n).is_some())
                        .collect();
                    if candidates.len() == 1 {
                        name = candidates[0].clone();
                    }
                }
            }
        }
        Type { name, args: ty.args.iter().map(|t| self.qualify(unit, t, bindings)).collect(), array: ty.array }
    }

    fn bindings(unit: &Java, ty: &Type) -> BTreeMap<String, Type> {
        unit.variables.iter().cloned().zip(ty.args.iter().cloned()).collect()
    }

    fn methods(&mut self, ty: &Type, name: &str, seen: &mut BTreeSet<String>) -> Vec<Signature> {
        if seen.len() >= 16 || !seen.insert(ty.name.clone()) {
            return Vec::new();
        }
        let Some((path, unit)) = self.source(ty) else {
            // MyBatis Plus supplies insert(T entity) in a dependency, outside
            // project.read. Its declared generic argument still identifies T.
            if ty.name == "com.baomidou.mybatisplus.core.mapper.BaseMapper"
                && name == "insert"
                && let Some(entity) = ty.args.first()
            {
                return vec![Signature { path: String::new(), params: Vec::new(), implicit: Some(entity.clone()) }];
            }
            return Vec::new();
        };
        let bindings = Self::bindings(&unit, ty);
        let mut found = Vec::new();
        for method in unit.methods.iter().filter(|m| m.name.name == name) {
            let params = method
                .params
                .iter()
                .map(|p| Parameter {
                    name: p.name.clone(),
                    alias: p.alias.clone(),
                    ty: self.qualify(&unit, &p.ty, &bindings),
                })
                .collect();
            found.push(Signature { path: path.clone(), params, implicit: None });
        }
        for parent in &unit.parents {
            let parent = self.qualify(&unit, parent, &bindings);
            found.extend(self.methods(&parent, name, seen));
        }
        found
    }

    fn property(&mut self, ty: &Type, name: &str, seen: &mut BTreeSet<String>) -> Option<Value> {
        if seen.len() >= 16 || !seen.insert(ty.name.clone()) {
            return None;
        }
        let (path, unit) = self.source(ty)?;
        let bindings = Self::bindings(&unit, ty);
        let field = unit.fields.iter().find(|f| f.name.name == name);
        let capitalized = name.chars().next()?.to_uppercase().to_string() + &name[name.chars().next()?.len_utf8()..];
        let getter = unit.methods.iter().find(|m| {
            m.params.is_empty()
                && (m.name.name == format!("get{capitalized}") || m.name.name == format!("is{capitalized}"))
        });
        if let Some((named, ty)) = field.map(|f| (&f.name, &f.ty)).or_else(|| getter.map(|m| (&m.name, &m.returns))) {
            return Some(Value {
                ty: self.qualify(&unit, ty, &bindings),
                location: Some(Location { path, range: named.range }),
            });
        }
        for parent in &unit.parents {
            let parent = self.qualify(&unit, parent, &bindings);
            if let Some(value) = self.property(&parent, name, seen) {
                return Some(value);
            }
        }
        None
    }

    fn properties(&mut self, ty: &Type, seen: &mut BTreeSet<String>) -> BTreeMap<String, Type> {
        let mut out = BTreeMap::new();
        if seen.len() >= 16 || !seen.insert(ty.name.clone()) {
            return out;
        }
        let Some((_, unit)) = self.source(ty) else { return out };
        let bindings = Self::bindings(&unit, ty);
        for field in unit.fields.iter().filter(|f| !f.is_static) {
            out.insert(field.name.name.clone(), field.ty.substitute(&bindings));
        }
        for method in unit.methods.iter().filter(|m| !m.is_static && m.params.is_empty()) {
            let suffix = method.name.name.strip_prefix("get").filter(|_| method.returns.name != "void").or_else(|| {
                method
                    .name
                    .name
                    .strip_prefix("is")
                    .filter(|_| matches!(method.returns.name.as_str(), "boolean" | "Boolean" | "java.lang.Boolean"))
            });
            if let Some(suffix) = suffix.filter(|s| s.starts_with(char::is_uppercase)) {
                let mut chars = suffix.chars();
                let first = chars.next().unwrap();
                let name = if chars.clone().next().is_some_and(char::is_uppercase) {
                    suffix.to_owned()
                } else {
                    first.to_lowercase().to_string() + chars.as_str()
                };
                out.entry(name).or_insert_with(|| method.returns.substitute(&bindings));
            }
        }
        for parent in &unit.parents {
            let parent = self.qualify(&unit, parent, &bindings);
            for (name, ty) in self.properties(&parent, seen) {
                out.entry(name).or_insert(ty);
            }
        }
        out
    }

    fn value(
        &mut self,
        path: &[String],
        env: &BTreeMap<String, Option<Value>>,
        implicit: Option<&Value>,
    ) -> Option<Value> {
        let (head, tail) = path.split_first()?;
        let mut value = match env.get(head) {
            Some(value) => value.clone()?,
            None => self.property(&implicit?.ty, head, &mut BTreeSet::new())?,
        };
        for name in tail {
            value = self.property(&value.ty, name, &mut BTreeSet::new())?;
        }
        Some(value)
    }

    fn member_path(&mut self, ty: &Type, path: &[String]) -> Option<Value> {
        let (first, rest) = path.split_first()?;
        let mut value = self.property(ty, first, &mut BTreeSet::new())?;
        for name in rest {
            value = self.property(&value.ty, name, &mut BTreeSet::new())?;
        }
        Some(value)
    }

    fn mapping(&mut self, mapping: &crate::xml::Mapping) -> Option<Location> {
        let mut ty = Type::parse(&mapping.ty);
        for owner in &mapping.owners {
            if let Some(explicit) = &owner.ty {
                let unit = self.source(&ty).map(|(_, unit)| unit).unwrap_or_default();
                ty = self.qualify(&unit, &Type::parse(explicit), &BTreeMap::new());
            } else {
                let path = owner.property.as_ref()?.split('.').map(str::to_owned).collect::<Vec<_>>();
                ty = self.member_path(&ty, &path)?.ty;
                if owner.collection {
                    ty = ty.element()?;
                }
            }
        }
        self.member_path(&ty, &mapping.path)?.location
    }

    fn scope(&mut self, expression: &Expression, signature: &Signature, xml_path: &str) -> Scope {
        let mut env = BTreeMap::new();
        let mut visible = BTreeSet::new();
        let mut implicit = signature.implicit.clone().map(|ty| Value { ty, location: None });
        let single = signature.params.len() == 1;
        for (i, param) in signature.params.iter().enumerate() {
            let value = Value {
                ty: param.ty.clone(),
                location: Some(Location { path: signature.path.clone(), range: param.name.range }),
            };
            let alias = param.alias.as_deref().unwrap_or(&param.name.name);
            if !single || param.alias.is_some() {
                visible.insert(alias.to_owned());
                env.insert(alias.to_owned(), Some(value.clone()));
                env.entry(format!("param{}", i + 1)).or_insert(Some(value.clone()));
            } else {
                env.insert("_parameter".into(), Some(value.clone()));
                if param.ty.element().is_some()
                    && !matches!(param.ty.name.rsplit('.').next(), Some("Map" | "HashMap" | "LinkedHashMap"))
                {
                    let alias = if param.ty.array { "array" } else { "collection" };
                    visible.insert(alias.to_owned());
                    env.insert(alias.into(), Some(value.clone()));
                    if param.ty.name.rsplit('.').next() == Some("List") {
                        visible.insert("list".into());
                        env.insert("list".into(), Some(value.clone()));
                    }
                    env.insert(param.name.name.clone(), Some(value.clone()));
                    visible.insert(param.name.name.clone());
                } else if scalar(&param.ty) {
                    env.insert(param.name.name.clone(), Some(value.clone()));
                    visible.insert(param.name.name.clone());
                } else {
                    implicit = Some(value);
                }
            }
        }
        for each in &expression.loops {
            let collection: Vec<_> = each.collection.split('.').map(|s| s.trim().to_owned()).collect();
            let ty = self.value(&collection, &env, implicit.as_ref()).map(|v| v.ty);
            let item = ty
                .as_ref()
                .and_then(Type::element)
                .map(|ty| Value { ty, location: Some(Location { path: xml_path.into(), range: each.item.range }) });
            env.insert(each.item.name.clone(), item);
            visible.insert(each.item.name.clone());
            if let Some(index) = &each.index {
                visible.insert(index.name.clone());
                let ty = ty
                    .filter(|t| matches!(t.name.rsplit('.').next(), Some("Map" | "HashMap" | "LinkedHashMap")))
                    .and_then(|t| t.args.first().cloned())
                    .unwrap_or_else(|| Type::parse("int"));
                env.insert(
                    index.name.clone(),
                    Some(Value { ty, location: Some(Location { path: xml_path.into(), range: index.range }) }),
                );
            }
        }
        Scope { names: env, implicit, visible }
    }

    fn expression(&mut self, expression: &Expression, signature: &Signature, xml_path: &str) -> Option<Location> {
        let scope = self.scope(expression, signature, xml_path);
        self.value(&expression.path, &scope.names, scope.implicit.as_ref()).and_then(|v| v.location)
    }

    fn signatures(&mut self, mapper: &Type, expression: &Expression) -> Vec<Signature> {
        let mut signatures = self.methods(mapper, &expression.statement, &mut BTreeSet::new());
        if signatures.is_empty()
            && let Some(parameter_type) = &expression.parameter_type
        {
            let unit = self.source(mapper).map(|(_, u)| u).unwrap_or_default();
            let ty = self.qualify(&unit, &Type::parse(parameter_type), &BTreeMap::new());
            signatures.push(Signature { path: String::new(), params: Vec::new(), implicit: Some(ty) });
        }
        signatures
    }
}

fn scalar(ty: &Type) -> bool {
    matches!(
        ty.name.rsplit('.').next(),
        Some(
            "String"
                | "CharSequence"
                | "Boolean"
                | "Byte"
                | "Short"
                | "Integer"
                | "Long"
                | "Float"
                | "Double"
                | "Character"
                | "boolean"
                | "byte"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "char"
                | "BigDecimal"
                | "BigInteger"
                | "Date"
                | "LocalDate"
                | "LocalDateTime"
                | "Instant"
        )
    )
}

pub(crate) fn definitions(
    project: &Project,
    paths: &mut Paths,
    namespace: &str,
    xml_path: &str,
    expressions: &[Expression],
) -> Vec<Location> {
    let mut resolver = Resolver {
        project,
        root: project.root().unwrap_or_default(),
        paths,
        units: BTreeMap::new(),
        started: Instant::now(),
    };
    let mapper = Type::parse(namespace);
    let mut out = Vec::new();
    for expression in expressions {
        for signature in resolver.signatures(&mapper, expression) {
            if let Some(location) = resolver.expression(expression, &signature, xml_path)
                && !out.iter().any(|l: &Location| l.path == location.path && l.range == location.range)
            {
                out.push(location);
            }
        }
    }
    out
}

pub(crate) fn mapping_definition(project: &Project, paths: &mut Paths, mapping: &crate::xml::Mapping) -> Vec<Location> {
    let mut resolver = Resolver {
        project,
        root: project.root().unwrap_or_default(),
        paths,
        units: BTreeMap::new(),
        started: Instant::now(),
    };
    resolver.mapping(mapping).into_iter().collect()
}

pub(crate) fn completions(
    project: &Project,
    paths: &mut Paths,
    namespace: &str,
    xml_path: &str,
    completion: &crate::xml::Completion,
) -> Vec<CompletionItem> {
    let mut resolver = Resolver {
        project,
        root: project.root().unwrap_or_default(),
        paths,
        units: BTreeMap::new(),
        started: Instant::now(),
    };
    let mapper = Type::parse(namespace);
    let mut candidates = BTreeMap::new();
    for expression in &completion.expressions {
        for signature in resolver.signatures(&mapper, expression) {
            let scope = resolver.scope(expression, &signature, xml_path);
            let target = if expression.path.is_empty() {
                scope.implicit.clone()
            } else {
                resolver.value(&expression.path, &scope.names, scope.implicit.as_ref())
            };
            if let Some(target) = target {
                candidates.extend(resolver.properties(&target.ty, &mut BTreeSet::new()));
            }
            if expression.path.is_empty() {
                for (name, value) in scope.names {
                    if let Some(value) = value
                        && scope.visible.contains(&name)
                    {
                        candidates.insert(name, value.ty);
                    }
                }
            }
        }
    }
    candidates
        .into_iter()
        .filter(|(name, _)| name.starts_with(&completion.prefix))
        .map(|(name, ty)| CompletionItem {
            detail: Some(ty.label()),
            replace: Some(completion.replace),
            ..CompletionItem::new(&name, CompletionKind::Property)
        })
        .collect()
}
