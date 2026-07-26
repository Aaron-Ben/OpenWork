# 数据库规范

适用于 `crates/openwork-core/migrations/` 下的 schema、`openwork-core` 的 SQLx 读写层，以及经 Tauri Command 暴露给 Desktop 的 DTO。

## 1. 时间字段规范

### 1.1 统一使用 `TIMESTAMP WITHOUT TIME ZONE`，存东八区本地时间

所有时间字段**必须**使用 `TIMESTAMP WITHOUT TIME ZONE`，存储**无时区的东八区本地时间**。禁止 `TEXT`，禁止 `BIGINT` 存 epoch。

```sql
-- ✅ 正确
created_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL
             DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
updated_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL
             DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),
started_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL,
ended_at     TIMESTAMP WITHOUT TIME ZONE,

-- ❌ 错误：用文本存时间
created_at   TEXT NOT NULL,

-- ❌ 错误：默认值落在 UTC
created_at   TIMESTAMP WITHOUT TIME ZONE NOT NULL
             DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'UTC'),
```

`CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'` 的含义是"把当前瞬间渲染成上海墙上时间，再去掉时区标记"，结果正是我们要存的值。

**库里所有时间都是东八区墙上时间，没有例外。** 任何一处写入 UTC 都会造成静默的 8 小时偏差 —— 数据库不会报错，因为它不知道这一列该是什么时区。这个约定只能靠 1.4 的统一入口和 3 的检查清单来守。

### 1.2 命名：瞬间用 `*_at`，时长用 `*_ms`

| 语义 | 后缀 | 类型 |
|---|---|---|
| 时间点 | `_at` | `TIMESTAMP WITHOUT TIME ZONE` |
| 时长 | `_ms` | `BIGINT` / `INTEGER` |

```sql
-- ✅ 正确
started_at          TIMESTAMP WITHOUT TIME ZONE NOT NULL,
ended_at            TIMESTAMP WITHOUT TIME ZONE,
permission_wait_ms  BIGINT,

-- ❌ 错误：用时间类型表达时长
permission_wait     INTERVAL,
-- ❌ 错误：时长字段用 _at 后缀
permission_wait_at  BIGINT,
```

时长由两个瞬间相减得出，**以整数毫秒落库**，不用 `INTERVAL`。理由：时长要参与算术和聚合，`INTERVAL` 在跨语言序列化时没有统一表示。

### 1.3 成对的时间字段必须有状态联动约束

```sql
-- ✅ 正确：终态必有 ended_at，运行中必无
CONSTRAINT turns_terminal_time_valid CHECK (
    (status = 'running' AND ended_at IS NULL) OR
    (status <> 'running' AND ended_at IS NOT NULL)
),
CONSTRAINT turns_end_after_start CHECK (ended_at IS NULL OR ended_at >= started_at)
```

任何 `started_at` / `ended_at` 组合都要写这两条。让数据库拒绝"已完成但没有结束时间"这类不可能状态，而不是靠代码自觉。

### 1.4 Rust 侧：`PrimitiveDateTime` + 唯一取时入口

所有取当前时间的地方，**必须**走同一个 helper，不允许各自 `OffsetDateTime::now_utc()` 之后自行处理：

```rust
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

/// 库中所有时间字段的统一时区：东八区。
pub const CHINA_OFFSET: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(offset) => offset,
    Err(_) => panic!("+08:00 is a valid offset"),
};

/// 当前的东八区墙上时间。落库的时间值只能来自这里。
pub fn china_now() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc().to_offset(CHINA_OFFSET);
    PrimitiveDateTime::new(now.date(), now.time())
}
```

```rust
// ✅ 正确
pub struct TurnRecord {
    pub started_at: PrimitiveDateTime,
    pub ended_at: Option<PrimitiveDateTime>,
}
.bind(china_now())

// ❌ 错误：直接落 UTC，差 8 小时且不会报错
.bind(OffsetDateTime::now_utc())

// ❌ 错误：用字符串在层间传时间
pub struct TurnRecord {
    pub started_at: String,
}

// ❌ 错误：在调用点自己算偏移，绕过统一入口
let now = OffsetDateTime::now_utc() + Duration::hours(8);
```

外部传入的时间（例如 Provider 响应里的时间戳）是 UTC 瞬间，落库前必须显式转换：

```rust
// ✅ 正确
let local = instant.to_offset(CHINA_OFFSET);
PrimitiveDateTime::new(local.date(), local.time())
```

### 1.5 序列化：出库时必须带 `+08:00`，绝不能带 `Z`

**这是本规范最容易出错的一条。** 库里存的是东八区墙上时间，如果序列化时标成 `Z`（UTC），前端会把 18:30 当成 UTC 再转成北京时间，显示 26:30 —— 一次 8 小时的错误标注，加上前端一次 8 小时的换算，最终偏 16 小时。

```rust
use time::format_description::well_known::Rfc3339;

/// 把库里的东八区墙上时间序列化成带偏移量的 RFC 3339 字符串。
pub fn to_wire(value: PrimitiveDateTime) -> Option<String> {
    value.assume_offset(CHINA_OFFSET).format(&Rfc3339).ok()
}
// → "2026-07-25T18:30:00.000000+08:00"
```

对应的 SQL 写法（首选让 SQLx 解码后在 Rust 里格式化，确需在 SQL 出字符串时）：

```sql
-- ✅ 正确：偏移量与存储约定一致
to_char(started_at, 'YYYY-MM-DD"T"HH24:MI:SS.US"+08:00"')

-- ❌ 错误：谎称是 UTC，前端会再加 8 小时
to_char(started_at, 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
```

