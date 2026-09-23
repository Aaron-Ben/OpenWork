# 注释规范

## 核心原则

- 代码说明"是什么"和"怎么做"，注释只说明"为什么"。
- 设计文档说明系统该怎样，注释说明这段代码为什么这样实现；两者不重复，注释引用文档章节即可。

## 1. 语言

| 内容 | 语言 |
|---|---|
| 代码注释、文档注释 | 中文 |
| 标识符 | 英文 |
| 模型可见文本（系统提示词、工具描述、工具结果、给模型的错误） | 英文，见 [agent-context.md](agent-context.md) |
| 日志、`CommandError.message` | 英文 |
| 界面文案 | 走 i18n，见 [frontend.md](frontend.md) |

专有名词保留英文原文：Session、Turn、Tool Call、Seatbelt、spill。

## 2. 文档注释（`///`、`//!`）

- `pub` 和 `pub(crate)` 的类型、trait、跨模块调用的函数必须有文档注释。
- 写**契约**：输入的前提、返回值的含义、什么情况下返回错误、有没有副作用。不要把函数名翻译一遍。
- 模块头 `//!` 写这个模块负责什么、不负责什么，以及对应的设计文档章节。

```rust
// ✅ 正确：说明返回值的含义与边界
/// `path` 在 `root` 之下时返回相对路径的各段；不在其下时返回 `None`。
fn components_below<'a>(root: &Path, path: &'a Path) -> Option<Vec<&'a str>>

// ❌ 错误：翻译函数名
/// 获取下面的组件。
fn components_below<'a>(root: &Path, path: &'a Path) -> Option<Vec<&'a str>>
```

## 3. 行内注释

只写代码本身表达不了的东西：

- 反直觉的实现，以及不这样写会出什么问题；
- 外部系统的限制（macOS、Seatbelt、PostgreSQL、模型厂商的行为）；
- 对设计文档条目的引用。

```rust
// ✅ 正确：解释为什么
// macOS 卷默认大小写不敏感：新建的 `.ENV` 保留调用方的拼写，但 dotenv 仍会把它当 `.env` 读。
pub(crate) fn is_workspace_sensitive(workspace: &Path, path: &Path) -> bool

// ❌ 错误：复述代码
// 遍历所有组件
for component in components { ... }
```

## 4. 常量

上限、阈值、超时的文档注释写出设计文档出处：`/// Paths returned (tools.md §9 glob).`

## 5. 生命周期

- 改代码时同步改相关注释；过时的注释比没有注释更糟。
- 不留注释掉的代码。
- 不写没有归属的 `TODO`。确实要留，写成 `// TODO(WP4): ...`，并且该事项已经记在计划或进度文件里。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 注释复述代码，或文档注释只是翻译函数名
- `pub` / `pub(crate)` 的类型或跨模块函数缺少文档注释
- 逻辑改了而注释没改
- 上限常量没有注明设计文档出处
- 注释掉的代码；没有归属的 `TODO` / `FIXME`
- 注释语言与本规范不一致
