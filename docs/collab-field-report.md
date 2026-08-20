# 协作模式真实环境取证报告

> 取证日期：2026-08-20（Asia/Shanghai）
> 实验窗口：21:05:52–21:22:23，共 16 分 30.782 秒
> 范围：只取数；未修改业务代码，未创建看板或卡片。

## 0. 前次报告更正

前次取证停在前提检查处的决定正确：当执行环境不能可信验证前提时，不应启动真实模型。需要更正的是“本机缺少前提”这一结论；同机复验和本次非沙箱执行证明，先前两项都是受限环境造成的假失败。

| 检查项 | 前次报告记录 | 同机复验 / 本次实际观测 |
|---|---|---|
| PostgreSQL | `localhost:5432 - no response` | `accepting connections`；`openwork-postgres Up (healthy)`；PostgreSQL 16.14 |
| OpenCode 登录态 | `FileSystem.open: .../opencode.log` | `1 credentials`（OpenCode Go / api） |
| OpenCode 版本 | `1.18.18` | `1.18.18` |

这两种失败形状应判为“当前环境受限”，不是“机器缺前提”。正确处置仍是停止并换到能直接访问本机的环境，而不是绕过前提检查。

## 1. 环境、模型与总量

### 1.1 前提与隔离

```text
$ opencode --version
1.18.18

$ pg_isready -h localhost -p 5432
localhost:5432 - accepting connections

$ docker ps --filter name=openwork-postgres
openwork-postgres  Up 58 minutes (healthy)

$ psql ... -c 'SELECT version();'
PostgreSQL 16.14 on aarch64-unknown-linux-musl ... 64-bit

$ opencode auth list
OpenCode Go  api
1 credentials
```

- 独立数据库：`openwork_field`。
- 独立 home：`/Users/xuenai/.openwork/collab-field`。
- daemon ready：socket `/Users/xuenai/.openwork/collab-field/daemon.sock`，MCP `http://127.0.0.1:64035/mcp`，OpenCode PID `21172`、generation `1`。
- Alice、Bob 的 `scannerEnabled` 均为 `false`；看板、卡片均为 0。

### 1.2 模型

`opencode models` 返回 27 个模型、两个 provider。本轮优先选择了 brief 指定的免费层：

| Agent | provider | model | 首次真实唤醒 |
|---|---|---|---|
| Alice | `opencode` | `deepseek-v4-flash-free` | 在 alpha 发布 `FREE_OK Alice` |
| Bob | `opencode` | `deepseek-v4-flash-free` | 在 alpha 发布 `FREE_OK bob` |

免费层首轮可用，因此没有回退到 `opencode-go/*`。下列 token 是 OpenCode 上报的模型用量；由于使用 free 模型，不能把它解释成订阅额度扣减。

未配置 triage support model。`collab_triages` 仅记录 fallback：`fail_open=true` 55 条、`fail_closed=false` 26 条、`rate_limited=false` 1 条；triage input/output token 不适用。

### 1.3 全部 run 与 token

```text
 total_runs | completed | running | input_tokens | cached_input_tokens | output_tokens
------------+-----------+---------+--------------+---------------------+--------------
         47 |        47 |       0 |       148744 |             3418112 |          6378
```

| trigger / outcome | run 数 | input | cached input | output |
|---|---:|---:|---:|---:|
| message / acted | 25 | 78,487 | 1,712,512 | 3,745 |
| message / unpublished | 20 | 68,776 | 1,632,896 | 2,450 |
| agenda / unpublished | 1 | 1,481 | 72,704 | 183 |
| rerun / silent | 1 | 0 | 0 | 0 |

## 2. M1 · 唤醒成本是否仍随房间历史增长

### 2.1 数字

正式样本为 Alice 的 8 个 message run；每轮在 alpha 连续发送 3 条长度相近的消息，并等 Alice、Bob 都 idle 后才进入下一轮。alpha 在第 8 批结束时序号为 43，已超过 brief 要求的 30 条历史。

| 轮次 | run id | alpha 批后序号 | outcome | input | 与上轮差值 | cached input | output | 备注 |
|---:|---|---:|---|---:|---:|---:|---:|---|
| 1 | `run_715678e0067c4587843dfc744ae842b3` | 15 | acted | 5,195 | — | 56,320 | 199 | 首轮明显离群 |
| 2 | `run_bdf1f3b36ae74a35bc9dabc5ff45c40c` | 19 | acted | 1,025 | -4,170 | 62,208 | 77 | |
| 3 | `run_45867fde9e0f49c2bcf9b1a4fee991cd` | 23 | acted | 1,060 | +35 | 63,744 | 75 | |
| 4 | `run_521de88eab934516bb73bdfe39a46d05` | 27 | acted | 1,095 | +35 | 65,280 | 75 | |
| 5 | `run_19124c6faed04d7ab339b2f4d0e706de` | 31 | acted | 872 | -223 | 67,072 | 75 | |
| 6 | `run_6aa70aa8fa374601bbe9fb5aaf0ede2b` | 35 | acted | 902 | +30 | 68,608 | 75 | |
| 7 | `run_659eb7da7fe6448499f45448d1a5ec56` | 39 | acted | 941 | +39 | 70,144 | 77 | |
| 8 | `run_536c25407fce4455bb4634a77f0dc016` | 43 | acted | 908 | -33 | 74,752 | 77 | 第 7/8 轮间发生 agenda run，cached 差值不可直接比较 |

