# RESTful Tools

独立的 OpenIDEA 插件：接口扫描、接口树、搜索、代码跳转、多环境和基础 HTTP 调试。
不修改 OpenIDEA 主程序，不依赖 IntelliJ 或 RestfulBox。

## 安装与使用

1. 在 OpenIDEA 的“设置 → 插件 → 从磁盘安装”中选择
   `dist/community.restful-tools-0.1.2.oiplugin`。已安装旧版的用户可直接安装此版本更新。
2. 授予读取项目和插件存储权限；需要本机请求时，勾选可选的
   `net:localhost` / `net:127.0.0.1` 权限。
3. 打开受信任项目，通过 Tools 菜单或 Find Action 中的
   `RESTful Tools: Open` 打开底部工具窗口。
4. 点击“扫描项目”，按“项目 → 模块 → 控制器 → 接口”浏览。模块通过 Maven/Gradle
   构建文件和 `src` 目录识别；模块和控制器默认折叠，展开时才创建接口节点。
   支持显示 `Tag` / `Api` 控制器名称、`Operation` / `ApiOperation` 接口名称。
   搜索框支持模块、URL、HTTP 方法、控制器、方法名、中文名称和文件路径，多个词必须全部匹配。
5. 单击接口将方法和 URL 填入请求编辑器；双击接口或点击“跳转代码”定位源码。
   `ANY` 表示未限制 HTTP 方法，调试时默认选 GET，可手动切换。
6. 在“编辑环境”中保存环境配置。选择环境后，URL、请求头值和请求体中的
   `{{name}}` 会使用当前环境替换。
7. 编辑 URL、请求头、请求体后点击“发送”。响应显示状态码、耗时、正文大小、
   响应头和正文；JSON 正文自动缩进。需要取消时使用 IDE 后台任务中的取消按钮。

扫描分批处理，面板持续显示已扫描/发现的文件总数，可用“取消扫描”保留部分结果。
源码修改后需重新扫描；扫描不是后台常驻索引。插件重载或空闲回收后也需重新扫描。
请求草稿和响应仅保留在当前面板中；环境配置按项目持久化，重启后仍可使用。

## API 1.2 兼容性

- `plugin.toml` 声明 `engines.openidea = "^1.2"`。
- 页面通过 `web/index.html` 加载独立的脚本和样式资源，避免发行版的安全策略阻止
  HTML 字符串视图中的内联脚本；不需要修改 IDE 的 CSP 或主程序。
- Rust SDK 固定到 OpenIDEA v0.1.38 的提交
  `9c5372c7ab44207f0d3ac3d01fcb62af224091b7`，其 WIT 包为
  `openidea:plugin@1.2.0`；不使用仓库当前的 1.3 SDK。
- 构建脚本检查生成组件中的 OpenIDEA 接口版本，发现非 1.2.0 接口就停止打包。
- `tests/component_api.rs` 可进一步解析实际组件的 import/export，检查二进制兼容性。

仅把清单从 `^1.3` 改成 `^1.2` 不足以解决兼容性，组件必须同时用 1.2 SDK 重编译。

## 支持的扫描范围

- Java、Kotlin（`.java` / `.kt` / `.kts`）。
- Spring MVC：`RequestMapping`、`GetMapping`、`PostMapping`、`PutMapping`、
  `DeleteMapping`、`PatchMapping`，类路径与方法路径组合。
- `path` / `value` 字面量、Java 数组、Kotlin 数组、`arrayOf(...)`、字面量字符串拼接。
- `RequestMapping` 中多个 `RequestMethod` 值。
- OpenFeign：接口方法的 Spring 路由注解和类的 `FeignClient(path = ...)`。
- JAX-RS：类/方法的 `Path` 和 `GET` / `POST` / `PUT` / `DELETE` / `PATCH` /
  `HEAD` / `OPTIONS` / `TRACE`，支持全限定注解名。
- 忽略注释和方法体中的伪注解，跳转位置使用 0-based 行号和 UTF-16 列。

这是源码声明扫描器，不是完整的 Java/Kotlin 类型分析器。暂不解析路径常量、
继承映射、组合注解、Spring 属性占位符、JAX-RS 子资源和 DTO 字段；无法求值的
路径表达式会跳过。未按导入解析注解类型，同名自定义注解可能被识别为框架注解。
Feign 的 `name` / `url` 不自动用作请求目标，请在环境中设置服务地址。
暂不支持 Actix-Web、SSE、并发请求或接入原生 Search Everywhere。

