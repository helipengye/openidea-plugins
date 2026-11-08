# OpenIDEA Plugins

English | [简体中文](README.zh-CN.md)

Three plugins for [OpenIDEA](https://github.com/l0ng-ai/openidea), covering identifier case conversion, MyBatis navigation and completion, and REST endpoint browsing and debugging. Each directory is an independent Rust plugin project with its own manifest and dependencies.

Plugin IDs use the `helipengye` prefix: `helipengye.camel-case`, `helipengye.mybatis-tools` and `helipengye.restful-tools`.

## Plugins

| Plugin | Features | Version | Plugin API requirement | Documentation |
| --- | --- | --- | --- | --- |
| CamelCase | Cycle selected text or caret words through kebab, upper snake, Pascal, camel, snake and space-separated styles; supports multiple carets and `Alt+Shift+U`. | `0.2.0` | `^1.2` | [English](camel-case/README.md) · [简体中文](camel-case/README.zh-CN.md) |
| MyBatis Tools | Navigate between Java mapper methods and XML statements; resolve XML references and complete statement IDs, parameters and Java properties. | `0.1.9` | `^1.2` | [English](mybatis-tools/README.md) · [简体中文](mybatis-tools/README.zh-CN.md) |
| RESTful Tools | Scan Spring MVC, OpenFeign and JAX-RS endpoints in Java/Kotlin; restore cached scans after restart, browse and search routes, navigate to source, and debug HTTP requests with environments. | `0.1.3` | `^1.2` | [English](restful-tools/README.md) · [简体中文](restful-tools/README.zh-CN.md) |

Versions and API requirements above come from each plugin's `plugin.toml`; plugin API versions are distinct from IDE application release versions.

## Installation

In OpenIDEA, open **Settings → Plugins**:

- Use **Install Plugin from Disk** for a plugin directory containing `plugin.toml` and its built `dist/plugin.wasm`.
- Use **Install Plugin from Package File** for a packaged `.oiplugin`.

Enable the plugin and grant the capabilities declared in its manifest. See each plugin's documentation for permissions and usage. Each plugin includes a Windows `build.ps1` entry point that produces an unsigned local installation package using the shared [build script](scripts/build-plugin.ps1).

Earlier builds with different plugin IDs are separate installations. Custom shortcut bindings and RESTful environments/scan caches belong to their original plugin IDs and are not automatically inherited by the renamed plugins.

Installing a prebuilt plugin requires neither Rust nor a local SDK checkout.

## Building from source and SDK dependencies

Source builds require a Rust toolchain supporting edition 2024 and the `wasm32-wasip2` target. Run build and test commands inside the relevant plugin directory; the repository root is not a Cargo workspace. See the individual READMEs for commands and the [official plugin guide](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md) for the plugin CLI.

The Rust source needs the `openidea-plugin` SDK dependency, but **a local SDK path is optional**. Cargo can obtain the SDK from a compatible pinned Git revision instead.

| Plugin | Current SDK configuration | Local SDK checkout needed? |
| --- | --- | --- |
| CamelCase | Git revision `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | No manual checkout; Cargo fetches the SDK. |
| MyBatis Tools | Git revision `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | No manual checkout; Cargo fetches the SDK. |
| RESTful Tools | Git revision `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | No manual checkout; Cargo fetches the SDK. |

All three SDK dependencies are pinned to the same Git commit for API 1.2 compatibility. Cargo fetches the SDK automatically, and each plugin's `Cargo.lock` records the resolved dependencies. No local SDK path is required. A local `path` dependency is an alternative when developing the SDK and plugin together.

## Repository layout

```text
openidea-plugins/
├── camel-case/       # Identifier case conversion
├── mybatis-tools/    # MyBatis navigation and completion
├── restful-tools/    # REST endpoint scanning and HTTP debugging
├── scripts/          # Shared local build and packaging script
├── .gitignore
├── LICENSE
├── README.md
└── README.zh-CN.md
```

The root `.gitignore` excludes generated `target/`, `dist/`, `.oiplugin` packages and local editor/OS files, while keeping `Cargo.lock` files. Files already tracked by Git remain tracked.

Licensed under Apache-2.0; see [LICENSE](LICENSE). Original OpenIDEA contributor attributions are retained, and each plugin directory includes its own copy of the license.
