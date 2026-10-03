# Skill

Skill 是文件系统上的可复用工作流包：一个目录、一个 `SKILL.md`，以及可选的脚本、参考文档和素材。本页描述目录契约、发现、模型看到的内容、`$` 显式选择、只读边界与 Desktop 的列表。

发现、解析、目录渲染与显式选择归 `openwork-core`（`src/skills/`、`src/context/skill_catalog.rs`、`src/context/world_state/skills_catalog.rs`）。skill 根的写保护归 `openwork-sandbox` 与 `openwork-tools`。`$` 候选框、列表与详情归 `desktop/src`。

## 1. 目录契约

### 1.1 目录结构

```text
~/.agents/skills/
└── commit/                     ← 目录名就是 skill 名
    ├── SKILL.md                （必需）
    ├── scripts/                （可选）可执行脚本
    ├── references/             （可选）按需读入的参考文档
    └── assets/                 （可选）脚本使用的模板与素材
```

| 文件 | 怎样进入上下文 | 代价 |
|---|---|---|
| `SKILL.md` | 目录列出它的路径；模型用 `read` 读，或用户用 `$` 选中（§4） | 正文的 token |
| `references/` | 模型用 `read` 读 | 文件本身的 token |
| `scripts/` | 模型用 `bash` 运行 | 只有输出的 token |
| `assets/` | 一般不进入，由脚本使用 | 0 |

### 1.2 `SKILL.md`

```markdown
---
name: commit
description: 按 Conventional Commits 规范生成提交信息。当用户要求提交改动时使用。
---

# Commit

1. `git diff --staged` 看改动
2. 归纳改了什么、为什么
3. 按 `<type>: <description>` 写信息
```

- 文件必须以一行 `---` 开始，并有一行 `---` 结束 frontmatter。frontmatter 必须是合法 YAML。
- 解析器只读两个字段：`name` 与 `description`。两者出现时都必须是字符串。
- `description` 同时说明“做什么”和“什么时候用”。它是目录里唯一的判断依据。
- 其他键（`allowed-tools`、`model`、`user-invocable`、`metadata` 等）不导致失败。解析器不保留它们。
- 正文是 frontmatter 之后的部分，去掉首尾空行。

### 1.3 校验与归一化

“归一化空白”指 `split_whitespace().join(" ")`：换行、TAB 与连续空格合并为一个空格。

| 项 | 规则 | 不满足时 |
|---|---|---|
| 目录名 | 1–64 字节；只含小写 ASCII 字母、数字与 `-`；首尾是字母或数字 | 跳过，记 warning |
| `name` | 缺省时取目录名。先归一化空白，再按目录名的规则校验，并且必须等于目录名 | 跳过，记 warning |
| `description` | 必填。先归一化空白，再截断到 1024 个字符 | 缺失时跳过，记 warning |
| `SKILL.md` 大小 | 不超过 64 KiB（65,536 字节） | 跳过，记 warning |
| `SKILL.md` 编码 | UTF-8 | 跳过，记 warning |
| `SKILL.md` 类型 | 不是符号链接 | 跳过，记 warning |
| 绝对路径 | UTF-8，不含控制字符。路径不归一化 | 跳过，记 warning |

warning 是 `{ path, reason }`。`reason` 是 `SkillLoadError` 或 `SkillParseError` 的英文文本，各种原因可以区分。Desktop 原样显示它。理由见 [Agent Note：字段归一化](../../.agents/notes/implemented/architecture/2026-08-07-skill-fields-normalized-not-rejected.md)。

### 1.4 单个 skill 失败

一个 skill 解析失败时，只有它不加载。扫描继续，其他 skill 照常加载，Turn 照常开始。失败的 skill 不占用 100 个的上限（§2）。

这条规则只管发现。用户在输入框里显式选中的 skill，如果提交时已不可用，Core 在创建 Turn 前拒绝这次提交（§4.2）。

## 2. 发现

```rust
pub struct SkillRoots {
    pub agents: Option<PathBuf>,
}
```

- `OpenWorkCoreConfig::from_env_or_local()` 把 `agents_skills_root` 设为 `$HOME/.agents/skills`。`HOME` 未设置时它是 `None`。
- `OpenWorkCore` 持有一份 `SkillRoots`。world state 采样、手动压缩、显式选择、`list_skills`、`read_skill` 与沙箱的硬保护根都使用这一份。
- `SkillRoots::default()` 表示没有 skill 来源，测试可以直接使用它。
- Desktop 不推导 skill 路径。

只有这一个来源。OpenWork 不扫描 `~/.claude/skills/`、仓库里的 `.agents/skills/` 或 `.claude/skills/`、任何 `.openwork/skills/`，也没有内置 skill。OpenWork 不创建任何 skill 目录。

扫描规则：

- 先规范化根路径。根不存在时，来源为空，不记 warning。根存在但读不了时，来源为空，记一条 warning。
- 只看根下一层的子目录（`<root>/<name>/SKILL.md`），不递归。
- 跳过不是目录的项、以 `.` 开头的目录和指向目录的符号链接。这三种不记 warning。
- 目录名不是 UTF-8 或不合 §1.3 规则时，记 warning。
- 候选按目录名排序后依次加载。成功加载 100 个以后，其余每个记一条 warning。
- 结果按 `source`、`name`、`path` 排序。输入不变时，结果逐字节相同。

