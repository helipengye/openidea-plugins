# RESTful Tools

English | [简体中文](README.zh-CN.md)

A standalone [OpenIDEA](https://github.com/l0ng-ai/openidea) plugin for endpoint scanning, route browsing and search, source navigation, environments and basic HTTP debugging. It requires no IntelliJ or RestfulBox installation, or changes to the IDE itself.

## Compatibility and permissions

- Plugin ID: `helipengye.restful-tools`; version: `0.1.3`.
- Plugin API requirement: `engines.openidea = "^1.2"`.
- Required capabilities: `project.read` to read project files and `storage` to save environments and scan caches.
- Optional capabilities: `net:localhost` and `net:127.0.0.1` for local requests.
- The panel supports English and Chinese.

## Installation and usage

1. Build the plugin, then select `dist/helipengye.restful-tools-0.1.3.oiplugin` in **Settings → Plugins → Install Plugin from Package File**. Installations with a different plugin ID are treated as separate plugins.
2. Grant project read and plugin storage capabilities. Enable the optional `net:localhost` / `net:127.0.0.1` capabilities when sending requests to local services.
3. Open a trusted project. Use the Tools menu or **RESTful Tools: Open** in Find Action to open the bottom tool window.
4. Click **Scan project**, then browse **project → module → controller → endpoint**. Modules are identified from Maven/Gradle build files and `src` directories. Modules and controllers start collapsed; endpoint nodes are created on expansion. Controller labels support `Tag` / `Api`; endpoint labels support `Operation` / `ApiOperation`.
5. Search by module, URL, HTTP method, controller, method name, display label or file path. All search terms must match.
6. Click an endpoint to fill the request method and URL. Double-click it or use **Go to code** to navigate to the source. `ANY` means the mapping does not restrict the HTTP method; the editor defaults to GET and allows changing it.
7. Use **Edit environments** to save configurations. The selected environment replaces `{{name}}` in URLs, header values and request bodies.
8. Edit the request and click **Send**. The response displays status, duration, body size, headers and body, with JSON formatting. Cancel requests through the IDE's background task controls.

Scanning runs in batches with progress counts. **Cancel scan** keeps partial results in the current instance. Completed scans are compressed and saved per project. Opening the panel after an IDE restart or plugin reload automatically restores the last saved scan, including search and source navigation, with its cache time displayed.

Cached results are snapshots, including any unsaved editor content used by the scan. They are not automatically checked against source changes. Rescan after editing source files; cached navigation positions may have moved. Cancelled scans and scans reaching a file-query or memory limit do not replace the previous complete cache. A completed scan with no endpoints saves an empty result, replacing older results.

Each compressed, Base64-encoded project cache is limited to 768 KiB. OpenIDEA 0.1.38 also limits total plugin storage to 1 MiB, shared with environments and other project caches. Oversized caches or storage failures leave the current results usable and the previous cache intact; the panel reports that caching failed. Invalid or incompatible caches are ignored so a new scan can be started. Request drafts and responses live only in the current panel. Environments persist per project across restarts.

## API 1.2 compatibility

The manifest declares `engines.openidea = "^1.2"`. The Rust SDK is pinned to OpenIDEA v0.1.38 commit `9c5372c7ab44207f0d3ac3d01fcb62af224091b7`, whose WIT package is `openidea:plugin@1.2.0`.

The panel uses `web/index.html` with separate script and style assets, avoiding inline-script restrictions without IDE CSP changes. The build script checks the generated component's OpenIDEA interface version strings and stops packaging if any differs from `1.2.0`. Lowering the manifest engine requirement alone does not make a component built against a newer SDK compatible; rebuild it with the matching SDK.

## Supported scans

- Java and Kotlin files: `.java`, `.kt`, `.kts`.
- Spring MVC: `RequestMapping`, `GetMapping`, `PostMapping`, `PutMapping`, `DeleteMapping` and `PatchMapping`, combining class and method paths.
- Literal `path` / `value` strings, Java arrays, Kotlin arrays, `arrayOf(...)` and literal string concatenation.
- Multiple `RequestMethod` values in `RequestMapping`.
- OpenFeign: Spring route annotations on interface methods and class-level `FeignClient(path = ...)`.
- JAX-RS: class/method `Path` and `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`, `OPTIONS`, `TRACE`, including fully qualified annotation names.
- Comments and fake annotations inside method bodies are ignored. Navigation uses zero-based lines and UTF-16 columns.

This is a source declaration scanner. It does not evaluate path constants, inherited mappings, composed annotations, Spring property placeholders, JAX-RS subresources or DTO fields. Unevaluable paths are skipped. Annotation types are not resolved through imports, so custom annotations sharing framework annotation names may also match. Feign `name` / `url` values do not set the request target; configure the service address in an environment. Actix-Web, SSE, concurrent requests and native Search Everywhere integration are unsupported.

Version 0.1.2 removes the earlier limits of 2,000 files, 1,000 endpoints and 1,000 endpoints per file. Each batch processes up to 32 files, and endpoint messages are paged at approximately 512 KiB to stay within call budgets and the 1 MiB message limit. Expanding a controller creates only that controller's endpoint nodes.

Files over 1 MiB or unreadable files are counted as skipped. Scanning stops with a notice at the 64 MiB index guard. A single IDE file query returns at most 10,000 items; the plugin adds queries for discovered modules and deduplicates results, and warns when a query reaches the host limit. Only files returned by the IDE are scanned; ignored/excluded directories are omitted. Results may differ from a semantic scanner because constants, composed annotations and inheritance are unsupported.

## Environments and networking

Example environment configuration; every variable value must be a string:

```json
{
  "local": { "baseUrl": "http://localhost:8080", "userId": "1" },
  "dev": { "baseUrl": "https://dev.example.com", "userId": "2" }
}
```

Example request: `{{baseUrl}}/users/{{userId}}`. Replace route templates such as `{id}` or `${...}` manually before sending. Variables can nest up to 16 levels; missing variables and cycles cause errors. Environments are stored as plain text in plugin storage, up to 128 KiB per project, and are not a password vault.

Only local network capabilities are declared by default. To access another HTTPS host, edit the plugin's `plugin.toml`:

```toml
[capabilities]
required = ["project.read", "storage"]
optional = ["net:localhost", "net:127.0.0.1", "net:dev.example.com"]
```

Repackage, install and grant the new capability. For linked development, reload after changing the manifest. `net:*.example.com` permits subdomains, but not the bare domain; `net:*` is unsupported. No IDE changes or recompilation are required.

Current request limits:

- Plain HTTP is limited to `localhost` / `127.0.0.1`; LAN IP addresses and LAN hostnames require HTTPS.
- HTTPS uses the IDE's network client and proxy configuration, without per-request certificate or proxy settings.
- One request at a time, with a 30-second timeout; the response appears after it is fully received.
- Each expanded text value is limited to 512 KiB; panel messages are limited to 900 KiB.
- The host response body limit is 8 MiB. The panel previews the first 128 KiB of the body and at most 100 response headers / 16 KiB.
- Binary responses are previewed as UTF-8 text. File upload and download are unsupported.

## Build and test

Use a Rust toolchain supporting edition 2024. Run from this plugin directory on Windows:

```powershell
rustup target add wasm32-wasip2
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

The entry script calls this repository's shared `../scripts/build-plugin.ps1`, generating `dist/plugin.wasm` and the unsigned local package `dist/helipengye.restful-tools-0.1.3.oiplugin`. The package includes the manifest, component, both READMEs, `LICENSE` and all `web/` assets. It builds independently of the IDE and other plugins. The first build fetches the pinned SDK from Git. If `CARGO_TARGET_DIR` is set, the script copies the component from that directory.

Alternatively, build and link the directory with the OpenIDEA plugin CLI; see the [official plugin guide](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md):

```sh
openidea-plugin build
openidea-plugin link
```

The CLI generates and validates `dist/plugin.wasm`; use `build.ps1` to produce the `.oiplugin` package. After rebuilding a linked plugin, use **Settings → Plugins → Reload**.

Run source unit tests covering scanning, navigation, environment storage, request validation and web asset consistency:

```sh
cargo test --locked
```

Suggested manual checks:

1. Install in an API 1.2.0 IDE without an engine mismatch.
2. Scan Java/Kotlin Spring, Feign and JAX-RS projects; verify search and source navigation.
3. Save two environments, switch between them and restart the IDE to verify persistence per project.
4. Send GET/POST requests to a local service and verify variable substitution and response display.
5. Verify errors and restored Send controls for missing variables, denied hosts, nonlocal HTTP and connection failures.
6. Verify Send and Scan are disabled during a request, and cancellation allows another request.

## Source layout and license

- `plugin.toml`: metadata, API requirement, capabilities, view and commands.
- `src/lib.rs`: panel messaging, batched scanning, project environment storage and request execution.
- `src/cache.rs`: compressed scan snapshots, size limits and restoration.
- `src/scan.rs`: source route scanning for Spring, Feign and JAX-RS.
- `src/request.rs`: environment substitution and HTTP request validation.
- `web/`: panel HTML, CSS, JavaScript and English/Chinese locales.
- `build.ps1`: entry point for the shared Windows build, API version checks and packaging script.

Licensed under Apache-2.0; see [LICENSE](LICENSE). Original OpenIDEA contributor attributions are retained.
