# 跨层契约规范

大多数 bug 出在层与层的边界上：Rust ↔ Desktop、Core ↔ 数据库、Core ↔ 模型、工具 ↔ 模型。本规范约束这些边界上的数据形状。数据库字段与时间见 [database.md](database.md)。

## 1. 每份契约只有一个所有者

同一份数据形状只在一个地方定义与解码，其他地方引用它：

| 契约 | 所有者 | 需要同步的地方 |
|---|---|---|
| Session Update / Snapshot | `openwork-core` 的类型 | `desktop/src/bridge/compat.ts`，形状变化时提升 `RUNTIME_SESSION_UPDATE_VERSION` |
| Tauri Command 错误 | `desktop/src-tauri/src/error.rs` | `desktop/src/lib/commandError.ts`、`tests/command_error_contract.rs` |
| 工具输入 schema | 工具的 `Input` 结构体（`schemars` 生成） | 工具描述文本里提到的参数名 |
| 数据库行 | SQLx 迁移 | `storage/postgres/*.rs` 的读写 |

- Rust 发来的数据形状在前端只由 `desktop/src/bridge/compat.ts` 定义类型，版本检查也只在 bridge 层做（`supportsRuntimeSessionUpdateVersion`）；组件和 store 从 `@/bridge/compat` 导入类型。
- 禁止在使用处用 `as` 自造同一份负载的局部类型：

```ts
// ❌ 错误：每个使用方各有一份私有的契约，Rust 改字段时不会有任何报错
const turnId = (update as { turnId?: string }).turnId

// ✅ 正确：类型来自唯一的所有者
import type { RuntimeSessionUpdateEnvelope } from '@/bridge/compat'
```

## 2. 序列化命名

| 位置 | 字段 | 枚举值 |
|---|---|---|
| 发给 Desktop 的 DTO、Session Update | `camelCase` | `snake_case` |
| 工具输入参数 | `camelCase` | `snake_case` |
| Trace 属性 | `camelCase` | `snake_case` |
| 数据库列 | `snake_case` | `snake_case` |

```rust
// ✅ 正确
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError { pub code: CommandErrorCode, pub message: String }

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandErrorCode { SessionNotFound, ... }
```

- ID 新类型用 `#[serde(transparent)]`，序列化结果就是字符串。
- 时间出库带 `+08:00`，见 [database.md](database.md) §1.5。

## 3. 修改契约时

一次改动里同时完成：

1. 所有者的类型；
2. 所有同步点（上表右列）；
3. 两侧的契约测试：Rust 侧断言序列化后的 JSON 形状，前端用同样形状的样本测试使用方；
4. 设计文档里对这份数据的描述。

不保留旧字段、不为旧形状写兼容解码（AGENTS.md）；Session Update 的形状变了就提升版本号。

## 4. 进入模型的数据

- 只把模型需要的内容放进请求。附件、artifact、UI 专用字段不发给模型，也不计入 token 估算（`model_visible_bytes`）。
- 发给模型的内容有上限，见 [agent-context.md](agent-context.md)。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 同一份负载在多处解码，或前端组件直接读 Rust 原始字段
- 改了 Rust 类型而 `compat.ts` 没同步，或 Session Update 形状变了没提升版本号
- 改了 `CommandErrorCode` 而 TypeScript 侧与契约测试没同步
- DTO 字段不是 `camelCase`，或枚举值不是 `snake_case`
- 为旧字段保留兼容解码
- UI 专用数据进入模型请求或 token 估算