扫描时机：每次 world state 采样（§3.2）、每次 `list_skills`、每次显式选择各扫描一次。没有文件监听器。

## 3. 模型看到什么

### 3.1 三层

| 层 | 内容 | 进入哪里 | 什么时候 |
|---|---|---|---|
| 目录 | 每个启用 skill 的 `name`、`description`、绝对路径 | Conversation 中的 world state 消息 | 采样时目录有变化（§3.2） |
| 正文 | `SKILL.md` 正文 | Conversation | 用户用 `$` 选中（§4），或模型 `read` 它的路径（§3.3） |
| 资源 | `references/`、`scripts/` | Conversation | 模型 `read` 或 `bash` 时 |

System 前缀里没有 skill 内容。

### 3.2 目录：`skills/catalog`

目录是 world state 的 `skills/catalog` section。它的正文：

```xml
<available_skills>
Skill 是一份放在 SKILL.md 里的操作指令。下面是本会话可用的全部 skill。

- commit: 按 Conventional Commits 规范生成提交信息。当用户要求提交改动时使用。 (file: /Users/me/.agents/skills/commit/SKILL.md)
- review-pr: 逐文件审查 PR 差异并按严重度分级。 (file: /Users/me/.agents/skills/review-pr/SKILL.md)

任务落在某条描述的场景里时，先用 read 完整读它的 path 再动手，不要凭描述猜正文。
SKILL.md 里的相对路径相对它所在目录解析。有 scripts/ 就跑现成脚本，不要重写等价代码。
选中的指令文件要读完；不相关的 references/ 不要读。
</available_skills>
```

- 每个启用的 skill 一行：`- {name}: {description} (file: {path})`。
- `path` 是规范化后的绝对路径，原样输出，可以直接交给 `read`。
- 行的顺序与 §2 的结果顺序相同。输入不变时，正文逐字节相同。
- 正文总长不超过 8000 个字符，包括首尾说明。超出时，从最后一行开始整行丢弃。每丢一行记一条 warning。
- 没有启用的 skill 时，正文是“无”。

run loop 在每次 Model Call 前采样一次。阈值压缩或溢出压缩之后、重新提交之前，它再采样一次。停用集合在 Turn 开始时取一次快照；文件系统每次采样都重新扫描。

| 采样结果 | 追加的消息 |
|---|---|
| 首次出现，或压缩删掉了旧的目录消息 | 正文 |
| 正文变化，或进程重启后旧消息仍在投影里 | `以下 skill 清单取代先前提供的清单。`、空行、正文 |
| 先前有目录，现在是“无” | `先前提供的 skill 清单不再适用，当前没有可用 skill。` |
| 没有变化，或一直是“无” | 不追加 |

- 每条消息是 User-role Text Message，`message_kind = 'world_state'`，在调用 Provider 前写入 `messages`。
- 四个 section 的顺序固定为 `project_context`、`agents_md`、`skills_catalog`、`sandbox_policy`。
- 摘要请求不包含 world state 消息。
- 用户自己输入的 `<available_skills>` 文本不算目录消息。

理由见 [Agent Note：目录作为 world state section](../../.agents/notes/implemented/architecture/2026-08-14-skill-catalog-world-state-section.md)。

采样时的 warning 只写进日志（`tracing::warn!`，消息为 `skill catalog warning`），不进入模型上下文。`list_skills` 用同一个渲染函数重新算出相同的 warning。理由见 [Agent Note：warning 由列表重新计算](../../.agents/notes/implemented/architecture/2026-08-07-skill-warnings-recomputed-by-listing.md)。

### 3.3 模型按需加载

模型用 `read` 读 `SKILL.md` 与 `references/`，用 `bash` 运行 `scripts/`。Skill 没有新的读文件或执行方式。

- 这些调用继承 `read` 与 `bash` 的全部限制（[tools.md §6.2](tools.md)、[tools.md §7](tools.md)）。
- 正文作为 Tool Result 进入 Conversation，可以被压缩回收。
- 内置工具里没有 `skill` 工具。
- Core 不记录本 Session 已加载过哪些 skill。
- 模型能读任何已知路径的 skill，包括已停用的 skill。目录不列出不等于访问控制。

理由见 [Agent Note：复用 read](../../.agents/notes/implemented/architecture/2026-08-07-skills-use-read-instead-of-a-skill-tool.md)。

## 4. 显式选择

用户在输入框里用 `$` 选中 skill。输入框显示 `$name`，另外保存精确路径。理由见 [Agent Note：精确路径绑定](../../.agents/notes/implemented/architecture/2026-08-07-explicit-skill-selection-by-exact-path.md)。

### 4.1 候选与绑定

候选框在下面三个条件都成立时打开：

- `$` 在行首或空白之后；
- `$` 到光标之间只有 `[a-z0-9-]`；
- 光标不在已绑定的 token 里。

所以 `$HOME`、`\$name` 与单词中间的 `$` 不打开候选框。

候选来自最近一次成功的 `list_skills`，只含启用的 skill。按名称和描述打分，分数降序，同分按 `name` 字典序：

