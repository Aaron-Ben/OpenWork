# 协作模式：真实环境取证 · 实施 brief

**这是给实施者的工单。** 语义权威是 [collaboration.md](collaboration.md)。

**这一轮不写业务代码、不改行为，只取数。** 观测到问题就如实记下来，不要顺手修——修什么、怎么修是下一轮的决定，而这一轮的价值恰恰在于给那个决定提供数字。

产出是一份报告：`docs/collab-field-report.md`。

---

## 0. 先读这段：这一轮会花掉真实额度

协作模式的推理跑在你本机的 `opencode serve` 里，用的是**用户自己的订阅额度**。而额度闸（P7 循环硬顶）**尚未实现**——[collaboration.md §15](collaboration.md) 的 R2 与 R-N4 都还成立：主动性可以在无人值守时持续唤醒，`task` 子 Agent 全开，token 消耗是无上限的乘法。

因此这一轮必须在**有人看着**的情况下跑，并且按 §1 的边界收敛。跑完立刻 `openwork-collab shutdown`，**不要让它挂着过夜**。

如果任何一步的行为超出预期（Agent 反复自我唤醒、日志里 run 一条接一条），**先 shutdown 再分析**。

---

## 1. 环境准备

### 1.0 这一轮必须在能真正访问本机的环境里跑

**沙箱化的执行环境会在前提检查上报假失败。** 2026-08-20 的第一次尝试就是这样停下的：报告里 PostgreSQL 是 `no response`、`opencode auth list` 读日志文件失败，而在同一台机器上直接执行，两者都正常。

两个失败的形状要认得出来——它们是环境限制，不是机器缺前提：

| 观测到 | 实际含义 |
|---|---|
| `pg_isready` 返回 `no response`，但 `docker ps` 显示容器 healthy | 执行环境连不上 localhost 端口 |
| `opencode auth list` 报 `FileSystem.open: ~/.local/share/opencode/log/opencode.log` | 执行环境没有该目录的写权限 |

碰到这两条中的任何一条，**先判断是不是沙箱**（`docker ps`、直接 `psql` 连一次），再决定是停止还是换环境。真的缺前提才停；是沙箱就换到不受限的环境跑，不要在受限环境里硬试。

### 1.1 前提检查（任一不满足就停下来报告，不要绕过）

```bash
opencode --version          # 期望 1.18.18；差别较大时记进报告，不要自行升级
pg_isready -h localhost -p 5432
```

opencode 必须已经登录过（`opencode auth list`）。没登录就跑，表现是第一次唤醒时报错而不是启动时报错。

### 1.2 用一个独立数据库，不要污染现有数据

```bash
createdb -h localhost -U openwork openwork_field
export DATABASE_URL="postgres://openwork:openwork@localhost:5432/openwork_field"
export OPENWORK_COLLAB_HOME="$HOME/.openwork/collab-field"
```

`OPENWORK_COLLAB_HOME` 也要换——Agent 的 home 目录、`daemon.sock` 都在那底下，复用会和现有状态串味。

### 1.3 起 daemon

```bash
cargo run -p openwork-collab --bin openwork-collab -- daemon
```

另开一个终端跑后面的命令（它们通过 socket 与 daemon 通信）。

### 1.4 建两个 Agent，两个房间

**`scanner-enabled` 一律传 `false`**，关掉跨房间扫描那条主动性路径。

```bash
openwork-collab agent-create Alice <provider> <model> "你是 Alice，务实、话少。" false alice
openwork-collab agent-create Bob   <provider> <model> "你是 Bob，喜欢追问细节。" false bob
```

`<provider>` / `<model>` 是 **OpenCode 的** provider 与 model，不是 OpenWork 的 provider。填错的失败发生在第一次唤醒时，不在创建时。

2026-08-20 在本机实测，`opencode models` 给出 27 个，分两个 provider：

- **`opencode/*-free`**（如 `opencode/deepseek-v4-flash-free`、`opencode/hy3-free`）—— 免费层，**优先用它**；
- `opencode-go/*`（如 `opencode-go/deepseek-v4-flash`）—— 走订阅额度。

同日 `opencode auth list` 只显示一条 `OpenCode Go` 凭证，**因此 `opencode/*-free` 是否真的可用没有验证过**。做法：先用 free 的建 Agent 跑一轮，第一次唤醒就会给出答案；不可用再退回 `opencode-go/*` 里最便宜的那个，并**在报告里写明用的是哪个、为什么退回**。

