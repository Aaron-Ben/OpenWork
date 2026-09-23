# 代码风格规范

适用于 `crates/` 与 `desktop/src-tauri/` 的 Rust 代码。前端见 [frontend.md](frontend.md)。

## 1. 规模

| 对象 | 上限 | 超过时 |
|---|---|---|
| Rust 源文件（`src/`） | **800 行**，超过 500 行就要审视有没有可拆出的独立职责 | 按职责拆成子模块，不按行数机械切分 |
| 集成测试文件（`tests/`） | 1500 行 | 按被测行为拆文件 |
| 函数 | **60 行**（不含空行与注释） | 拆出有名字的步骤函数 |
| 函数参数 | **5 个** | 合成一个有语义的结构体，例如 `EditTarget { path, display, artifact_path, change_id, observations }` |
| 嵌套 | 4 层 | 提前返回、`let ... else`、拆函数 |

**存量例外**：`core.rs`（2388 行）、`session/trace.rs`、`session/run_loop.rs`、`session/actor.rs`、`file_change.rs` 等超限文件，新增代码不得继续让它们变大；改到它们时，把本次涉及的职责拆出去。

## 2. 命名与类型

- **标识符用领域词**，与设计文档一致：`Turn`、`Session`、`ToolCall`、`Spill`、`Grant`，不要自造同义词。
- **ID 一律用新类型**，不在层间传裸 `String`：

```rust
// ✅ 正确：session/ids.rs 的 string_id! 或 ToolCallId 这类新类型
fn cancel(&self, turn_id: &TurnId) -> Result<(), SessionError>

// ❌ 错误：两个 String 参数可以传反，编译器不会报错
fn cancel(&self, session_id: &str, turn_id: &str) -> Result<(), SessionError>
```

- **状态用枚举，不用 bool 或字符串**：两个以上互斥状态、或者会出现在序列化结果里的值，必须是枚举。

```rust
// ✅ 正确
pub enum SandboxMode { AcceptEdits, Auto }

// ❌ 错误：再加一个模式时所有调用点都会静默出错
pub struct Policy { pub bash_can_write_workspace: bool }
```

- **上限与阈值是具名常量**，文档注释写明出处；数字本身以设计文档为准，改动要走计划里的决策流程：

```rust
// ✅ 正确
/// Paths returned (tools.md §9 glob).
const MAX_RESULTS: usize = 100;

// ❌ 错误：魔术数字，且无法追溯来源
if matches.len() > 100 { ... }
```

## 3. 可见性与模块

- 默认私有；crate 内共享用 `pub(crate)`；只有跨 crate 的 API 才用 `pub`，并在 `lib.rs` 统一 `pub use`。
- 依赖方向见 AGENTS.md，禁止反向依赖或循环依赖；新增跨 crate 依赖先改 `docs/architecture.md`。
- 一个模块一个职责。不要建 `utils.rs`、`helpers.rs`、`common.rs` 这类按"类型"归堆的模块，按领域命名（`spill.rs`、`observation.rs`、`scan.rs`）。

## 4. 导入

三组，组间空一行：标准库 → 外部 crate → 本 crate（`crate::` / `super::`）。由 `cargo fmt` 排序，不手动调整组内顺序。

```rust
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::scan::{ScanBudget, compile_glob};
use crate::spill::SpillWriter;
```

禁止 `use xxx::*`，测试模块里的 `use super::*;` 除外。

## 5. 值与所有权

- **优先返回新值**，不就地修改入参。配置类对象用消费 `self` 的 `with_*` 方法：

```rust
// ✅ 正确：ToolSessionContext 的写法
let session = ToolSessionContext::local(root, profile)
    .with_spill_directory(spill)
    .with_file_observations(observations);

// ❌ 错误：构造后再改字段，调用方看不出哪些字段是必需的
let mut session = ToolSessionContext::local(root, profile);
session.spill = Some(spill);
```

- 共享只读数据用 `Arc<T>`；需要共享的可变状态见 [async-concurrency.md](async-concurrency.md)。
- 不要为了绕过借用检查而 `clone()` 大对象；先调整计算顺序（例如先算出结果再移动所有权）。

## 6. lint 抑制

- `cargo clippy --workspace --all-targets -- -D warnings` 必须通过。
- 必须抑制时用 `#[expect(lint, reason = "...")]`，不用 `#[allow]`。`expect` 在 lint 不再触发时会报错，避免抑制残留。
- 例外：lint 只在部分平台、feature 或测试构建中触发时，`expect` 会在不触发的构建里报错。这时用 `#[cfg_attr(条件, expect(lint, reason = "..."))]`，或带 `reason` 的 `#[allow]`。只给测试用的代码直接加 `#[cfg(test)]`，不要抑制 `dead_code`。
- 抑制范围尽量小：放在具体的函数或语句上，不放在整个模块上。全仓库统一的 lint 策略写在根 `Cargo.toml` 的 `[workspace.lints]`，不用 `#![allow(...)]`。
- 禁止用 `#[expect(clippy::too_many_arguments)]` 绕过参数上限，改用结构体。
- 禁止 `#[allow(dead_code)]`：没用的代码直接删。

```rust
// ✅ 正确：只在非 macOS 构建里没有调用方
#[cfg_attr(not(target_os = "macos"), expect(dead_code, reason = "only the Seatbelt backend calls this"))]
fn sandbox_exec_path() -> &'static Path { ... }

// ❌ 错误：函数早已被调用，allow 却一直留着，也不会有任何提示
#[allow(dead_code)]
pub(crate) fn china_now() -> PrimitiveDateTime { ... }
```

## 7. 不留兼容痕迹

| 禁止 | 正确做法 |
|---|---|
| 把不用的变量改名为 `_x` 保留 | 删除 |
| 注释掉旧代码或写 `// removed` | 删除，历史交给 git |
| 旧路径重新导出新类型 | 改完所有引用 |
| 新旧两套实现并存、用开关切换 | 一次切换，删除旧实现 |
| `#[deprecated]` 标记后保留 | 删除并更新调用方 |

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 新代码让超限的存量文件继续变大，或新文件超过 800 行
- 函数超过 60 行、参数超过 5 个、嵌套超过 4 层
- 层间用裸 `String` 传 ID；用 bool 或字符串表示多状态
- 上限、阈值、超时写成字面量，或没有注明出处
- `utils.rs` / `helpers.rs` / `common.rs` 之类的归堆模块
- 不属于 §6 例外的 `#[allow(...)]`；没有 `reason` 的 `#[expect(...)]` / `#[allow(...)]`；`#[allow(dead_code)]`；模块级的抑制
- 注释掉的代码、兼容层、新旧双路径