| 匹配 | 分数 |
|---|---|
| 名称等于查询 | 1000 |
| 名称以查询开头 | 800 − 多出的字符数 |
| 名称包含查询 | 600 − 出现位置 |
| 查询是名称的子序列 | 400 − 间隔总数 |
| 描述包含查询 | 200 − min(出现位置, 199) |
| 都不满足 | 不列出 |

查询为空时，列出全部启用的 skill。

- `ArrowUp` / `ArrowDown` 循环移动选择。
- `Enter` / `Tab` 接受当前候选。
- `Escape` 关闭候选框。
- 输入框只打开一个菜单：有 `$` 目标时，`/compact` 菜单不打开。

接受候选后，输入框把查询替换为 `$name`，并保存一个不显示的绑定：

```ts
type SkillMentionBinding = {
  start: number   // textarea 的 UTF-16 偏移
  end: number     // 半开区间；文本必须恰好是 `$${name}`
  name: string
  path: string    // 候选项的绝对 SKILL.md 路径
}
```

- 编辑完全在绑定之前时，绑定不变；完全在绑定之后时，绑定随之平移。
- 编辑与绑定相交时，删除这个绑定。
- 无法确定编辑区间时，清空所有绑定。
- 外部改写输入框的值（例如发送成功后清空）时，清空所有绑定。
- 文本不再恰好是 `$name` 的绑定一律丢弃。
- 没有绑定的 `$name` 是普通文本。

### 4.2 提交与校验

提交时，Desktop 按起点排序绑定，按 `path` 去重，再追加去掉首尾空白的草稿：

```ts
type UserInput =
  | { type: "skill"; name: string; path: string }
  | { type: "text"; text: string }

runtime_turn_start({ sessionId, clientRequestId, input: UserInput[] })
```

Bridge 原样传递 `input`。它不读文件，不携带正文，不改写 `$name`。

Core 在创建 Turn 前处理输入（`OpenWorkCore::start_turn`）：

1. Text 部分为空时，返回 `invalid_request`。
2. 有 Skill 项时，用同一份 `SkillRoots` 与当前停用集合重新发现。
3. 规范化每个 `path`，并要求它恰好是 `<已配置的根>/<合法目录名>/SKILL.md`。
4. 要求 `(name, path)` 是当前启用的发现结果之一。超出 100 个上限的 skill 不能选。
5. 读取正文。同一 canonical path 只保留第一次。
6. 第 2–5 步任何一步失败，返回 `skill_unavailable`（`Selected skill is unavailable: <name>`）。
7. 每条正文消息的估算 token 必须小于 8000。否则返回 `configuration_invalid`，信息提示拆分 skill。

任何一步失败都不创建 Turn，也不写 Conversation。正文取提交时读到的版本。Core 不扫描 Text 里的 `$name`。

### 4.3 正文进入 Conversation

每个选中的 skill 产生一条 User-role Text Message，按输入顺序写在用户可见消息之前：

```text
<skill>
<name>commit</name>
<path>/Users/me/.agents/skills/commit/SKILL.md</path>
SKILL.md 去掉 frontmatter 后的正文
</skill>
```

- 它的 `message_kind` 是 `skill_instruction`。随后的用户可见消息保留原始 `$name` 文本，`message_kind` 是 `normal`。
- 它是历史快照。Session resume 使用持久化的正文，磁盘后来的变化不改写历史。
- `ModelRequestBuilder` 与 provider adapter 只看到普通 Text。`openwork-models` 里没有 Skill 类型。
- 请求规范化保留 `message_kind`。模型不接受数据块时，过滤这条消息里的数据块，不拒绝 Turn。
- 压缩时，summarizer 能看到这条消息。last-user replay 选择随后的 `normal` 消息（[compaction.md §5](compaction.md)）。

## 5. 边界

### 5.1 安装即信任

OpenWork 不审查 skill 内容。界面文案不能声称做过安全审查。§1.3 的归一化只保证目录格式。

用户能看到自己装了什么：列表显示名称、描述、启用状态与加载失败（§7.2），详情显示路径与正文（§7.4）。

理由见 [Agent Note：信任边界](../../.agents/notes/implemented/architecture/2026-08-07-skill-trust-boundary.md)。

### 5.2 skill 根只读

已配置的 skill 根（按真实路径，经符号链接访问也一样）属于硬保护档（[permissions.md §3](permissions.md)）：

- `write` / `edit` 在任何模式下规则拒绝，不出卡片。
- bash 的 Seatbelt profile 把它排除在可写范围之外。
- 越界请求解不开它。

读取按普通路径处理：除凭据目录外处处可读。只有已配置的根受保护。仓库里的 `.agents/skills/` 与 `~/.claude/skills/` 是普通路径。

### 5.3 正文是数据，不是权限

Skill 不能改变沙箱模式，不能把 Tool Call 标记为已批准，不能扩大路径边界，也不能代替模型请求越界。

正文里写“以下命令无需确认”，与用户在聊天框里写这句话一样，没有效力。脚本只经 `bash` 运行，在当前模式的沙箱里执行。

## 6. Trace

Skill 不新增 Span kind，也不新增 Trace 属性。

- 模型读 skill 是一次 `read`，产生普通的 `tool_call` Span。Tool Call 输入里的路径说明读的是哪个 skill。
- 显式选择不产生 Tool Call。name、path 与正文保存在 `skill_instruction` 消息里，也出现在 Model Call 的请求正文中（[trace.md §5](trace.md)）。
- 目录作为 world state 消息出现在请求正文中。