0.1.2 移除了旧版 2,000 文件、1,000 接口以及单文件 1,000 接口的数量限制。
每批最多处理 32 个文件，接口按不超过约 512 KiB 的消息传回，避免单次调用的时间/
CPU 预算和 1 MiB 消息大小限制。保存全部结果后再展开某个控制器，不会一次创建几千个接口按钮。

仍有明确的边界：超过 1 MiB 或无法读取的文件会计入跳过数量；索引达到 64 MiB 时
停止扫描并提示。IDE API 单次文件查询最多返回 10,000 项，插件会补充按已发现模块查询
并去重；如有查询达到宿主上限，会提示可能仍有文件遗漏。只扫描 IDE 返回的项目文件，
忽略/排除目录不会扫描。清除数量限制不等于完整复刻 IDEA 的语义扫描，路径常量、
组合注解和继承等尚未支持的情况仍可能导致数量不同。

## 环境与网络

环境示例（所有变量值都是字符串）：

```json
{
  "local": { "baseUrl": "http://localhost:8080", "userId": "1" },
  "dev": { "baseUrl": "https://dev.example.com", "userId": "2" }
}
```

请求示例：`{{baseUrl}}/users/{{userId}}`。扫描得到的 `{id}`、`${...}` 等路径模板
不会自动猜测值，请在发送前替换。变量可嵌套，最多 16 层；缺失变量或循环引用会报错。
环境配置以明文写入插件存储，每个项目最多 128 KiB，不是密码保险箱。

插件默认只声明本机网络权限。其他 HTTPS 主机须先编辑插件目录的 `plugin.toml`：

```toml
[capabilities]
required = ["project.read", "storage"]
optional = ["net:localhost", "net:127.0.0.1", "net:dev.example.com"]
```

重新打包、安装并授予对应权限；使用链接目录开发时修改清单后重新加载插件。
支持 `net:*.example.com` 子域模式，但不包含裸域名，也不支持 `net:*`。
修改清单无需修改或重新编译 OpenIDEA。

现有 API 的限制仍适用：

- 明文 HTTP 仅限 `localhost` / `127.0.0.1`，不支持内网 IP 或内网域名的 HTTP。
- HTTPS 请求沿用 IDE 的网络客户端和代理设置；不提供每请求证书或代理配置。
- 同时只发起一个请求，超时 30 秒；响应完整接收后展示。
- 本插件限制变量展开后的单项文本为 512 KiB、面板消息为 900 KiB。
- 宿主响应体上限 8 MiB，面板正文最多预览前 128 KiB；响应头最多预览 100 项、16 KiB。
- 二进制正文按 UTF-8 文本预览，不提供文件上传/下载。

## 构建与测试

Windows（Rust 工具链需安装 `wasm32-wasip2`）：

```powershell
rustup target add wasm32-wasip2
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

脚本生成 `dist/plugin.wasm` 和可从磁盘安装的 `.oiplugin` 包。插件独立构建，不需要
编译 OpenIDEA，也不会修改主程序或其他插件。首次构建需要从 Git 获取固定的 SDK。
如已设置 `CARGO_TARGET_DIR`，构建产物会从该目录复制。

在仓库内也可使用 Git Bash：

```sh
plugins/build.sh official/restful-tools
```

以下测试留给使用者执行（构建后再运行，兼容性测试会读取 `dist/plugin.wasm`）：

```powershell
cargo test --locked
```

建议手工检查：

1. API 1.2.0 的 IDE 安装成功，插件详情中不出现引擎版本不匹配。
2. 扫描含 Java/Kotlin Spring、Feign、JAX-RS 的项目，搜索、双击跳转的位置正确。
3. 添加两个环境，切换环境并重启 IDE，确认配置按项目保存。
4. 对本机测试服务发 GET/POST，检查 URL、请求头、请求体变量替换和响应展示。
5. 缺失变量、未授权主机、非本机 HTTP 和连接失败时，面板显示错误并恢复发送按钮。
6. 请求执行中，确认发送和扫描按钮禁用；通过 IDE 后台任务取消后可再次请求。
