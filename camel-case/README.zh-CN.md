# CamelCase

[English](README.md) | 简体中文

[OpenIDEA](https://github.com/l0ng-ai/openidea) 命名格式转换插件，可将选中文本或各光标处的单词循环转换为常见命名格式。

## 兼容性与权限

- 插件 ID：`openidea.camel-case`
- 版本：`0.2.0`
- 插件 API 要求：`engines.openidea = "^1.2"`
- IDE 动作 ID：`plugin.openidea.camel-case.toggle`
- 必需权限：`editor.read` 和 `editor.write`，用于读取文本和应用编辑。

## 使用方法

1. 打开 **设置 → 插件**。已构建的插件目录使用 **Install Plugin from Disk（从磁盘安装）**；打包后的 `.oiplugin` 使用 **Install Plugin from Package File（从包文件安装）**。安装后启用插件。
2. 选中标识符，或将光标放在单词中。
3. 使用默认快捷键 **Alt+Shift+U**，或在 Find Action 中运行 **CamelCase: Toggle Case**。Windows、Linux 和 macOS 使用相同的默认快捷键。
4. 重复执行，按以下顺序循环转换：

```text
foo-bar-baz → FOO_BAR_BAZ → FooBarBaz → fooBarBaz
    ↑                                    ↓
foo bar baz ←────────────────────── foo_bar_baz
```

支持多选区和多光标，所有转换合并为一次可撤销编辑。能够识别缩写边界，例如 `HTTPServer` 转换为 `httpServer`。

没有选区时，单词范围只包含字母、数字和下划线。转换带连字符或空格的短语时，请完整选中文本。不识别的混合格式（如 `foo_BAR`）和不含字母的文本（如 `42`）保持不变。此操作只修改文本，不会重命名项目中其他位置的符号引用。

## 构建与测试

需要支持 Rust 2024 edition 的工具链、`wasm32-wasip2` 目标和 OpenIDEA 插件 CLI，安装方式见[官方插件开发指南](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md)。

SDK 固定到 OpenIDEA 提交 `9c5372c7ab44207f0d3ac3d01fcb62af224091b7`，与本仓库另外两个插件一致：

```toml
openidea-plugin = { git = "https://github.com/l0ng-ai/openidea.git", rev = "9c5372c7ab44207f0d3ac3d01fcb62af224091b7" }
```

Cargo 自动获取 SDK，不需要本地 OpenIDEA 源码或 SDK 路径。首次构建需要联网获取 SDK 和 Rust 依赖。生成的 WebAssembly 组件要求插件 API 1.2，与清单中的 `engines.openidea = "^1.2"` 一致。

在本插件目录执行：

```sh
rustup target add wasm32-wasip2
cargo test --locked
openidea-plugin build
openidea-plugin link
```

`build` 生成 `dist/plugin.wasm` 并校验插件，`link` 将当前目录链接到本机 IDE。链接开发时，每次重新构建后在 **设置 → 插件 → Reload** 中重新加载。

## 源码结构

- `plugin.toml`：命令、快捷键、权限及 API 要求。
- `src/lib.rs`：格式识别、单词拆分、编辑器集成及单元测试。
- `Cargo.toml` / `Cargo.lock`：Rust 依赖配置和锁文件。

插件清单声明许可证为 `Apache-2.0`。
