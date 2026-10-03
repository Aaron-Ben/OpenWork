# 测试

本文规定测试的分层、原则、写法与运行方式。做法参照 DSH 的 `docs/testing.md`。完成的标准见根 [CLAUDE.md](../CLAUDE.md)。

## 1. 分层

| 层 | 位置 | 运行 | 依赖 |
|---|---|---|---|
| 单元 | 源文件内 `#[cfg(test)] mod tests`；前端 `desktop/src/**/*.test.ts(x)` | `cargo test`、`pnpm --dir desktop test` | 无 |
| 集成 | `crates/<crate>/tests/*.rs` | `cargo test -p <crate> --test <文件>` | 无 |
| 契约 | 外部协议的请求与响应，由本地 mock 服务器回放 | 同集成 | 无，不需要 API key |
| PostgreSQL | `crates/openwork-core/tests/postgres_*.rs` 与协作测试 | 同集成 | `TEST_DATABASE_URL`；协作另需 `TEST_REDIS_URL` |
| 真机沙箱 | `crates/openwork-sandbox/tests/`、`crates/openwork-tools/tests/sandbox_calls.rs` | 同集成 | macOS 的 `sandbox-exec` |
| 真实 API | 标 `#[ignore = "requires …"]` 的测试 | `cargo test -p <crate> --test <文件> -- --ignored` | API key 与网络 |

- PostgreSQL 测试在缺少 `TEST_DATABASE_URL` 时静默返回。所以必须用 `scripts/check.sh` 运行全量测试，它在变量缺失时直接失败。
- 真机沙箱测试用 `#[cfg(target_os = "macos")]`，不得标 `#[ignore]`。它们是沙箱行为的证据。
- 真实 API 测试不进 `scripts/check.sh`。改动模型调用、Provider 预设或模型可见的行为时，手动运行相关的真实 API 测试，并在汇报中写明结果。

## 2. 原则

- **只 mock 昂贵或不确定的边界。** 模型服务、网络与时钟可以替换，下游一律用真实实现。手写的替身只能证明数据能流过，证明不了真实组件的行为。
- **验证真实世界，不相信自我报告。** 断言重新读取文件或数据库，不只看被测对象返回了什么。失败分支断言没有副作用：文件没变、没有写库、没有出卡片。
- **走真实入口。** 模型调用的契约测试经 HTTP 打到本地 mock 服务器，不直接调用内部的解析函数。
- **测试自己拥有资源。** 临时目录、端口与子进程由测试创建，并在测试结束时释放，失败时也一样。只在单独运行时才通过的测试，是测试本身的缺陷。

## 3. 写法

- 测试名写行为与结果，读起来是一句话，例如 `spilled_output_is_readable_without_approval_and_not_writable`。禁止 `test_1`、`it_works`、`basic`。
- 对应子系统页验收条目的测试以 `acc_NN_` 开头，并在文档注释中写出条目来源，例如 `/// tools.md §10 #24: …`。
- 断言具体结果，不只断言 `is_ok()`。模型可见的文本逐字断言，因为它就是契约。
- 禁止为了通过而放宽断言，例如把 `==` 改成 `>=`、删掉一条断言。因共享环境确实需要放宽时，写注释说明原因，并优先改为只断言本测试自己的数据。

## 4. 隔离

- 文件系统用 `tempfile::TempDir` 或测试专用目录。禁止写真实的 `~/.openwork`，路径通过配置注入。
- 共用一个数据库的测试，只操作自己创建的数据：使用唯一 ID，按自己的 ID 过滤，不断言整表行数。
- 不依赖执行顺序、当前时间或 `sleep`。需要时间时注入时间，或使用 `tokio` 的虚拟时钟。需要等待时，等具体事件。

## 5. 什么时候必须加测试

- 新行为：至少覆盖正常路径、边界值与非法输入。
- 修 bug：先写能复现的失败测试，再修。
- 外部协议的改动：先录制或更新契约样例，再改代码。
- 路径与权限判定：覆盖大小写变体、符号链接、`..`、不存在的路径、带空格与非 ASCII 的路径。
- 两份实现表达同一条规则时（例如文件工具围栏与 Seatbelt 规则），加对等测试把它们绑在一起。

## 6. 运行

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork_test scripts/check.sh   # 全量检查
cargo test -p openwork-tools --test read_before_edit                                          # 单个集成测试文件
cargo test -p openwork-sandbox tiers::tests                                                   # 单个模块的单元测试
pnpm --dir desktop test -- transcript                                                         # 单个前端测试
```

开发时先运行改动相关的测试；完成时运行 `scripts/check.sh`。