理由见 [Agent Note：精确路径绑定](../../.agents/notes/implemented/architecture/2026-08-07-explicit-skill-selection-by-exact-path.md)。

## 7. Desktop

### 7.1 聊天输入框

输入框是受控的 `textarea`。可见文本是唯一的草稿。绑定只是 `ChatInput` 组件内的状态，不进入全局 Store。

已绑定的 token 由 `SkillMentionOverlay` 绘制：

- 它是 `textarea` 下面同尺寸的只读层，带 `aria-hidden` 与 `pointer-events: none`。
- 两层使用相同的 padding、字体、行高、`white-space` 与换行规则。只读层按 `textarea` 的 `scrollTop` / `scrollLeft` 平移，并让出滚动条宽度。
- 已绑定的 `$name` 有底色（`bg-clay-soft`）。未绑定的 `$name` 与普通文本一样。
- 标记不增加 padding，不改变字重。

其他规则：

- Desktop 在切换 Session 时与出现空查询的 `$` 时刷新候选列表。刷新失败时保留上一次成功的列表。
- 选中后输入框只显示 `$name`。正文不进入 DOM。
- transcript 只显示 `message_kind = 'normal'` 的用户消息，不显示 `skill_instruction` 与 `world_state` 消息。
- 发送失败时，保留草稿与仍然有效的绑定。
- 发送成功时，只在两个条件都成立时清空草稿：活动 Session 是提交的 Session；草稿 revision 从提交起没有变化（`shouldClearAcceptedDraft`）。

### 7.2 列表

设置侧栏有独立的 Skills 视图（`SkillSettings.tsx`、`SkillList.tsx`）。列表没有编辑器，只能改变是否向模型展示 skill。

- 每行显示名称、描述、启用状态与提示（“会向模型展示”或“不会向模型展示”），以及一个开关。行内不显示路径。
- 标题栏有刷新按钮。开关写入期间，刷新按钮与所有开关都不可用。
- 每条 warning 显示为一张卡片：`~` 缩写的路径（悬停显示完整路径）与 `未加载：<原因>`。
- 列表读取失败时显示 `读取 Skill 目录失败：<原因>`。

`~` 缩写把 `/.agents/skills/` 之前的部分换成 `~`（`displaySkillPath`）。

数据来自一个没有项目参数的 Tauri Command。它使用 Core 持有的 `SkillRoots`：

```ts
list_skills() -> { skills: SkillSummary[], warnings: SkillWarning[] }

type SkillSummary = {
  source: 'agents'
  name: string
  description: string
  path: string          // 绝对路径
  disabled: boolean
}
type SkillWarning = { path: string; reason: string }
```

`list_skills` 失败只影响列表。聊天的候选框保留上一次成功的结果；Turn 路径自己扫描。

### 7.3 启用状态

启用状态是应用级设置，以归一化后的 `name` 为键，存在 PostgreSQL 中：

```sql
CREATE TABLE skill_status (
    name TEXT PRIMARY KEY,
    disabled BOOLEAN NOT NULL DEFAULT FALSE
);
```

- 没有记录与 `disabled = false` 都表示启用。写入用 upsert，保留明确的 `false` 记录。
- 停用只表示不进入 `skills/catalog`，也不能被 `$` 选中。skill 仍在列表中，已知路径仍可读。
- Core 启动时加载停用集合。写入时，Core 在锁内先写数据库，成功后才更新内存。写入失败时返回数据库错误，内存保持原样。
- 每个 Turn 与每次手动压缩在开始时取停用集合的快照。切换只影响之后开始的 Turn 与压缩。

```ts
set_skill_disabled(name: string, disabled: boolean)
  -> { skills: SkillSummary[], warnings: SkillWarning[] }
```

界面文案写“不会向模型展示”，不写“禁止访问”。启停不是路径访问控制，也不改变 skill 根的只读边界。

### 7.4 详情

点击列表行，进入详情视图（`SkillDetail.tsx`）：

- 标题是转换后的名称：按 `-` 分词，每个词首字母大写（`codebase-design` → `Codebase Design`）。
- 标题旁有 `Skill` 徽标。
- 下面是描述、`~` 缩写的路径，以及一张正文卡片。卡片用 `MarkdownRenderer` 渲染去掉 frontmatter 的正文。

正文按需读取：

```ts
read_skill(path: string) -> SkillDetail  // { source, name, description, path, body }
```

`read_skill` 只接受规范化后恰好是 `<已配置的根>/<合法目录名>/SKILL.md` 的路径。它拒绝根外路径、嵌套目录和其他文件，返回 `invalid_request`。前端读不到 agent 读不到的 skill 文件。

三种语言的界面资源键结构一致，测试检查这一点。

只读之外的编辑能力还没有设计，见 [Agent Note：后续能力](../../.agents/notes/proposed/feature/2026-08-07-skill-follow-ups.md)。

## 8. 失败语义

