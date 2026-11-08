# OpenIDEA Plugins

[English](README.md) | 简体中文

本仓库包含三个 [OpenIDEA](https://github.com/l0ng-ai/openidea) 插件，分别提供命名格式转换、MyBatis 导航补全，以及 REST 接口浏览与调试。每个目录都是独立的 Rust 插件项目，拥有自己的插件清单和依赖配置。

插件 ID 统一使用 `helipengye` 前缀：`helipengye.camel-case`、`helipengye.mybatis-tools` 和 `helipengye.restful-tools`。

## 插件介绍

| 插件 | 主要功能 | 版本 | 插件 API 要求 | 文档 |
| --- | --- | --- | --- | --- |
| CamelCase | 将选中文本或光标处单词循环转换为连字符、全大写下划线、大驼峰、小驼峰、下划线及空格分词格式；支持多光标和 `Alt+Shift+U` 快捷键。 | `0.2.0` | `^1.2` | [中文](camel-case/README.zh-CN.md) · [English](camel-case/README.md) |
| MyBatis Tools | 在 Java Mapper 方法与 XML 语句之间导航，跳转 XML 引用，补全语句 ID、参数及 Java 属性。 | `0.1.9` | `^1.2` | [中文](mybatis-tools/README.zh-CN.md) · [English](mybatis-tools/README.md) |
| RESTful Tools | 扫描 Java/Kotlin 中的 Spring MVC、OpenFeign 和 JAX-RS 接口，重启后恢复扫描缓存，浏览和搜索路由、跳转源码，并通过多环境配置调试 HTTP 请求。 | `0.1.5` | `^1.2` | [中文](restful-tools/README.zh-CN.md) · [English](restful-tools/README.md) |

以上版本和 API 要求取自各插件的 `plugin.toml`。插件 API 版本与 IDE 应用发布版本是不同概念。

## 安装

在 OpenIDEA 中打开 **设置 → 插件**：

- 插件目录包含 `plugin.toml` 和已构建的 `dist/plugin.wasm` 时，使用 **Install Plugin from Disk（从磁盘安装）**。
- 已打包的 `.oiplugin` 文件使用 **Install Plugin from Package File（从包文件安装）**。

安装后启用插件，并授予插件清单中声明的权限。具体权限和使用方法见各插件文档。每个插件均提供 Windows `build.ps1` 入口，通过[共用构建脚本](scripts/build-plugin.ps1)生成未签名的本地安装包。

旧构建若使用不同插件 ID，会被视为独立安装。自定义快捷键、RESTful 环境和扫描缓存归属于原插件 ID，不会自动继承到改名后的插件。

安装已构建的插件不需要 Rust 工具链，也不需要本地 SDK 源码。

## 源码构建与 SDK 依赖

从源码构建需要支持 Rust 2024 edition 的工具链和 `wasm32-wasip2` 目标。请进入对应插件目录执行构建与测试命令，仓库根目录不是 Cargo workspace。具体命令见各插件 README，插件 CLI 的安装与使用见[官方插件开发指南](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md)。

Rust 源码需要依赖 `openidea-plugin` SDK，但**不强制使用本地 SDK 路径**。也可以让 Cargo 从固定的兼容 Git 提交获取 SDK。

| 插件 | 当前 SDK 配置 | 是否需要本地 SDK 源码目录 |
| --- | --- | --- |
| CamelCase | Git 提交 `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | 不需要手动准备，Cargo 自动获取 SDK。 |
| MyBatis Tools | Git 提交 `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | 不需要手动准备，Cargo 自动获取 SDK。 |
| RESTful Tools | Git 提交 `9c5372c7ab44207f0d3ac3d01fcb62af224091b7` | 不需要手动准备，Cargo 自动获取 SDK。 |

三个插件的 SDK 依赖均固定到同一 Git 提交，以保持 API 1.2 兼容性。Cargo 自动获取 SDK，各插件的 `Cargo.lock` 记录解析后的依赖，无需配置本地 SDK 路径。同时开发 SDK 和插件时，也可以选择本地 `path` 依赖。

## 目录结构

```text
openidea-plugins/
├── camel-case/       # 命名格式转换
├── mybatis-tools/    # MyBatis 导航与补全
├── restful-tools/    # REST 接口扫描与 HTTP 调试
├── scripts/          # 共用本地构建及打包脚本
├── .gitignore
├── LICENSE
├── README.md
└── README.zh-CN.md
```

根目录 `.gitignore` 忽略生成的 `target/`、`dist/`、`.oiplugin` 安装包及本地编辑器/系统文件，保留 `Cargo.lock`。已经被 Git 跟踪的文件仍保持跟踪。

许可证：Apache-2.0，见 [LICENSE](LICENSE)。保留原有 OpenIDEA 贡献者署名，各插件目录均附带一份许可证。