free 层能用的话，§0 的额度顾虑基本消失，可以把轮次跑得更足。

```bash
openwork-collab room-create Alpha alpha
openwork-collab room-create Beta  beta
for r in alpha beta; do for p in alice bob; do openwork-collab room-add $r $p; done; done
```

**不要建任何看板或卡片。** agenda 主动性靠"指派/提及的未完成卡片"和"停滞房间"两类信号；没有卡片就少一半燃料。停滞房间那一半关不掉，但有 decline 上限 3 兜底（§11.1）。

### 1.5 triage 模型：可选

配了的话（`openwork-collab triage-config <provider> <model>` + `credential-check`）就会走真实的整收件箱裁决。没配也能跑——triage 失败时对"有人类在等"的场景 fail open，照常唤醒（§8.2）。

**没配的代价写进报告**：M3 的裁决部分观测不到，M1/M2 不受影响。不要为了配它去申请新凭证，配不上就如实记下来。

---

## 2. M1 · 唤醒成本还随房间历史增长吗（§16 #46）

### 这里有一个必须避开的测量陷阱

**不要直接比较 `collab_runs.input_tokens` 的绝对值。**

每个 Agent 的 OpenCode session 是**持久**的（§3.1），transcript 会一直累积到压缩为止。所以 `input_tokens` 本来就会随轮次增长，**跟这次修的东西无关**。照绝对值看，你会得出"没修好"的错误结论。

该看的是**相邻两轮之间的增量**：

- 修复前：每轮把整个房间历史重新注入一次 → 增量本身随房间历史增长（累计呈二次）
- 修复后：每轮只注入自上一轮以来的未读 → **增量应当大致持平**（累计呈线性）

另外 `cached_input_tokens` 会让绝对值进一步失真，一并记录但不作为判据。

### 做法

在房间 alpha 里以 `user` 身份分批发消息，每批之间等 Agent 回合结束（`openwork-collab status` 看是否还在忙）：

```bash
openwork-collab send alpha user "第 1 批：<随便一句需要回应的话>"
```

至少跑 **8 轮**，每轮 2–3 条消息，让房间历史累积到 30 条以上。

然后取数：

```sql
SELECT id, agent_id, trigger, outcome,
       input_tokens, cached_input_tokens, output_tokens, started_at
  FROM collab_runs
 WHERE agent_id = 'alice' AND status = 'completed'
 ORDER BY started_at;
```

### 报告里要给出

- 一张表：轮次 → `input_tokens` → **与上一轮的差值**；
- 一句结论：这个差值是持平还是随轮次增长；
- 中途如果发生了压缩（`collab_events` 里 `outcome`/活动出现 compaction，或 `input_tokens` 突然回落），**在表里标出来**——压缩点前后的增量不可直接比较。

**可选、成本较高**：在 `94fa15c`（修复前的提交）上用另一个数据库跑同样的脚本，给出对照数字。做不了就跳过，不要为此耗时间。

---

## 3. M2 · 多房间 digest 的路由正确性（优先级最高）

**这一条从来没有用真实模型验证过。** 整个"按 Agent 唤醒整个收件箱"的重构，建立在一个假设上：模型看到 `rooms: [{roomId, roster, unread}]` 之后，能分清哪条消息属于哪个房间，并用正确的 `room_id` 调 `openwork_reply`。假设不成立的话，重构就是错的，而单元测试**测不出来**——它们只验证了 prompt 组装得对。

### 做法

制造一次"同一个 Agent 的两个房间同时有未读"的唤醒。关键是让两条消息落在同一个 debounce 窗口（2.5 秒）里：

```bash
openwork-collab send alpha user "Alpha 房间的问题：今天天气如何？"
openwork-collab send beta  user "Beta 房间的问题：1+1 等于几？"
```

两条要**连着发**。发完看 `collab_runs`：如果产生了两条 run 而不是一条，说明没落进同一个窗口，调整间隔重试并在报告里说明。

重复 **3 次**，每次换不同的问题，且两个房间的问题要**明显不可互换**（这样答错房间一眼能看出来）。

### 报告里要给出

- 每次唤醒是不是**只有一条 run**（这是"按 Agent 唤醒"生效的证据）；
- 每个房间收到的回复内容，是否对应**该房间的问题**；
- 有没有出现"把 Alpha 的答案发进 Beta"的串台；
- 有没有出现某个房间被完全忽略。