| 失败 | 结果 |
|---|---|
| 某个 skill 解析失败 | 它不加载，记 warning，其他照常 |
| skill 根存在但读不了 | 来源为空，记 warning，Turn 照常开始 |
| skill 根未配置或不存在 | 来源为空，不记 warning，应用正常启动 |
| 目录超出 8000 字符 | 从末尾整行丢弃。采样时写日志，列表显示同样的 warning |
| 模型 `read` 的文件已删除 | `read` 的普通“文件不存在”错误，Turn 继续 |
| 用户编辑了已绑定的 `$name` | 删除该绑定，剩下的 `$name` 是普通文本 |
| 显式选择在提交时已删除、停用、改名或路径不符 | 返回 `skill_unavailable`，不创建 Turn，不写 Conversation，Desktop 保留草稿 |
| 显式选择的正文达到 8000 token | 返回 `configuration_invalid`，不创建 Turn |
| Desktop `list_skills` 失败 | 只影响列表；候选框保留上一次成功的结果 |
| 启停状态写入失败 | 返回数据库错误，内存状态与目录行为不变 |

发现失败不阻止 Turn。显式选择失败必须发生在 Turn 被接受之前。已接受的 Turn 使用持久化的正文快照，之后磁盘上的变化不会让它失败。

## 9. 验收

编号沿用原设计文档的验收编号。46、47 是新增条目。测试路径相对 `crates/`，前端测试写出文件与用例名。

带 Postgres 的测试需要 `TEST_DATABASE_URL`。

**解析与来源**

1. 缺 `name` 时取目录名；缺 `description` 时不加载，并记 warning。
   - 测试：`openwork-core/src/skills/tests.rs::missing_name_uses_the_skill_directory_name`；`openwork-core/src/skills/tests.rs::missing_description_is_a_specific_parse_failure`；`openwork-core/src/skills/tests.rs::invalid_skill_only_adds_a_warning_and_does_not_hide_a_valid_sibling`
2. `description` 里的换行、TAB、连续空格归一化为一个空格，skill 正常加载。
   - 测试：`openwork-core/src/skills/tests.rs::description_whitespace_is_normalized_instead_of_rejected`；`openwork-core/src/skills/tests.rs::description_is_truncated_to_1024_characters`
3. 目录名含换行或不合 §1.3 规则时，跳过该目录，并记 warning。
   - 测试：`openwork-core/src/skills/tests.rs::invalid_directory_names_are_skipped_with_warnings`
4. `SKILL.md` 超过 64 KiB、不是 UTF-8、路径含控制字符时不加载，原因可以区分。
   - 测试：`openwork-core/src/skills/tests.rs::file_size_encoding_and_control_path_failures_have_distinct_warnings`
5. 未知 frontmatter 键不导致失败。
   - 测试：`openwork-core/src/skills/tests.rs::unknown_frontmatter_keys_are_ignored_by_the_two_field_parser`
6. 一个 skill 解析失败，不影响同目录的其他 skill，也不影响 Turn 开始。
   - 测试：`openwork-core/src/skills/tests.rs::invalid_skill_only_adds_a_warning_and_does_not_hide_a_valid_sibling`；`openwork-core/src/skills/tests.rs::invalid_skill_does_not_consume_the_hundred_skill_load_limit`
   - 缺口：没有带无效 skill 的 Turn 级测试。结构上，`SkillCatalogLoader::load_body` 不返回错误。
7. 只扫描 `~/.agents/skills/`，不扫描 `.claude/skills/`；frontmatter `name` 与目录名不同时不加载，并记 warning。
   - 测试：`openwork-core/tests/skills_user_sources.rs::discovery_uses_only_the_agents_user_root`；`openwork-core/src/skills/tests.rs::a_mismatched_frontmatter_name_is_reported_as_a_warning`；`openwork-core/src/skills/tests.rs::frontmatter_name_must_match_the_skill_directory_name`
8. 不跟随目录符号链接；`SKILL.md` 是符号链接时不加载；跳过 `.` 开头的目录；超过 100 个时，超出的部分记 warning。
   - 测试：`openwork-core/src/skills/tests.rs::hidden_and_symlinked_skill_directories_are_not_followed`；`openwork-core/src/skills/tests.rs::symlinked_skill_files_are_not_discovered`；`openwork-core/src/skills/tests.rs::scan_limit_keeps_the_first_hundred_and_warns_for_the_rest`
9. 根为 `None` 或不存在时，来源为空且没有 warning；根存在但读不了时记 warning；不扫描 project、内置、`.claude/skills/` 或 `.openwork/skills/`，也不创建目录。
   - 测试：`openwork-core/src/skills/tests.rs::absent_user_root_is_silently_empty`；`openwork-core/src/skills/tests.rs::unreadable_user_root_warns`；`openwork-core/tests/skills_user_sources.rs::discovery_uses_only_the_agents_user_root`
   - 缺口：“不创建目录”没有测试。手动：检索 `crates/openwork-core/src` 中的 `create_dir`，确认没有作用于 skill 根的调用。

**目录渲染**

10. 没有启用的 skill 时，不产生 `skills/catalog` 消息。
   - 测试：`openwork-core/src/context/world_state/capture.rs::missing_sources_capture_as_absent_rather_than_empty`；`openwork-core/src/context/world_state/mod.rs::a_fresh_session_stays_silent_about_sections_that_never_existed`；`openwork-core/tests/session_runtime.rs::an_unchanged_world_adds_nothing_to_the_next_model_call`