事件表中没有 compact/compaction 记录，运行时也未观察到 `compacting`；不能从本次样本证明发生过压缩。

### 2.2 结论

第 2–8 轮的 input 为 `1025, 1060, 1095, 872, 902, 941, 908`，相邻差值为 `+35, +35, -223, +30, +39, -33`。在房间历史持续增长时，差值没有随轮次增长，样本呈大致持平而非累计加速。第 1 轮的 5,195 是离群值，但它之后没有形成上升趋势。

未执行成本更高的修复前提交 `94fa15c` 对照实验。

## 3. M2 · 多房间 digest 路由正确性

### 3.1 run 数与发送间隔

同一组消息会分别唤醒 Alice、Bob，因此“一条 run”按 `agent_id` 判断。三组中每个 Agent 都恰好只有一条 message run。

| 样本 | 两条 send 间隔 | Alice run | Bob run |
|---:|---:|---|---|
| 1 | 12.474 ms | `run_61dfc3aba22a4a499480b7b7fd6be938`（acted） | `run_22d73223071549adac6cb50033ba4cc8`（unpublished） |
| 2 | 12.723 ms | `run_a9cfad44d59b4c7f9752f18443c6254b`（acted） | `run_aa0b8683e2c54e31952dbe410ef71b46`（acted） |
| 3 | 10.565 ms | `run_adbc65111dc7400f8842ec28d472ab6a`（acted） | `run_edc52d607a1541a1bd9db879ec8934bb`（acted） |

### 3.2 原始问题与发布内容

| 样本 | 房间 | 用户原文 | 实际发布者 / 正文 | 对应本房间问题 |
|---:|---|---|---|---|
| 1 | alpha | `M2-1 Alpha：天气问题。请只回答：需要城市。` | Alice：`需要城市` | 是 |
| 1 | beta | `M2-1 Beta：1+1 等于几？请只回答数字。` | Alice：`2` | 是 |
| 2 | alpha | `M2-2 Alpha：把英文 cat 翻译成中文，只回答译文。` | Bob：`猫` | 是 |
| 2 | beta | `M2-2 Beta：7 是不是质数？只回答是或否。` | Bob：`是` | 是 |
| 3 | alpha | `M2-3 Alpha：水在标准大气压下多少摄氏度沸腾？只回答数字和单位。` | Bob：`100°C` | 是 |
| 3 | beta | `M2-3 Beta：法国首都是哪里？只回答城市名。` | Bob：`巴黎` | 是 |

竞争方的 reply 被 HELD 后，随后使用 `ack` 或 reaction 结算；这不是房间被忽略。

### 3.3 结论

- 3/3 组均实现每 Agent 一条跨房间 run。
- 6/6 个房间答案都对应本房间问题。
- 串台：0。
- 被完全忽略的房间：0。

本次真实模型样本支持“一个 Agent 的跨房间 digest 能按 `roomId` 正确路由回复”这一重构假设。

## 4. M3 · Agent 是否调用 `ack`

### 4.1 场景与原始结果

连续 10 轮在 alpha 发送：`M3-NN：bob，这个交给你看一下就行；请回复收到 NN。`。每轮等两位 Agent idle 后再继续。Bob 逐轮发布 `收到 01` 至 `收到 10`。

| 轮次 | Alice 结算 | Bob 结算 | carriedOver（Alice / Bob） | digest 房间数（Alice / Bob） |
|---:|---|---|---|---:|
| 1 | `ack alpha` | reply `收到 01` | `[] / []` | `1 / 1` |
| 2 | `ack alpha` | reply `收到 02` | `[] / []` | `1 / 1` |
| 3 | `ack alpha` | reply `收到 03` | `[] / []` | `1 / 1` |
| 4 | `ack alpha` | reply `收到 04` | `[] / []` | `1 / 1` |
| 5 | `ack alpha` | reply `收到 05` | `[] / []` | `1 / 1` |
| 6 | `ack alpha` | reply `收到 06` | `[] / []` | `1 / 1` |
| 7 | `ack alpha` | reply `收到 07` | `[] / []` | `1 / 1` |
| 8 | `ack alpha` | reply `收到 08` | `[] / []` | `1 / 1` |
| 9 | `ack alpha` | reply `收到 09` | `[] / []` | `1 / 1` |
| 10 | `ack alpha` | reply `收到 10` | `[] / []` | `1 / 1` |

