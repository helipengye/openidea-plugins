# MyBatis Tools

English | [简体中文](README.zh-CN.md)

A plugin for [OpenIDEA](https://github.com/l0ng-ai/openidea) providing navigation and completion between Java MyBatis mapper interfaces and XML mapping files.

## Compatibility and permissions

- Plugin ID: `helipengye.mybatis-tools`
- Plugin version: `0.1.9` in `plugin.toml`; the Rust crate version is currently `0.1.0`.
- Plugin API requirement: `engines.openidea = "^1.2"`.
- Required capability: `project.read`, for finding and reading mapper XML and Java sources. The plugin does not execute SQL or connect to a database.
- The SDK is pinned to commit `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` of OpenIDEA for API 1.2 compatibility.

## Features

- Gutter navigation in both directions between Java mapper methods and XML `select`, `insert`, `update` and `delete` statements.
- Go to Definition from XML statement IDs to Java methods, and from mapper namespaces to Java declarations.
- Navigation for local and qualified `resultMap` references, `<include refid="…">` and Java type attributes such as `resultType` and `parameterType`.
- Navigation from result mapping properties and SQL expression properties to Java source where resolvable.
- Completion of unmapped Java method names in statement `id="…"` attributes.
- Parameter and Java property completion in `#{…}`, `${…}` and dynamic SQL test expressions, including `@Param` names, nested properties and `foreach` variables where resolvable.

Java Ctrl-click / Go to Definition remains handled by the IDE. Use gutter icons to navigate from a Java declaration to its XML statement.

Version 0.1.9 reduces latency when navigating from XML `test` expressions: Java declaration positions reuse a line index, and expression navigation reads XML tags as needed without first collecting all mappings. Reference navigation resolves only the value at the caret. Shared SQL fragments, inherited parameters, nested properties and `foreach` scopes remain supported; the IDE's 500ms timeout is unchanged.

## Usage

1. Open **Settings → Plugins**. Use **Install Plugin from Disk** for the built plugin directory, or **Install Plugin from Package File** for a packaged `.oiplugin`. Enable it and grant `project.read`.
2. Open a project containing Java mapper sources and XML mapping files.
3. Set the XML `namespace` to the fully qualified Java interface name, and match statement IDs to method names.
4. Click gutter icons to switch between the mapper method and its statement. Use Go to Definition on XML references and the IDE's completion action while editing supported values.

For example, `com/acme/UserMapper.java`:

```java
package com.acme;

import org.apache.ibatis.annotations.Param;

public interface UserMapper {
    String findName(@Param("id") long id);
}
```

Corresponding `UserMapper.xml`:

```xml
<mapper namespace="com.acme.UserMapper">
  <select id="findName" resultType="java.lang.String">
    SELECT name FROM users WHERE id = #{id}
  </select>
</mapper>
```

The gutter links `findName` in both files. The XML `findName` value navigates to the Java method; `id` can be completed inside the placeholder.

## Boundaries

This plugin analyzes project source text, rather than a complete Java compiler model or MyBatis runtime configuration. Java source must be available for property resolution; Kotlin mappers, classes available only in dependency JARs, custom type aliases and complex expressions are not guaranteed to resolve.

Same-named XML files are tried first, up to 32 candidates. Other filenames are discovered through a sliced scan of up to 10,000 XML files, advanced by provider calls, so initial navigation may need another attempt. Lookup results are cached for up to 10 minutes; reload the plugin if file moves or newly added mappings are not reflected yet. Ignored or excluded files depend on what the IDE returns.

## Build and test

Requirements: a Rust toolchain supporting edition 2024 and `wasm32-wasip2`. The Windows build script uses the shared `../scripts/build-plugin.ps1` in this repository. The first build needs network access to fetch the pinned SDK and Rust dependencies.

Run from this plugin directory:

```powershell
rustup target add wasm32-wasip2
cargo test --locked
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

The script generates `dist/plugin.wasm` and `dist/helipengye.mybatis-tools-0.1.9.oiplugin`. The package version comes from `plugin.toml`. The unsigned local package includes the manifest, component, both READMEs and `LICENSE`.

Alternatively, use the OpenIDEA plugin CLI; see the [official plugin guide](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md):

```sh
openidea-plugin build
openidea-plugin link
```

`build` generates and validates `dist/plugin.wasm`; `link` loads this folder in the local IDE. After rebuilding, use **Settings → Plugins → Reload**. Unit tests in `src/lib.rs` cover parsing, navigation, sliced indexing, statement/parameter completion, UTF-16 positions, shared SQL/foreach scopes and a 6000-property navigation regression within 500ms in the native mock host. Native test timings do not measure IDE/WASM performance.

## Source layout

- `plugin.toml`: API requirement, capabilities and provider registrations.
- `src/lib.rs`: mapper indexing, providers and unit tests.
- `src/java.rs`: Java source and type parsing.
- `src/xml.rs`: XML tags, mappings and expression contexts.
- `src/navigation.rs`: Java type/property resolution and expression completion.
- `build.ps1`: entry point for the shared Windows build and packaging script.

Licensed under Apache-2.0; see [LICENSE](LICENSE). Original OpenIDEA contributor attributions are retained.