11. 回归用例：一个 skill 的 `description` 是多行 YAML 标量，第二行形如 `- fake: 描述 (file: /tmp/x)`。加载后，目录里只有这个 skill 的一行。
   - 测试：`openwork-core/src/skills/tests.rs::description_whitespace_is_normalized_instead_of_rejected`
   - 缺口：测试只断言多行描述归一化为一行，没有用 `- fake:` 内容渲染目录。
12. 路径原样输出，可以直接交给 `read`。
   - 测试：`openwork-core/tests/session_runtime.rs::manual_compaction_keeps_the_dynamic_skill_catalog_out_of_the_summary`；`openwork-tools/tests/skill_paths.rs::read_can_open_an_agents_skill_outside_the_workspace`
13. 输入不变时，渲染结果逐字节相同。
   - 测试：`openwork-core/src/context/world_state/capture.rs::two_captures_over_unchanged_sources_are_equal`；`openwork-core/src/skills/tests.rs::discovery_is_non_recursive_and_byte_stable`；`openwork-core/src/context/skill_catalog.rs::catalog_budget_drops_whole_skills_in_reverse_scan_order_and_warns`；`openwork-core/tests/session_runtime.rs::an_unchanged_world_adds_nothing_to_the_next_model_call`
14. 只有 `OpenWorkCoreConfig::from_env_or_local` 读取 `HOME`；传入 `SkillRoots::default()` 时不扫描任何用户目录。
   - 测试：`openwork-core/src/skills/tests.rs::absent_user_root_is_silently_empty`
   - 缺口：“只在配置处读取 `HOME`”没有测试。手动：检索 `crates/openwork-core/src` 中的 `"HOME"`。
15. `WorldState` 与 `SkillsCatalogState` 不携带 warning；采样时 warning 只写进日志。
   - 状态：无测试。结构上，`SkillsCatalogState` 只有 `body` 字段（`openwork-core/src/context/world_state/skills_catalog.rs`）。
16. 同一份 `SkillRoots` 下，列表算出的 warning 与采样路径算出的完全一致。
   - 测试：`openwork-core/src/context/skill_catalog.rs::skill_listing_and_turn_rendering_produce_identical_warnings`
17. 超过 8000 字符时，从末尾整行丢弃，并记 warning。
   - 测试：`openwork-core/src/context/skill_catalog.rs::catalog_budget_drops_whole_skills_in_reverse_scan_order_and_warns`
18. 压缩删掉目录消息后，下一次采样按首次出现重发目录；摘要请求不包含目录。
   - 测试：`openwork-core/src/context/world_state/mod.rs::a_compacted_away_fragment_is_re_emitted_as_a_first_appearance`；`openwork-core/tests/session_runtime.rs::a_request_rebuilt_after_compaction_still_carries_world_state`；`openwork-core/tests/session_runtime.rs::manual_compaction_keeps_the_dynamic_skill_catalog_out_of_the_summary`
   - 缺口：重发测试用的是 `agents_md` section 与项目上下文。四个 section 共用同一套状态机，但没有用 skill 目录断言重发。

**显式选择**

19. 行首或空白后的 `$` 打开候选；`$HOME`、`\$name` 与单词中间的 `$` 不打开；候选只含启用的 skill，匹配与排序确定。
   - 测试：`desktop/src/features/chat/skillMentions.test.ts › "opens only for a lowercase token at a line or whitespace boundary"`；`desktop/src/features/chat/skillMentions.test.ts › "ranks enabled candidates deterministically"`；`desktop/src/features/chat/components/ChatInput.test.tsx › "shows enabled skill candidates for a dollar trigger"`
20. 键盘可以移动、接受和关闭候选；`/compact` 菜单与 `$` 菜单不同时打开。
   - 测试：`desktop/src/features/chat/components/ChatInput.test.tsx › "shows enabled skill candidates for a dollar trigger"`
   - 缺口：测试只断言 `$` 打开时没有 `/compact` 菜单。键盘操作没有测试。手动：在输入框输入 `$`，用方向键、`Enter`、`Tab`、`Escape` 操作。
21. 接受候选只插入可见的 `$name`，并保存 `{ start, end, name, path }`；与绑定相交的编辑删除绑定，无法确定区间的编辑清空绑定；只读层与 `textarea` 的光标、换行和滚动对齐，只有已绑定的 token 有底色。
   - 测试：`desktop/src/features/chat/skillMentions.test.ts › "selects a visible token while preserving an exact hidden path"`；`desktop/src/features/chat/skillMentions.test.ts › "moves bindings around external edits and removes an edited token"`；`desktop/src/features/chat/skillMentions.test.ts › "derives insert, deletion, paste, and IME ranges from value and caret state"`；`desktop/src/features/chat/skillMentions.test.ts › "preserves an earlier binding across successive IME composition replacements"`；`desktop/src/features/chat/components/ChatInput.test.tsx › "renders a selected skill as an inline overlay token without duplicating accessible text"`
   - 缺口：光标、换行与滚动对齐没有自动测试。手动：输入多行长文本并滚动，确认底色与文字重合。
22. 手写或失去绑定的 `$name` 作为普通文本提交，不激活 skill。
   - 测试：`desktop/src/features/chat/skillMentions.test.ts › "does not transfer a binding to an identical handwritten token"`
   - 缺口：Core 不扫描 Text 没有测试。结构上，`OpenWorkCore::start_turn` 只解析 `UserInput::Skill`。
