pub mod request;
pub mod scan;

use openidea_plugin::views::{View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Plugin, host, register_plugin};
use request::{Draft, Settings};
use scan::Endpoint;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const VIEW: &str = "restful";
const FIND_LIMIT: u32 = 10_000;
const FILES_PER_BATCH: usize = 32;
const MAX_INDEX_BYTES: usize = 64 << 20;
const PAGE_BYTES: usize = 512 << 10;
const MAX_PREVIEW: usize = 128 << 10;

#[derive(Default)]
struct Scan {
    endpoints: Vec<Endpoint>,
    paths: Vec<String>,
    modules: Vec<String>,
    files: usize,
    skipped: usize,
    limited: bool,
    done: bool,
    cancelled: bool,
    bytes: usize,
    generation: u64,
    owner: String,
}

#[derive(Default)]
pub struct RestfulTools {
    scans: BTreeMap<String, Scan>,
    generation: u64,
}

fn project_root(ctx: &Invocation) -> Result<String, Error> {
    ctx.project().ok_or_else(|| Error::Invalid("project".into()))?.root()
}

fn storage_key(root: &str) -> String {
    format!("environments:{root}")
}

fn module_for(root: &str, path: &str, modules: &[String]) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path).trim_start_matches('/');
    modules
        .iter()
        .filter(|module| {
            !module.is_empty() && relative.strip_prefix(module.as_str()).is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|module| module.len())
        .cloned()
        .or_else(|| relative.split_once("/src/").map(|(module, _)| module.to_owned()))
        .filter(|module| !module.is_empty())
        .unwrap_or_else(|| ".".into())
}

fn settings(root: &str) -> Result<Settings, Error> {
    let saved = host::storage::get(&storage_key(root))?;
    saved.map_or_else(
        || Ok(Settings::default()),
        |saved| serde_json::from_str(&saved).map_err(|e| Error::Invalid(e.to_string())),
    )
}

fn post(mut value: Value, client: &str) -> Result<(), Error> {
    value["client"] = json!(client);
    host::post_view_message(VIEW, &value.to_string())
}

impl RestfulTools {
    fn begin_scan(&mut self, ctx: &Invocation, client: &str) -> Result<(), Error> {
        let project = ctx.project().ok_or_else(|| Error::Invalid("project".into()))?;
        let root = project.root()?;
        let task = host::begin_task("RESTful Tools", true);
        let mut modules = BTreeSet::new();
        for glob in ["**/pom.xml", "**/build.gradle", "**/build.gradle.kts"] {
            for path in project.find_files(glob, FIND_LIMIT)? {
                let relative = path.strip_prefix(&root).unwrap_or(&path).trim_start_matches('/');
                modules.insert(relative.rsplit_once('/').map_or("", |(parent, _)| parent).to_owned());
            }
        }
        let mut scopes = modules.clone();
        scopes.insert(String::new());
        let mut files = BTreeSet::new();
        let mut limited = false;
        for scope in scopes {
            for extension in ["java", "kt", "kts"] {
                if task.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                let glob =
                    if scope.is_empty() { format!("**/*.{extension}") } else { format!("{scope}/**/*.{extension}") };
                let found = project.find_files(&glob, FIND_LIMIT)?;
                limited |= found.len() >= FIND_LIMIT as usize;
                files.extend(found);
            }
        }
        self.generation += 1;
        self.scans.insert(
            root,
            Scan {
                paths: files.into_iter().collect(),
                modules: modules.into_iter().collect(),
                limited,
                generation: self.generation,
                owner: client.into(),
                ..Scan::default()
            },
        );
        Ok(())
    }

