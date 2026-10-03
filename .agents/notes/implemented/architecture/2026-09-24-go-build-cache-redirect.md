# Agent Note: Go 编译缓存重定向到 OpenWork 私有的临时目录

Status: implemented

## 问题

`openwork-sandbox` crate 的真机矩阵在 `auto` 下探测了常用工具链。Go 默认把编译缓存写到 `~/Library/Caches/go-build`。它在可写根之外，所以沙箱拒绝了 `go build` 与 `go test`。所以要决定：要不要把工具链的缓存目录默认加入可写根。

Go 的模块缓存 `~/go/pkg/mod` 有同类问题。首次下载依赖时，`go` 要写它。

## 决策

- `SandboxEnvironment::bash_environment`（`crates/openwork-sandbox/src/policy.rs`）返回 `GOCACHE=<临时根>/openwork/go-build`。临时根取 `temp_roots` 的最后一项。`SandboxEnvironment::detect` 把规范化后的 `$TMPDIR` 排在最后。
- bash 启动时把这组变量叠加到会话环境上，覆盖同名变量（`crates/openwork-tools/src/builtins/process/bash.rs` 的 `environment`）。
- 可写根不变。`~/Library/Caches/go-build` 与 `~/go/pkg/mod` 都不可写。模块缓存也不重定向。已缓存的模块只读，构建不受影响。首次下载依赖时，命令被拒，模型再申请越界。
- 测试：sandbox `matrix::go_builds_and_tests_with_the_private_cache`（真实内核；没有安装 `go` 时跳过）、`policy::bash_environment_points_the_go_cache_at_a_private_temp_directory`；tools `sandbox_calls::bash_sees_the_private_go_cache`。

设计见 [permissions.md §5、§15 #50](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**把 `~/Library/Caches/go-build` 加入可写根。** 这样不用改环境，缓存也与用户共享。没有采用：如果沙箱内的进程能写这份缓存，用户日后在沙箱外编译时就可能取到篡改过的产物。它还要改路径四档（permissions.md §3）。工具链缓存是否默认可写，留给[按 Trace 数据决定的提议](../../proposed/architecture/2026-09-24-trace-driven-cache-and-session-grants.md)。

**把 Go 模块缓存加入可写根，或同样重定向。** 没有采用。原始记录只写了结论：保持现状，首次下载走越界。

## 后果

- 可写范围不变，`go build` 与 `go test` 不需要越界。
- 私有缓存不与用户的缓存共享。首次编译更慢，系统清理临时目录后要重建。
- 没有设置 `$TMPDIR` 时，最后一个临时根是 `/private/tmp`。这时缓存位于 `/private/tmp/openwork/go-build`，不是按用户隔离的目录。
- 新增依赖后的首次下载要批准一次。`escalationPaths` 记录这些申请。[按 Trace 数据决定的提议](../../proposed/architecture/2026-09-24-trace-driven-cache-and-session-grants.md)用这些数据决定模块缓存是否默认可写。
- npm、pnpm、pytest 的结果由 `matrix::toolchain_probes_are_recorded` 记录，不断言。