23. Bridge 只提交有序的 `UserInput[]`，不读文件，不携带正文，不在前端拼提示词。
   - 测试：`desktop/src/bridge/commands.test.ts › "submits ordered structured user input"`；`openwork-core/src/user_input.rs::structured_inputs_round_trip_without_model_content_blocks`
   - 缺口：“不读文件”靠结构保证：`runtime_turn_start` 原样转交 `input`。
24. Core 在创建 Turn 前按当前根、启用状态、canonical path 与 name 重新校验；失败返回 `skill_unavailable`，数据库与 Conversation 都没有新 Turn，Desktop 保留草稿。
   - 测试：`openwork-core/src/skills/tests.rs::selected_skills_reject_disabled_or_mismatched_names`；`openwork-core/src/skills/tests.rs::selected_skills_reject_valid_files_outside_the_current_discovery_limit`；`openwork-core/src/skills/tests.rs::read_skill_rejects_paths_outside_the_configured_roots`；`desktop/src-tauri/src/error.rs::unavailable_skill_has_a_specific_host_error_code`
   - 缺口：没有经 `OpenWorkCore::start_turn` 断言“没有新 Turn”与“保留草稿”的测试。
25. 成功时按文本位置排序、按 canonical path 去重，正文快照持久化为用户可见消息之前的 contextual User-role Text Message。
   - 测试：`openwork-core/src/skills/tests.rs::selected_skills_resolve_exact_paths_and_deduplicate_body_snapshots`；`desktop/src/features/chat/skillMentions.test.ts › "deduplicates submitted paths and exposes bound segments for rendering"`；`openwork-core/src/session/input.rs::turn_input_labels_skill_context_separately_from_the_user_request`；`openwork-core/tests/postgres_session_storage.rs::postgres_persists_contextual_input_before_the_visible_user_message`
26. `ModelRequestBuilder` 直接组装普通 Text；provider adapter 不接收 Skill 类型，也不读 Skill 文件。
   - 测试：`openwork-core/src/context/normalize.rs::contextual_user_items_are_filtered_rather_than_rejected`
   - 缺口：测试只断言 contextual 消息作为普通 Text 通过规范化。“没有 Skill 类型”靠检索确认：`crates/openwork-models` 中没有 `skill`。
27. Session resume 使用持久化的快照；磁盘变化不改写历史，也不让已接受的 Turn 失败。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_persists_contextual_input_before_the_visible_user_message`
   - 缺口：测试只断言快照被持久化。没有“改磁盘后 resume”的测试。
28. Desktop transcript 不显示 `skill_instruction` 消息；发送失败保留草稿与绑定；发送成功只在 Session 与草稿 revision 都不变时清空；接受期间的新输入不丢失。
   - 测试：`desktop/src/features/chat/transcript.test.ts › "hides persisted Skill instructions while keeping the visible user input"`；`desktop/src/features/chat/transcript.test.ts › "drops skill instructions the same way the live transcript does"`；`desktop/src/features/chat/ChatPage.test.ts › "clears only an unchanged draft in the session that accepted it"`
   - 缺口：发送失败保留绑定没有测试。
29. 压缩 summarizer 能看到 Skill 正文；last-user replay 选择随后持久化的用户可见消息。
   - 测试：`openwork-core/src/session/compaction/compacted_view.rs::a_skill_instruction_is_not_the_user_request_either`
   - 缺口：summarizer 输入包含正文没有测试。结构上，`generate_summary` 只移除 `world_state` 消息（`openwork-core/src/session/compaction/summary.rs`）。
30. `SkillSummary`、候选接口与 Desktop 界面里没有覆盖关系字段或覆盖顺序。
   - 状态：无测试。`SkillSummary` 只有 `source`、`name`、`description`、`path`、`disabled`（`openwork-core/src/skills/discovery.rs`）。

**边界**

31. Tool Surface 里没有名为 `skill` 的工具定义。
   - 测试：`openwork-tools/src/builtins/mod.rs::builtin_registry_exposes_each_tool_once_in_selected_order`
   - 缺口：测试只固定 7 个内置工具。没有测试断言整个 Tool Surface 里没有 `skill`。
32. `write` / `edit` 在任何模式下都拒绝写 skill 根，经别名也一样；bash 也不能写它。
   - 测试：`openwork-tools/tests/skill_paths.rs::write_and_edit_are_denied_for_the_agents_skill_root_in_every_mode`；`openwork-tools/tests/skill_paths.rs::an_alias_cannot_bypass_canonical_skill_root_write_protection`；`openwork-core/src/session_tools.rs::only_the_configured_skill_root_is_protected_even_through_a_symlink`；`openwork-sandbox/tests/matrix.rs::protected_locations_stay_closed_and_temp_stays_open`；`openwork-sandbox/src/policy.rs::grants_unlock_what_they_name_but_never_hard_protected_paths`
33. `read` / `grep` / `glob` / `list` 能读 skill 根下的资源；仓库里的 `.agents/skills/` 与 `~/.claude/skills/` 是普通路径，不受硬保护。
   - 测试：`openwork-tools/tests/skill_paths.rs::all_read_only_file_tools_can_use_the_agents_root`；`openwork-tools/tests/skill_paths.rs::read_can_open_an_agents_skill_outside_the_workspace`；`openwork-core/src/session_tools.rs::only_the_configured_skill_root_is_protected_even_through_a_symlink`
   - 缺口：`~/.claude/skills/` 没有单独测试。
34. Skill 正文里的“免审批”表述不改变实际审批行为。
   - 状态：无测试。结构上，审批只看模式、越界请求与危险命令检测（[permissions.md §1](permissions.md)），不读消息正文。
35. 文案不声称对 skill 内容做过安全审查。
   - 状态：手动。在 `desktop/src/i18n/locales` 中检索“安全”“审查”“safe”“review”“verified”，确认 skill 相关文案没有这类表述。

**Trace 与 Desktop 列表**

36. 模型读 skill 产生工具名为 `read` 的 `tool_call` Span；显式选择不产生 Tool Call；两者都不新增 Span kind 或 Trace 属性。
   - 状态：无测试。`read` 的 Span 由通用 Tool Call 路径产生；显式选择的路径（`OpenWorkCore::start_turn`）不调用工具。
37. 显式选择的 name、path 与正文能从持久化的 User Message 还原；也能从 Model Call 的请求正文读出。
   - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_persists_contextual_input_before_the_visible_user_message`
   - 缺口：请求正文没有测试。