分层职责：

```
PostgreSQL  TIMESTAMP WITHOUT TIME ZONE   ← 东八区墙上时间
Rust        PrimitiveDateTime             ← 东八区墙上时间
DTO / IPC   RFC 3339 带 +08:00            ← 一个明确的瞬间
前端        Date.parse                    ← 瞬间
显示        Asia/Shanghai                 ← 渲染
```

**前端禁止对时间字符串做切片、拼接或正则**。只有两种合法操作：`Date.parse()` 拿到瞬间，以及交给 `formatBeijingDateTime()` 显示。

```typescript
// ✅ 正确
formatBeijingDateTime(span.startedAt)
const durationMs = Date.parse(span.endedAt) - Date.parse(span.startedAt)

// ❌ 错误：字符串手术
const date = span.startedAt.slice(0, 10)
const time = span.startedAt.split('T')[1].replace('+08:00', '')
```

新增时间显示一律走 `apps/desktop/src/lib/dateTime.ts`，不要在组件里各自 `new Intl.DateTimeFormat`。

## 2. 迁移规范

### 2.1 类型转换必须写 `USING`

PostgreSQL 不会自动转换不兼容的列类型：

```sql
-- ❌ 错误：直接 ALTER TYPE 会失败
ALTER TABLE turns ALTER COLUMN started_at TYPE TIMESTAMP WITHOUT TIME ZONE;

-- ✅ 正确：显式说明如何解释旧值
ALTER TABLE turns
    ALTER COLUMN started_at TYPE TIMESTAMP WITHOUT TIME ZONE
        USING (started_at AT TIME ZONE 'Asia/Shanghai');
```

`timestamptz AT TIME ZONE 'Asia/Shanghai'` 把瞬间渲染成上海墙上时间并去掉时区标记，正是本规范要的形态。

### 2.2 时区口径变更要显式平移，并说明依据

如果列类型已经是 naive，只是口径要从 UTC 改成东八区，那不是类型转换而是**值平移**：

```sql
-- ✅ 正确：注明旧值口径，否则无从判断该不该加这 8 小时
-- 旧值为 naive UTC（见 202607180003），统一为东八区墙上时间
UPDATE turns SET started_at = started_at + INTERVAL '8 hours',
                 ended_at   = ended_at   + INTERVAL '8 hours';
```

这类迁移**必须在文件顶部注释写清旧值是什么口径**。naive 列上看不出时区，一旦判断错就是静默的 8 小时偏差，且不可逆推。

### 2.3 加非空列走三步

```sql
-- ✅ 正确
ALTER TABLE trace_spans ADD COLUMN trace_id TEXT;              -- 1. 可空加列
UPDATE trace_spans SET trace_id = COALESCE(turn_id, id);       -- 2. 回填
ALTER TABLE trace_spans ALTER COLUMN trace_id SET NOT NULL;    -- 3. 收紧
```

一步到位的 `ADD COLUMN ... NOT NULL DEFAULT ...` 只在默认值对所有历史行都语义正确时才可以用。

### 2.4 其他

- 迁移文件是 schema 的唯一事实来源，位置 `crates/openwork-core/migrations/`。
- **已应用的迁移不可修改**，只能新增：`sqlx migrate add --source crates/openwork-core/migrations <description>`。
- 每个迁移必须能在**空库**上从头跑通，不能依赖某次手工修复过的状态。
- 破坏性操作（`DROP COLUMN` / `DROP CONSTRAINT`）要在迁移文件顶部用注释写明理由。

## 3. 违规模式检测

发现以下情况应立即指出并给出修复建议：

**时间类型与口径**
- 时间字段使用 `TEXT` / `BIGINT` / `TIMESTAMPTZ`
- 列默认值写成 `AT TIME ZONE 'UTC'`
- Rust 侧用 `String` 承载时间字段
- 直接 `.bind(OffsetDateTime::now_utc())` 落库，或在调用点自己 `+ 8 hours` 绕过 `china_now()`
- 外部 UTC 时间戳未经 `to_offset(CHINA_OFFSET)` 直接落库
- 时长字段用 `INTERVAL` 或 `*_at` 命名

**序列化**
- 出库字符串带 `Z` 后缀（会造成 16 小时偏差）
- 前端对时间字符串做 `slice` / `split` / `replace` / 正则
- 组件内直接 `new Intl.DateTimeFormat` 而不走 `lib/dateTime.ts`

**约束**
- 有 `started_at` / `ended_at` 却没有终态联动 CHECK 和 `ended_at >= started_at`

**迁移**
- 类型转换没写 `USING`
- 时区口径平移没有注明旧值口径
- 加 `NOT NULL` 列时没有回填步骤
- 修改了已经应用过的迁移文件

---

## 附：收敛状态

时间口径的代码改造**已完成**：

- 列默认值统一为 `AT TIME ZONE 'Asia/Shanghai'`；
- `storage/time.rs` 提供唯一入口 `CHINA_OFFSET` / `china_now()` / `to_china()`，旧的 `utc_naive()` 已删除；
- `storage/postgres.rs` 的出库字符串全部是 `+08:00`，`Z` 已归零。

**回归检查**（新增写入路径或查询时跑一遍）：

```bash
# 两条都应该输出 0
grep -c 'utc_naive' crates/openwork-core/src/storage/*.rs
grep -c '\\"Z\\"' crates/openwork-core/src/storage/postgres.rs
```

第二条是最重要的一道防线：标错时区不会报错，只会让界面上的时间整体偏 16 小时，必须由 grep 或测试守住，不能靠肉眼。
