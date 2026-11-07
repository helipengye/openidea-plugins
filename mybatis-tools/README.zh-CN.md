# MyBatis Tools

[English](README.md) | 简体中文

[OpenIDEA](https://github.com/l0ng-ai/openidea) 的 MyBatis 辅助插件，为 Java Mapper 接口与 XML 映射文件提供导航和补全。

## 兼容性与权限

- 插件 ID：`community.mybatis-tools`。
- 插件版本：`plugin.toml` 中为 `0.1.8`；Rust crate 版本目前为 `0.1.0`。
- 插件 API 要求：`engines.openidea = "^1.2"`。
- 必需权限：`project.read`，用于查找和读取 Mapper XML 与 Java 源码。插件不执行 SQL，也不连接数据库。
- SDK 固定到 OpenIDEA 提交 `9c5372c7ab44207f0d3ac3d01fcb62af224091b7`，保持 API 1.2 兼容性。

## 功能

- 通过行号旁的导航图标，在 Java Mapper 方法与 XML 的 `select`、`insert`、`update`、`delete` 语句之间双向跳转。
- 从 XML 语句 ID 跳转到 Java 方法，从 Mapper 的 `namespace` 跳转到 Java 声明。
- 跳转本地及带命名空间的 `resultMap` 引用、`<include refid="…">`，以及 `resultType`、`parameterType` 等 Java 类型属性。
- 在能够解析的情况下，从结果映射属性、SQL 表达式属性跳转到 Java 源码。
- 在语句的 `id="…"` 中补全尚未映射的 Java 方法名。
- 在 `#{…}`、`${…}` 和动态 SQL 的 test 表达式中补全参数及 Java 属性；在能够解析的情况下支持 `@Param` 名称、嵌套属性及 `foreach` 变量。

Java 中的 Ctrl-click / Go to Definition 继续交给 IDE 原生处理。从 Java 声明跳转到 XML 语句时，请使用行号旁的导航图标。

## 使用方法

1. 打开 **设置 → 插件**。已构建的插件目录使用 **Install Plugin from Disk（从磁盘安装）**；打包后的 `.oiplugin` 使用 **Install Plugin from Package File（从包文件安装）**。启用插件并授予 `project.read` 权限。
2. 打开包含 Java Mapper 源码与 XML 映射文件的项目。
3. 将 XML 的 `namespace` 设置为 Java 接口全限定名，将语句 ID 与方法名对应。
4. 点击行号旁的图标在 Mapper 方法与语句之间切换；在 XML 引用上使用 Go to Definition，在支持的位置调用 IDE 的代码补全。

例如，`com/acme/UserMapper.java`：

```java
package com.acme;

import org.apache.ibatis.annotations.Param;

public interface UserMapper {
    String findName(@Param("id") long id);
}
```

对应的 `UserMapper.xml`：

```xml
<mapper namespace="com.acme.UserMapper">
  <select id="findName" resultType="java.lang.String">
    SELECT name FROM users WHERE id = #{id}
  </select>
</mapper>
```

两个文件的 `findName` 通过行号旁的图标互相跳转。XML 的 `findName` 属性值可跳转到 Java 方法，占位符中可补全 `id`。

## 使用边界

插件基于项目源码文本分析，不包含完整的 Java 编译器模型或 MyBatis 运行时配置。属性解析需要对应的 Java 源码；不保证解析 Kotlin Mapper、仅存在于依赖 JAR 中的类型、自定义类型别名及复杂表达式。

插件先尝试同名 XML，最多读取 32 个候选文件；其他文件名通过分片扫描最多 10,000 个 XML 文件发现。扫描由编辑器 provider 调用逐步推进，首次导航可能需要再次尝试。查询结果最多缓存 10 分钟；移动文件或新增映射尚未生效时，可重新加载插件。忽略或排除文件能否被发现取决于 IDE 返回的文件范围。

## 构建与测试

需要支持 Rust 2024 edition 的工具链、`wasm32-wasip2` 目标和 OpenIDEA 插件 CLI，安装方式见[官方插件开发指南](https://github.com/l0ng-ai/openidea/blob/main/plugins/README.md)。首次构建需要联网获取固定版本的 SDK 和 Rust 依赖。

在本插件目录执行：

```sh
rustup target add wasm32-wasip2
cargo test --locked
openidea-plugin build
openidea-plugin link
```

`build` 生成并校验 `dist/plugin.wasm`；`link` 将当前目录链接到本机 IDE。重新构建后，在 **设置 → 插件 → Reload** 中重新加载。`src/lib.rs` 中的单元测试覆盖解析、导航、分片索引及语句/参数补全。

## 源码结构

- `plugin.toml`：API 要求、权限及 provider 注册。
- `src/lib.rs`：Mapper 索引、provider 及单元测试。
- `src/java.rs`：Java 源码及类型解析。
- `src/xml.rs`：XML 标签、映射和表达式上下文。
- `src/navigation.rs`：Java 类型、属性解析及表达式补全。

插件清单声明许可证为 `Apache-2.0`。