38. 列表显示每个 skill 的名称、描述与启用状态，不显示路径；详情显示 `~` 缩写的路径；未加载的 skill 说出具体原因。
   - 测试：`desktop/src/features/settings/components/SkillList.test.tsx › "shows path-free skill rows and keeps concrete load failures"`；`desktop/src/features/settings/components/SkillDetail.test.tsx › "renders the title, badge, description, path, and markdown body"`
39. Desktop `list_skills` 失败不影响聊天的 Turn 路径；三种语言的资源结构一致。
   - 测试：`desktop/src/i18n/i18n.test.ts › "keeps all locale resources on the same key structure"`
   - 缺口：列表失败的隔离没有测试。结构上，Turn 路径在 Core 内扫描，不经过 `list_skills`。

**启用状态**

40. `skill_status` 只有 `name` 与 `disabled` 两列；`name` 是主键；`disabled` 非空，默认 `false`。
   - 状态：无测试。表结构在 `openwork-core/migrations/202608040001_create_skill_status.sql`；`openwork-core/tests/postgres_session_storage.rs` 只断言表存在。
41. 状态按全局 `name` 持久化；Core 重启后仍读到同一状态。
   - 测试：`openwork-core/tests/postgres_skill_status.rs::skill_status_persists_disabled_and_enabled_values_by_name`；`openwork-core/src/core.rs::core_skill_status_updates_listing_and_survives_restart`
42. `list_skills` 同时返回启用与停用的 skill，用 `disabled` 区分。
   - 测试：`openwork-core/src/core.rs::core_skill_status_updates_listing_and_survives_restart`；`openwork-core/src/context/skill_catalog.rs::catalog_omits_disabled_skills_without_removing_them_from_discovery`
43. 停用的 skill 不进入之后 Turn 或压缩采样的 `skills/catalog`，重新启用后恢复；运行中的 Turn 不受影响。
   - 测试：`openwork-core/src/context/world_state/capture.rs::disabled_skills_do_not_reach_the_captured_catalog`；`openwork-core/tests/session_runtime.rs::session_handle_applies_disabled_skills_to_turn_and_manual_compaction`；`openwork-core/src/context/world_state/skills_catalog.rs::disabling_every_skill_announces_that_the_catalog_no_longer_applies`
   - 缺口：“重新启用后恢复”与“运行中的 Turn 不受影响”没有测试。结构上，停用集合在 `start_turn` 时取快照。
44. 停用的 skill 的已知路径仍可读；启停不改变工具权限。
   - 测试：`openwork-tools/tests/skill_paths.rs::read_can_open_an_agents_skill_outside_the_workspace`
   - 缺口：测试没有停用状态。结构上，工具与沙箱不读 `skill_status`。
45. Desktop 开关只传 `{ name, disabled }`；写入失败保留原状态；三种语言的文案表达“不向模型展示”，不声称“禁止访问”。
   - 测试：`desktop/src/bridge/commands.test.ts › "maps skill status changes by name and returns the refreshed listing"`；`desktop/src/features/settings/components/SkillList.test.tsx › "shows path-free skill rows and keeps concrete load failures"`
   - 缺口：写入失败没有测试；测试只检查中文文案。
46. 显式选择的正文消息达到 8000 token 时，Core 拒绝提交，信息写出 skill 名称并提示拆分。
   - 测试：`openwork-core/src/context/item_limits.rs::an_oversized_skill_names_itself_and_suggests_splitting`
   - 缺口：测试只覆盖上限函数，没有经 `start_turn` 的测试。
47. 目录变化时，追加带取代声明的全量正文；全部停用后，追加失效声明；没有变化时不追加。
   - 测试：`openwork-core/src/context/world_state/skills_catalog.rs::a_changed_catalog_is_resent_whole`；`openwork-core/src/context/world_state/skills_catalog.rs::disabling_every_skill_announces_that_the_catalog_no_longer_applies`；`openwork-core/src/context/world_state/mod.rs::an_unchanged_world_emits_nothing`；`openwork-core/src/context/world_state/mod.rs::a_restarted_session_re_emits_with_a_replacement_notice`