    fn scan_batch(&mut self, ctx: &Invocation) -> Result<usize, Error> {
        let project = ctx.project().ok_or_else(|| Error::Invalid("project".into()))?;
        let root = project.root()?;
        let scan = self.scans.get_mut(&root).ok_or_else(|| Error::Invalid("scan_changed".into()))?;
        let from = scan.endpoints.len();
        if scan.done {
            return Ok(from);
        }
        let task = host::begin_task("RESTful Tools", true);
        let started = Instant::now();
        let mut source_bytes = 0;
        for _ in 0..FILES_PER_BATCH {
            if task.is_cancelled() {
                scan.cancelled = true;
                scan.done = true;
                return Err(Error::Cancelled);
            }
            let Some(path) = scan.paths.get(scan.files) else { break };
            task.report(scan.files as u32, Some(scan.paths.len() as u32), Some(path));
            scan.files += 1;
            match project.read_file(path) {
                Ok(source) if source.len() <= 1 << 20 => {
                    source_bytes += source.len();
                    for mut endpoint in scan::scan(path, &source) {
                        endpoint.module = module_for(&root, path, &scan.modules);
                        scan.bytes += serde_json::to_vec(&endpoint).unwrap().len();
                        if scan.bytes > MAX_INDEX_BYTES {
                            scan.limited = true;
                            scan.done = true;
                            break;
                        }
                        scan.endpoints.push(endpoint);
                    }
                }
                _ => scan.skipped += 1,
            }
            if scan.done || source_bytes >= 512 << 10 || started.elapsed().as_millis() >= 200 {
                break;
            }
        }
        scan.done |= scan.files == scan.paths.len();
        Ok(from)
    }

    fn scanned(&self, root: &str, client: &str, from: usize, reset: bool) -> Result<(), Error> {
        let scan = self.scans.get(root);
        let generation = scan.map_or(0, |s| s.generation);
        if reset {
            post(json!({"op": "scan-begin", "root": root, "generation": generation}), client)?;
        }
        let Some(scan) = scan else {
            return post(json!({"op": "scan", "root": root, "scan": null, "continue": false}), client);
        };
        let page = |start, end| {
            post(
                json!({"op": "scan-page", "root": root, "generation": generation,
            "offset": start, "endpoints": &scan.endpoints[start..end]}),
                client,
            )
        };
        let mut start = from;
        let mut bytes = 0;
        for (i, endpoint) in scan.endpoints.iter().enumerate().skip(from) {
            let size = serde_json::to_vec(endpoint).unwrap().len();
            if bytes + size > PAGE_BYTES && i > start {
                page(start, i)?;
                start = i;
                bytes = 0;
            }
            bytes += size;
        }
        if start < scan.endpoints.len() {
            page(start, scan.endpoints.len())?;
        }
        post(
            json!({"op": "scan", "root": root, "scan": {
            "generation": generation, "files": scan.files, "total_files": scan.paths.len(), "count": scan.endpoints.len(),
            "skipped": scan.skipped, "limited": scan.limited, "done": scan.done, "cancelled": scan.cancelled,
            "limit": if scan.bytes > MAX_INDEX_BYTES { "memory" } else if scan.limited { "files" } else { "" }
        }, "continue": !scan.done && (scan.owner.is_empty() || scan.owner == client)}),
            client,
        )
    }

