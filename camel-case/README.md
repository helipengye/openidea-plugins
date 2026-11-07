# CamelCase

English | [简体中文](README.zh-CN.md)

A plugin for [OpenIDEA](https://github.com/l0ng-ai/openidea) that cycles selected text or the word at each caret through common identifier styles.

## Compatibility and permissions

- Plugin ID: `helipengye.camel-case`
- Version: `0.2.0`
- Plugin API requirement: `engines.openidea = "^1.2"`
- IDE action ID: `plugin.helipengye.camel-case.toggle`
- Required capabilities: `editor.read` and `editor.write`, for reading text and applying edits.

## Usage

1. Open **Settings → Plugins**. Use **Install Plugin from Disk** for the built plugin folder, or **Install Plugin from Package File** for a packaged `.oiplugin`, then enable it.
2. Select an identifier, or place the caret inside a word.
3. Use the default shortcut **Alt+Shift+U**, or run **CamelCase: Toggle Case** from Find Action. The default shortcut is the same on Windows, Linux and macOS.
4. Repeat the action to cycle through these styles:

```text
foo-bar-baz → FOO_BAR_BAZ → FooBarBaz → fooBarBaz
    ↑                                    ↓
foo bar baz ←────────────────────── foo_bar_baz
```

Multiple selections and carets are processed together as one undoable edit. Acronym boundaries are recognized: `HTTPServer` becomes `httpServer`.

Without a selection, a word consists of letters, digits and underscores. Select the whole phrase to convert text containing hyphens or spaces. Unrecognized mixed styles such as `foo_BAR`, or text without letters such as `42`, are left unchanged. This action changes text only; it does not rename symbol references across the project.

## Build and test

Requirements: a Rust toolchain supporting edition 2024 and the `wasm32-wasip2` target. The Windows build script uses the shared `../scripts/build-plugin.ps1` in this repository.

The SDK is pinned to OpenIDEA commit `9c5372c7ab44207f0d3ac3d01fcb62af224091b7`, matching the other plugins in this repository:

```toml
openidea-plugin = { git = "https://github.com/l0ng-ai/openidea.git", rev = "9c5372c7ab44207f0d3ac3d01fcb62af224091b7" }
```

Cargo fetches the SDK automatically; no local OpenIDEA checkout or SDK path is required. The first build needs network access to fetch the SDK and Rust dependencies. The generated WebAssembly component requires plugin API 1.2, matching `engines.openidea = "^1.2"` in the manifest.

Run from this plugin directory:

```powershell
rustup target add wasm32-wasip2
cargo test --locked
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

The script generates `dist/plugin.wasm` and `dist/helipengye.camel-case-0.2.0.oiplugin`. The unsigned local package includes the manifest, component, both READMEs and `LICENSE`.

Alternatively, use the OpenIDEA plugin CLI; see the [official plugin guide](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md):

```sh
openidea-plugin build
openidea-plugin link
```

`build` generates `dist/plugin.wasm` and validates the plugin. `link` loads the directory in your local IDE. After rebuilding a linked plugin, use **Settings → Plugins → Reload**.

## Source layout

- `plugin.toml`: command, shortcut, capabilities and API requirement.
- `src/lib.rs`: case detection, word splitting, editor integration and unit tests.
- `Cargo.toml` / `Cargo.lock`: Rust dependency configuration and lockfile.
- `build.ps1`: entry point for the shared Windows build and packaging script.

Licensed under Apache-2.0; see [LICENSE](LICENSE). Original OpenIDEA contributor attributions are retained.