**串台或忽略，就是这一轮最重要的发现**，请原样贴出消息内容，不要概括。

---

## 4. M3 · Agent 到底会不会调 `ack`（R-N10）

逐房间结算的设计押在一件事上：模型会对"读过但不打算答"的房间调 `openwork_ack`。不调的话，那些房间保持未读、每轮都回到 digest 里——[collaboration.md §15](collaboration.md) 的 **R-N10** 记的就是这个。这是刻意选的失败方向（多花钱，不丢消息），但它的**真实频率没人量过**。

### 做法

在 M2 的场景上继续，制造一些"明显不需要这个 Agent 回答"的消息，比如在 alpha 里点名 bob：

```bash
openwork-collab send alpha user "bob，这个交给你看一下就行"
```

跑 **10 轮以上**，然后取每轮的结算记录：

```sql
SELECT created_at, run_id, agent_id, kind, payload
  FROM collab_events
 WHERE kind IN ('inbox.settled', 'inbox.acked')
 ORDER BY created_at;
```

`inbox.settled` 的载荷里有 `advanced`（推进了的房间）、`carriedOver`（留到下一轮的）、`ackedRooms`。

### 报告里要给出

- **`ack` 被调用过几次**（`inbox.acked` 的条数）；
- 每轮 `carriedOver` 的房间数，按时间列出；
- **`carriedOver` 是否单调增长**、是否总是同一批房间；
- 每轮 digest 里 `rooms` 的数量有没有随之变多。

如果 `ack` 一次都没被调用：**不要改代码**。记下来，并把当时的 `AGENTS.md` 全文贴进报告——那说明 standing prompt 没把这件事说清楚，而改 prompt 是下一轮的事。

---

## 5. M4 · 顺带盯着（不必刻意构造）

这三条是文档里记着、但从未在真实环境见过的形态。看到了就记，没看到也如实说"未观测到"。

| 现象 | 对应 | 怎么认出来 |
|---|---|---|
| 前一轮的结论被后来的注入吞掉 | **R-N7 / R-N8** | 日志里有 `prompt.injected`，而那一轮最终只回答了后一个问题，前一个石沉大海 |
| 注入预算用尽 | §8.1 | 连续 4 条 `prompt.injected` 之后不再出现，改由 rerun 接手 |
| 主动性自行唤醒 | §11.1 | `collab_runs.trigger` 出现 `agenda` / `idle` / `scanner` |

第三条特别要留意：我们关掉了 scanner、也没建卡片，**但停滞房间那条路径关不掉**。如果 `trigger='agenda'` 的 run 频繁出现，记下频率——那直接关系到 P7 该把额度闸设在哪。

---

## 6. 报告格式

写进 `docs/collab-field-report.md`，开头交代清楚：

- opencode 版本、用的 provider / model、triage 模型配了没有；
- 总共跑了多少轮、消耗的 token 总量（`SELECT sum(input_tokens), sum(output_tokens) FROM collab_runs`）；
- 每个测量（M1–M4）各一节，**先给数字/原始记录，再给结论**；
- 最后一节：**没能测到的东西**，以及为什么。

数字比叙述重要。能贴 SQL 输出就贴原始输出，不要只写"表现正常"。

---

## 7. 明确不要做

| 不要 | 理由 |
|---|---|
| 因为观测到问题就改代码 | 这一轮只取数。改什么取决于数字长什么样，而现在还没有数字 |
| 因为 `ack` 没被调用就加自动结算 | 那会把逐房间结算修掉的丢消息 bug 请回来 |
| 因为 `carriedOver` 非空就判定有问题 | 它非空是设计中的正常状态，要看的是**趋势** |
| 顺手实现 P7 额度闸 | 它的参数就来自这一轮的数字 |
| 让 daemon 无人值守地长跑 | 没有额度闸，见 §0 |
| 在现有 `openwork` 库上跑 | 用 §1.2 的独立库 |

---

## 8. 收尾

```bash
openwork-collab shutdown
dropdb -h localhost -U openwork openwork_field    # 数字已经记进报告之后再删
rm -rf "$HOME/.openwork/collab-field"
```

**先确认报告里的数字都取全了再删库**——重跑一次的成本是真金白银。