    fn message(&mut self, value: Value, ctx: &Invocation) -> Result<(), Error> {
        let root = project_root(ctx)?;
        let client = value.get("client").and_then(Value::as_str).unwrap_or_default();
        if value.get("root").and_then(Value::as_str).is_some_and(|requested| !requested.is_empty() && requested != root)
        {
            return Err(Error::Invalid("project_changed".into()));
        }
        match value.get("op").and_then(Value::as_str).unwrap_or_default() {
            "ready" => {
                post(json!({"op": "ready", "root": root, "settings": settings(&root)?}), client)?;
                self.scanned(&root, client, 0, true)
            }
            "scan" => {
                self.begin_scan(ctx, client)?;
                self.scan_batch(ctx)?;
                self.scanned(&root, client, 0, true)
            }
            "scan-next" | "scan-cancel" => {
                let scan = self.scans.get_mut(&root).ok_or_else(|| Error::Invalid("scan_changed".into()))?;
                if value.get("generation").and_then(Value::as_u64) != Some(scan.generation)
                    || !scan.owner.is_empty() && scan.owner != client
                {
                    return Err(Error::Invalid("scan_changed".into()));
                }
                scan.owner = client.into();
                if value["op"] == "scan-cancel" {
                    scan.cancelled = true;
                    scan.done = true;
                    let from = scan.endpoints.len();
                    return self.scanned(&root, client, from, false);
                }
                let from = self.scan_batch(ctx)?;
                self.scanned(&root, client, from, false)
            }
            "navigate" => {
                if value
                    .get("generation")
                    .and_then(Value::as_u64)
                    .is_some_and(|generation| self.scans.get(&root).is_none_or(|scan| scan.generation != generation))
                {
                    return Err(Error::Invalid("scan_changed".into()));
                }
                let index =
                    value.get("index").and_then(Value::as_u64).ok_or_else(|| Error::Invalid("endpoint".into()))?;
                let endpoint = self
                    .scans
                    .get(&root)
                    .and_then(|scan| scan.endpoints.get(index as usize))
                    .ok_or_else(|| Error::NotFound("endpoint".into()))?;
                host::open_location(&endpoint.path, endpoint.line, endpoint.column)
            }
            "save" => {
                let settings: Settings =
                    serde_json::from_value(value["settings"].clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                settings.validate()?;
                host::storage::set(&storage_key(&root), &serde_json::to_string(&settings).unwrap())?;
                post(json!({"op": "saved"}), client)
            }
            "send" => {
                let settings: Settings =
                    serde_json::from_value(value["settings"].clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                let draft: Draft =
                    serde_json::from_value(value["draft"].clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                let request = request::prepare(&draft, &settings)?;
                let _task = host::begin_task("RESTful Tools", true);
                let start = Instant::now();
                let response = host::fetch(&request)?;
                let bytes = response.body.len();
                let body = String::from_utf8_lossy(&response.body[..bytes.min(MAX_PREVIEW)]);
                let mut header_bytes = 0;
                let headers: Vec<_> = response
                    .headers
                    .into_iter()
                    .take(100)
                    .take_while(|(name, value)| {
                        header_bytes += name.len() + value.len();
                        header_bytes <= 16 << 10
                    })
                    .collect();
                post(
                    json!({"op": "response", "status": response.status, "headers": headers,
                    "body": body, "bytes": bytes, "truncated": bytes > MAX_PREVIEW, "elapsed": start.elapsed().as_millis()}),
                    client,
                )
            }
            _ => Err(Error::Invalid("message".into())),
        }
    }
}

impl Plugin for RestfulTools {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "open" => host::reveal_view(VIEW)?,
            "scan" => {
                self.begin_scan(ctx, "")?;
                self.scan_batch(ctx)?;
                host::reveal_view(VIEW)?;
                self.scanned(&project_root(ctx)?, "", 0, true)?;
            }
            _ => return Err(Error::NotFound(id.into())),
        }
        Ok(None)
    }
}

impl Views for RestfulTools {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        if view != VIEW {
            return Err(Error::NotFound(view.into()));
        }
        Ok(View::message("RESTful Tools"))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if view != VIEW {
            return Err(Error::NotFound(view.into()));
        }
        let ViewEvent::Message(message) = event else { return Ok(()) };
        let value = serde_json::from_str(&message).map_err(|e| Error::Invalid(e.to_string()));
        let client = value
            .as_ref()
            .ok()
            .and_then(|v: &Value| v.get("client"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let op = value
            .as_ref()
            .ok()
            .and_then(|v: &Value| v.get("op"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let generation = value.as_ref().ok().and_then(|v| v.get("generation")).and_then(Value::as_u64);
        if let Err(error) = value.and_then(|value| self.message(value, ctx)) {
            let (code, detail) = match error {
                Error::Invalid(code) => (code, String::new()),
                Error::Denied(detail) => ("permission".into(), detail),
                Error::Cancelled => ("cancelled".into(), String::new()),
                other => ("failed".into(), other.to_string()),
            };
            post(
                json!({"op": "error", "request": op, "code": code, "detail": detail, "generation": generation}),
                &client,
            )?;
        }
        Ok(())
    }
}

register_plugin!(RestfulTools, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{HttpResponse, mock::MockHost};

    fn send(plugin: &mut RestfulTools, host: &MockHost, mut value: Value) -> Value {
        value["client"] = json!("test-page");
        plugin.handle(VIEW, ViewEvent::Message(value.to_string()), &host.invocation()).unwrap();
        let reply: Value = serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
        assert_eq!(reply["client"], "test-page");
        reply
    }

    #[test]
    fn scans_unsaved_sources_and_navigates_to_utf16_location() {
        let host = MockHost::install();
        host.add_file("src/Users.java", "class Users {}");
        host.open_editor(
            "/project/src/Users.java",
            "java",
            "class Users { @GetMapping(\"/users\") void list() {} }",
            &[],
        );
        let mut plugin = RestfulTools::default();
        let reply = send(&mut plugin, &host, json!({"op": "scan"}));
        assert_eq!(reply["scan"]["count"], 1);
        let page = host
            .view_messages()
            .into_iter()
            .map(|(_, text)| serde_json::from_str::<Value>(&text).unwrap())
            .find(|value| value["op"] == "scan-page")
            .unwrap();
        assert_eq!(page["endpoints"][0]["url"], "/users");
        send(&mut plugin, &host, json!({"op": "navigate", "index": 0}));
        let location = &host.opened_locations()[0];
        assert_eq!(location.0, "/project/src/Users.java");
        assert_eq!(location.1, 0);
        assert_eq!(location.2, "class Users { @GetMapping(\"/users\") void ".encode_utf16().count() as u32);
    }

    #[test]
    fn environments_survive_reactivation_and_are_isolated_by_project() {
        let host = MockHost::install();
        let custom = json!({"active": "dev", "environments": {"dev": {"baseUrl": "https://dev.example.com"}}});
        let mut plugin = RestfulTools::default();
        assert_eq!(send(&mut plugin, &host, json!({"op": "save", "settings": custom}))["op"], "saved");
        let mut reactivated = RestfulTools::default();
        let config = settings("/project").unwrap();
        assert_eq!(config.active, "dev");
        host.set_project_root(Some("/another"));
        send(&mut reactivated, &host, json!({"op": "ready"}));
        let messages = host.view_messages();
        let ready = messages
            .iter()
            .rev()
            .map(|(_, text)| serde_json::from_str::<Value>(text).unwrap())
            .find(|value| value["op"] == "ready")
            .unwrap();
        assert_eq!(ready["settings"]["active"], "local");
    }

    #[test]
    fn sends_through_host_and_returns_permission_errors_to_page() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "net:localhost"]);
        host.on_fetch(|request| {
            Ok(HttpResponse {
                status: 201,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: request.body.clone().unwrap(),
            })
        });
        let mut plugin = RestfulTools::default();
        let settings = Settings::default();
        let mut message = json!({"op": "send", "settings": settings, "draft": {
            "method": "POST", "url": "{{baseUrl}}/users", "headers": "Content-Type: application/json", "body": "{\"id\":1}"
        }});
        let reply = send(&mut plugin, &host, message.clone());
        assert_eq!(reply["status"], 201);
        assert_eq!(reply["body"], "{\"id\":1}");
        assert_eq!(host.requests()[0].url, "http://localhost:8080/users");
        message["draft"]["url"] = json!("https://ungranted.example.com/");
        let reply = send(&mut plugin, &host, message);
        assert_eq!(reply["op"], "error");
        assert_eq!(reply["request"], "send");
        assert_eq!(reply["code"], "permission");
    }

