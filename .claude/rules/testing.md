# 测试规范

## 1. 放在哪里

| 类型 | 位置 | 适用 |
|---|---|---|
| 单元测试 | 源文件内 `#[cfg(test)] mod tests` | 纯逻辑：解析、渲染、路径判定、预算计算 |
| 集成测试 | `crates/<crate>/tests/*.rs` | 跨模块行为、对外 API、验收条目 |
| PostgreSQL 测试 | `crates/openwork-core/tests/postgres_*.rs` | 存储层读写与迁移 |
| 真机沙箱测试 | `crates/openwork-sandbox/tests/` | 真实调用 `sandbox-exec` 的行为 |
| 前端测试 | 与模块同目录的 `*.test.ts(x)` | reducer、store、视图模型、组件 |

## 2. 命名

- 测试名写**行为和结果**，读起来是一句话：`spilled_output_is_readable_without_approval_and_not_writable`。
- 禁止 `test_1`、`it_works`、`basic`、`normal_case`。
- 对应设计文档验收条目的测试以 `acc_NN_` 开头，并在文档注释中写出条目来源：

```rust
/// tools.md §12 #24: at most 100 paths, newest first, exact total, full list on disk.
#[tokio::test]
async fn acc_24_returns_the_newest_hundred_with_an_exact_total() { ... }
```

## 3. 断言

- 断言具体结果，而不只是 `is_ok()` / `!is_error()`。模型可见文本逐字断言，因为它就是契约。
- 失败分支要断言**没有副作用**：文件没变、数据库没写、卡片没出。

```rust
// ✅ 正确
assert_eq!(written.text_content(), "Read lib.rs before editing it.");
assert_eq!(std::fs::read_to_string(workspace.path().join("lib.rs")).unwrap(), "fn a() {}\n");

// ❌ 错误：只知道失败了，不知道是否按规定失败、是否留下副作用
assert!(written.is_error());
```

- 禁止为了通过而放宽断言（`==` 改 `>=`、删掉某条断言）。确因测试共享环境不得不放宽时，写注释说明原因，并优先改成只断言本测试自己的数据。

## 4. 隔离

- 文件系统用 `tempfile::TempDir` 或测试专用目录；**禁止写真实的 `~/.openwork`**，通过配置注入路径（例如 `OpenWorkCoreConfig.spill_root`）。
- PostgreSQL 测试共用一个数据库，所以每个测试只操作自己创建的数据：用唯一 ID，断言按自己的 ID 过滤，不断言整表行数。
- 不依赖执行顺序、当前时间或 `sleep` 等待；需要时间的地方注入时间，需要等待的地方等具体事件。
- 不访问真实网络和真实模型；需要真实服务的测试标 `#[ignore = "requires ..."]` 写明依赖。

## 5. 环境依赖

- PostgreSQL 测试在缺少 `TEST_DATABASE_URL` 时静默返回：

```rust
let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
    return;
};
```

  因此**必须用 `scripts/check.sh` 跑测试**，它在变量缺失时直接失败。不要把"未设置变量时通过"当成测试通过。
- macOS 真机沙箱测试用 `#[cfg(target_os = "macos")]`，**不得**标 `#[ignore]`，它们是沙箱行为的证据。
- 依赖 token 数触发的测试（压缩、修剪），阈值用 Trace 的 `requestEstimatedInputTokens` 校准，不手算。

## 6. 什么时候必须加测试

- 新行为：至少覆盖正常路径、边界值、非法输入。
- 修 bug：先写能复现的失败测试，再修。
- 路径与权限判定：大小写变体、符号链接、`..`、不存在的路径、带引号、空格、正则元字符、非 ASCII 的路径。
- 两份实现表达同一规则时（例如文件工具的路径判断与 Seatbelt 规则），加对等测试把它们绑在一起。

## 7. 前端测试

- 用 vitest。逻辑放在纯函数模块里（`runtimeReducer.ts`、`transcript.ts`）直接测，组件测试只测交互与渲染结果。
- i18n 的三种语言键结构由 `i18n.test.ts` 保证一致，新增文案三种语言一起加。

## 8. 运行

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork_test scripts/check.sh
cargo test -p openwork-tools --test read_before_edit     # 单个集成测试文件
cargo test -p openwork-sandbox tiers::tests               # 单个模块的单元测试
pnpm --dir desktop test -- transcript                     # 单个前端测试
```

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- 测试名不描述行为；验收测试没有 `acc_NN_` 前缀或没有注明条目来源
- 只断言成功或失败，不断言具体结果；失败分支不检查副作用
- 为通过测试放宽断言且没有说明
- 测试写真实 `~/.openwork`、访问真实网络，或依赖执行顺序、当前时间、`sleep`
- PostgreSQL 测试断言整表状态，而不是只断言自己的数据
- 真机沙箱测试被标 `#[ignore]`
- 修 bug 没有先加复现测试