原始汇总：

```text
agent_id | inbox.acked calls
---------+------------------
alice    | 10
bob      | 0

20 条 inbox.settled：advanced 均为 ["alpha"]，carriedOver 均为 []，digest_rooms 均为 1。
Alice 的 10 条 settled：ackedRooms=["alpha"]；Bob 的 10 条：ackedRooms=[]。
```

### 4.2 结论

- 针对“明确点名 Bob、Alice 不需回答”的目标场景，Alice 调用 `ack` 10/10 次。
- `carriedOver` 序列始终为 0，不单调增长，也不存在反复滞留的同一批房间。
- digest 房间数始终为 1，没有随轮次变多。

因此本次模型/standing prompt 组合没有复现 R-N10 的系统性漏调 `ack`。这只是该模型与该场景的 10 轮样本，不外推到所有模型和复杂对话。

## 5. M4 · 注入与主动性

| 现象 | 数量 | 原始记录 / 判断 |
|---|---:|---|
| `prompt.injected` | 1 | 21:06:08，Alice 初始 run `run_f8c27a73dec1443da87f134d15aef8a8` 接住连通性消息 |
| 注入吞掉前一问题 | 0 个可确认样本 | 同一 run 最终发布 `FREE_OK Alice`；未见前一条需要发布却消失 |
| 注入预算耗尽 | 0 | 仅 1 次注入，没有连续 4 次形态 |
| `trigger='agenda'` | 1 | `run_0988f0dfb2ee4970a0f81c183254a79c`，Alice，focused beta，unpublished |
| `trigger='idle'` | 0 | 未观测到 |
| `trigger='scanner'` | 0 | 未观测到；scanner 已关闭 |

agenda run 发生于 21:15:16–21:15:22，input/cached/output 为 `1481 / 72704 / 183`。它调用 `ack` 结算 alpha 与 beta，`carriedOver=[]`，没有发布消息。16.5 分钟窗口内只出现 1 次 agenda run；随后有 1 条 `rate_limited` triage 记录（`stalled room is already claimed or cooling down`），没有再产生主动 run。

## 6. 额外观测与边界

### 6.1 拓扑建立本身触发了真实 run

`room-add` 产生的成员加入 system message 在正式连通性消息发送前已经触发两条 message run：

| Agent | run | outcome | input / cached / output |
|---|---|---|---:|
| Alice | `run_f8c27a73dec1443da87f134d15aef8a8` | acted（后续接住注入） | `24361 / 71424 / 794` |
| Bob | `run_539c5b6ac2bf4861bb62061b716b92b4` | unpublished | `22829 / 22784 / 106` |

这两条共消耗 47,190 input tokens，且发生在正式测试问题之前。本轮只记录，不判断它是否应修改。

### 6.2 解释边界

- M1 没有修复前提交对照，结论只针对当前提交的 8 轮趋势。
- 未配置真实 support-model triage，所以不能评价整收件箱模型裁决质量。
- `carriedOver=[]` 是本样本事实；即便非空也不能单凭非空判 bug。
- free 模型成功意味着本次不需要订阅模型回退，但 token 数仍是理解上下文成本的重要数据。

## 7. 未能测到的内容

| 未测项 | 原因 | 影响 |
|---|---|---|
| 真实 support-model triage | 没有可用且已配置的 OpenWork support provider；brief 明确不为此申请凭证 | M3 只覆盖主模型的 ack 行为，不覆盖 support-model 裁决 |
| 修复前 `94fa15c` 对照 | 可选且成本较高，本轮未执行 | M1 只能判断当前趋势，不能给出前后量化差 |
| 注入预算上限形态 | M4 不要求刻意构造，样本只有 1 次注入 | 不能评价第 5 次注入转 rerun 的真实行为 |
| compaction 前后趋势 | 未观察到可确认 compaction | 不能评价压缩点两侧成本 |
| 多房间下 `carriedOver` 逐轮累积 | M3 十轮的 digest 都只有 alpha 这 1 个房间，没有构造另一个持续未结算的房间 | M3 证明该模型会在单房间点名场景调用 `ack`，但不能回答多房间中未结算房间是否会逐轮堆积 |

## 8. 收尾

数字与原始记录已在删除隔离数据库前取全。收尾结果：

| 项目 | 结果 |
|---|---|
| `openwork-collab shutdown` | `{"shuttingDown": true}`，daemon 终端随后输出 `daemon.stopped=true` 并以 0 退出 |
| `dropdb openwork_field` | 已执行；从 `pg_database` 复查计数为 `0` |
| 删除 `/Users/xuenai/.openwork/collab-field` | 已执行；路径复查结果为 `FIELD_HOME_REMOVED` |
| 业务代码变更 | 本轮新增 0；只覆盖本报告。工作树中已有的 Rust/文档改动早于本轮，不归因于本次取证 |