    #[test]
    fn web_assets_and_locale_keys_match() {
        let en: BTreeMap<String, String> = serde_json::from_str(include_str!("../web/locales/en.json")).unwrap();
        let zh: BTreeMap<String, String> = serde_json::from_str(include_str!("../web/locales/zh.json")).unwrap();
        assert_eq!(en.keys().collect::<Vec<_>>(), zh.keys().collect::<Vec<_>>());
        let page = include_str!("../web/index.html");
        assert!(page.contains(r#"href="./page.css""#));
        assert!(page.contains(r#"src="./page.js""#));
        assert!(!page.contains("<script>"), "page code is loaded from the plugin's own URL");
        assert!(include_str!("../plugin.toml").contains(r#"web = "web/index.html""#));
    }

    #[test]
    fn modules_use_the_nearest_build_file_or_source_root() {
        let modules = vec!["".into(), "services".into(), "services/api".into(), "arm-web".into()];
        assert_eq!(module_for("/project", "/project/services/api/src/main/java/C.java", &modules), "services/api");
        assert_eq!(module_for("/project", "/project/arm-web2/src/main/java/C.java", &modules), "arm-web2");
        assert_eq!(module_for("/project", "/project/src/main/java/C.java", &modules), ".");
    }

    #[test]
    fn scans_8896_endpoints_across_more_than_2000_files_and_pages_the_snapshot() {
        let host = MockHost::install();
        for module in ["arm-bi", "arm-web"] {
            host.add_file(&format!("{module}/pom.xml"), "<project/>");
        }
        for n in 0..2224 {
            let module = if n % 2 == 0 { "arm-bi" } else { "arm-web" };
            let source = format!(
                "class C{n} {{\n{}\n}}",
                (0..4).map(|m| format!("@GetMapping(\"/r/{n}/{m}\") void m{m}() {{}}\n")).collect::<String>()
            );
            host.add_file(&format!("{module}/src/main/java/C{n}.java"), &source);
        }
        let mut plugin = RestfulTools::default();
        let mut reply = send(&mut plugin, &host, json!({"op": "scan"}));
        assert_eq!(reply["scan"]["total_files"], 2224);
        let mut calls = 0;
        while reply["continue"] == true {
            let generation = reply["scan"]["generation"].clone();
            reply = send(&mut plugin, &host, json!({"op": "scan-next", "generation": generation}));
            calls += 1;
            assert!(calls < 2224, "the scan must finish");
        }
        assert_eq!(reply["scan"]["count"], 8896);
        assert_eq!(reply["scan"]["files"], 2224);
        assert_eq!(reply["scan"]["limited"], false);
        plugin.scanned("/project", "test-page", 0, true).unwrap();
        let pages: Vec<_> =
            host.view_messages().into_iter().filter(|(_, text)| text.contains("\"scan-page\"")).collect();
        assert!(pages.len() > 1);
        assert!(pages.iter().all(|(_, text)| text.len() < 1 << 20));
        assert!(plugin.scans["/project"].endpoints.iter().all(|e| ["arm-bi", "arm-web"].contains(&e.module.as_str())));
    }

    #[test]
    fn cancelled_scan_keeps_partial_results_and_rejects_an_old_generation() {
        let host = MockHost::install();
        for n in 0..80 {
            host.add_file(&format!("C{n}.java"), "class C { @GetMapping(\"/r\") void m() {} }");
        }
        let mut plugin = RestfulTools::default();
        let reply = send(&mut plugin, &host, json!({"op": "scan"}));
        let generation = reply["scan"]["generation"].clone();
        let cancelled = send(&mut plugin, &host, json!({"op": "scan-cancel", "generation": generation}));
        assert_eq!(cancelled["scan"]["cancelled"], true);
        assert_eq!(cancelled["continue"], false);
        assert_eq!(cancelled["scan"]["count"], reply["scan"]["count"]);
        send(&mut plugin, &host, json!({"op": "scan"}));
        let error = send(&mut plugin, &host, json!({"op": "scan-next", "generation": generation}));
        assert_eq!(error["code"], "scan_changed");
    }
}
