# 进度：补齐 Cumora 协作核心

配合 [collab-core.md](collab-core.md) 使用。每完成一步就更新；计划删除时一并删除。

分支：`permission`。

## 1. 工作包状态

| 工作包 | 状态 | 说明 |
|---|---|---|
| 文档 | 完成 | 4 份协作文档合并为 collaboration.md（运行时 + 存储）与 collaboration-desktop.md；K1–K7 写入 collaboration.md；collaboration-desktop.md 按设计稿 v2 重写 |
| 设计稿 | v2 已确认 | https://claude.ai/artifact/MDGsQTdy7KuRLYPuvFGHeR（去掉运行记录页、“最近的协调”、Agent 运行统计与房间里的卡片事件行） |
| U1、U2 | 未开始 | |
| K1 | 完成 | `computer/prompt.rs` 渲染增量（从 `runner.rs` 移出，runner.rs 879→795 行）；inbox 增加 `rooms` 与 `team`；`AGENTS.md` 契约增加 Addressing 一节；fake OpenCode 改为从 `# room-…` 标题行取房间 |
| K5 | 完成（后端、CLI、Agent 提示与 Desktop bridge；Desktop 界面在 U2） | 迁移 `202609240002_message_quotes.sql`（`(room_id, id)` 唯一约束 + 同房间复合外键）；`MessageView.quoted`；`reply --quote`；Desktop `collab_message_send` 增加 `quotedMessageId`；引用穿透 mute（inbox 与唤醒）；inbox/glance/messages 与增量显示消息 id 和引用行；`AGENTS.md` 补 `--quote`；`Messages::views` 统一补齐引用摘要，`insert` 改用 `NewMessage` 结构（原 6 个参数） |
| K2 | 完成 | `server/routing.rs`（点名对象、`@all`、路由题）；triage payload 增加 `routing` 与 `routed` 参数，人类消息那一步拆成 `human_step`；`collab_triages.source` 增加 `routing`（迁移 `202609240003`），最终结论写入 `response_mode`；Computer 端 `runner.rs` 改为 `runner/mod.rs` + `runner/routing.rs`，`parse_route` 只认明确的 `me` |
| K3 | 完成 | lap floor（`n > k`，本批每个房间都越过时以 `lap_floor` 跳过），人类关注 = 人类消息或 `collab_rooms.user_viewed_seq`（迁移 `202609240004`）；判断顺序为硬上限 → 私聊检查点 → lap floor；写入时的 20 条硬上限也按最近一次人类关注计数；Desktop `RoomViewed` 命令（只增不减、不进幂等账本）、Tauri `collab_room_viewed`、`messageStore` 在前台看到新消息时上报 |
| K4 | 完成（含 E17） | `Messages::duplicate_of_last_peer_in`：锁住房间行后与最近一条别人发的 normal 消息比较（去首尾空白），`reply`（含带 HELD token 的重试）与 `dm` 在写入前调用，拒绝码 `DUPLICATE`，文本附对方原话前 200 字 |
| K8 | 完成 | 对齐 Cumora 的差距，见 E18–E22。迁移 `202609240005`（`collab_messages.run_id`、triage source `fail_closed`）；`reply` 的检查拆到 `server/agent_commands/reply.rs`（原 `agent_commands.rs` 708 行、`reply` 约 160 行）；shim 拆为 `shim/{mod,parse,render}.rs`（原 966 行）；Runner 的 triage 模型一步拆到 `runner/classify.rs`（`runner/mod.rs` 799→770 行）；集成测试夹具拆到 `tests/support/room_fixture.rs`，发布检查的测试移到 `tests/posting.rs`（`messaging.rs` 1524→1016 行） |
| K6–K7 | 未开始 | |

## 2. 已定决策

| # | 结论 |
|---|---|
| E1 | 本轮只补 Cumora 协作核心（K1–K7）；Memory、steer、Calendar 等第 1 类能力不做（2026-09-24） |
| E2 | 协作文档合并为 2 份：collaboration.md（运行时、存储、验收）与 collaboration-desktop.md（界面）；删除 collaboration-data-model.md 与 collaboration-architecture-hardening.md（2026-09-24） |
| E3 | K1 增量照搬 Cumora `chatDelta`：40 行、600 字、名册为全部 active Agent + `local-user`、时间用 `+08:00`、不重复 persona（2026-09-24） |
| E4 | Server 不调用模型的规则不改；K2 由每个未被点名的 Agent 用自己的 triage 模型判断 `me`/`each`，失败时参与；将来需要全局唯一决策时由 Server 把任务派给 Computer（2026-09-24） |
| E5 | K3 照搬 Cumora lap floor（`n > k`），人类关注包括用户在 Desktop 看到房间；不采用认领档与全租户在场档；20 条硬上限保留（2026-09-24） |
| E6 | K4 逐字重复：比较紧挨着的一条别人发的 normal 消息，HELD token 不能绕过，拒绝码 `DUPLICATE`（2026-09-24）。范围由 E19 改为只在成员超过 2 人的房间拦 |
| E7 | K5 引用：同房间复合外键，`dm` 不支持，Desktop 可引用，穿透 mute，作为 K2 点名对象（2026-09-24） |
| E8 | K6 Column `kind`（todo/doing/done/空）直接替换 `is_terminal`；领取只从 todo 推进到最左 doing；超时接手 = 20 分钟未更新且负责人没有 running Run，负责人归档时立即可接手（2026-09-24） |
| E9 | K7 卡片唤醒持久化到 `collab_card_wakes`，同 Agent 同卡片合并，成功才结算，Agent 触发的计入每分钟 30 次限额（2026-09-24） |
| E10 | 界面沿用工作台令牌、重做信息结构，按设计稿 v2 实现（2026-09-24） |
| E12 | 房间与看板之间按 Cumora 方式用卡片链接连接：消息里的 `card-…` id 渲染成胶囊，点击在房间右侧栏预览卡片；`AGENTS.md` 契约补“谈到卡片时写出 id”；不做看板事件自动写入房间（2026-09-24） |
| E13 | `reply` / `dm` 正文直接跟在 id 后面（多个参数按空格拼接），`--stdin`、`--file`、`--` 仍可用；起因是 K1 实测中模型第一次总写成位置参数、每次多一跳（2026-09-24） |
| E14 | CLI 三处修正：`--` 之前任何位置的 `--help` / `-h` 显示帮助，子命令后只显示该子命令的用法；glance 没有新消息时写明 `No new messages since you last read this room (latest sequence N).`；`AGENTS.md` 写明发消息的写法与 `openwork <command> --help`。起因是 E13 实测中 Bo 猜 `--body`、子命令 `--help` 只报错、Ada 被 `(no messages)` 误导（2026-09-24） |
| E15 | 派生的 OpenCode 配置把 Agent 的主模型与判断模型标为 `status: active`，不再因模型目录刷新后标为 deprecated 而失效；Desktop 默认模型改为 `deepseek/deepseek-flash`。缓存目录仍是每次会话的临时目录，持久化另议（2026-09-24） |
| E16 | lap floor 只在 triage 时判断，不在 `reply` / `dm` 写入时再判断：与 Cumora 一致，多个 Agent 同时基于 n = k 的状态被唤醒时，最多放过一轮并发接话（2026-09-24） |
| E17 | HELD 按 Cumora 的方式重写：文案写明消息没有发出，直接重发改过的内容即可通过（HELD 时已推进 seen），`--held-token` 只用于原稿照发；`reply` 的 `--quote` / `--held-token` 写在正文前后都可以，`--` 之后一律当正文。`AGENTS.md` 契约的 HELD 一条同步改为直接重发（Cumora `glance-protocol.ts` 的 "recompute your item, and resend"）。起因是 K4 实测中模型把 `--held-token` 写在正文之后被拒（2026-09-24） |
| E18 | 补上 Cumora 的连发闸（新工作包 K8）：成员超过 2 人的房间里，房间最后一条是自己发的且不到 10 分钟时 `reply` 以 `MONOLOGUE` 拒绝；同一 Run 在该房间的第 2 条放行；`--continue` 放行并跳过 HELD，不跳过逐字重复；Agent 发的消息记录 `collab_messages.run_id`。起因是对照 Cumora `cli.ts` 发现这道核心闸在缺口清单里漏掉了（2026-09-24） |
| E19 | 逐字重复只在成员超过 2 人的房间拦，私聊不拦。新事实：Cumora 锁内复查以 `member_count > 2` 为条件，E6 写的“私聊也拦”不来自 Cumora，且会拒掉 Agent 在私聊里回用户同一句“好的”（2026-09-24） |
| E20 | 模型可见的消息列表照 Cumora 设上限：inbox 240 字、messages 280 字、glance 200 字、HELD 最多 8 条每条 200 字、引用 180 字，截断处 `…` 并注明 `messages --json`；`messages --json` 输出完整正文；`messages` 推进 seen；triage 模型输入每类最后 40 条、每条 500 字（2026-09-24） |
| E21 | triage 模型失败照 Cumora：限流与超时退避、delivery 保留；无法解析与其他错误 fail closed（`fail_closed`，结算为 `triage_false`，不退避）。走到模型这一步的批次只含 Agent 消息（2026-09-24） |
| E22 | `AGENTS.md` 的开头一段与五条 glance-and-yield 规则、每轮增量的开头一段照搬 Cumora 原文，只替换命令名、去掉表情回应；Desktop 窗口回到前台时补报 `collab_room_viewed`（2026-09-24） |
| E11 | 协作模式不要运行记录：删除运行记录页与其专用后端（`observability`、`collab_run_events`、事件上报、`collab_run_list`/`collab_run_trace`）；界面只展示当前状态和房间说明行；`collab_runs` 与 `collab_triages` 保留为内部状态（2026-09-24） |

## 3. 写文档时新定的实现细节（2026-09-24 用户已确认）

- 用户查看记录存为 `collab_rooms.user_viewed_seq`（看到的最大 sequence），而不是讨论时说的 `user_viewed_at` 时间戳：按 sequence 判断“哪些 Agent 消息被看过”没有时钟问题，也能直接用来算未读。
- 卡片 Turn 处理的卡片集合由 `collab_card_wakes.run_id` 记录，Run 的 trigger 为 `card`；`focus_card_id` 仍只用于 Agenda。讨论时说的“Run 带上 `focus_card_id`”在一个 Run 处理多张卡片时无法表达。

## 4. 检查记录

- 2026-09-24 K1：`scripts/check.sh` 全部通过（首轮只有 fmt 未过，格式化后整体重跑通过）。此前单独跑 `openwork-collab` 全量时 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败一次（CLAUDE.md 已知不稳定），单独重跑两次通过。验收 §16 #8 → `computer::prompt::tests::acc_08_message_turn_prompt_renders_the_documented_delta`、`acc_08_digest_over_forty_lines_names_what_it_left_out`、`long_bodies_are_cut_at_six_hundred_characters`、`roster_skips_self_and_archived_authors_but_their_messages_keep_names`、`agenda_turn_prompt_carries_time_brief_and_roster`、`computer::home::tests::acc_08_standing_contract_names_the_addressing_rules`、`messaging::acc_08_inbox_carries_room_headers_and_the_active_team`。

- 2026-09-24 文档：`cargo test -p openwork-collab --test architecture` 通过（文档清单已去掉 collaboration-data-model.md）。

- 2026-09-24 K1 真实模型实测（用户授权）：`runtime_e2e::desktop_server_computer_and_real_opencode_smoke` 通过（`deepseek/deepseek-v4-flash`，11.55 秒）。三 Agent 群聊测量（临时测试，跑完已删）：
  - 用 `deepseek-v4-flash` 时第二轮全部失败（`Unexpected server error`）：第一次运行后 OpenCode 刷新的 `models.json` 把 v4-flash 标为 deprecated，即遗留问题 3，与 K1 无关；
  - 改用 `deepseek/deepseek-flash` 并预置模型目录：按角色点名 “Reviewer, …” 只有 Bo 回复且正确；按名字点名 “Ada, …” 只有 Ada 回复且正确；10 个 Run 全部 completed，Agent 消息经 triage 判定后沉默；
  - OpenCode 会话记录中的命令：没有 `participants` / `members`；Bo 仍调了 `openwork reply --help`；Ada 第一次写成 `openwork reply <room> "<body>"` 被拒（`body requires --stdin, --file <path>, or -- <body>`），加 `--` 后重试成功。多出的这一跳来自 reply 的正文语法，不是名册。

- 2026-09-24 E13 实现：shim 新增两个测试（先失败后通过）。`scripts/check.sh` 两次都只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败（CLAUDE.md 已知不稳定），该文件单独连跑 3 次 2 过 1 败；其余全部通过。真实模型复测（`deepseek/deepseek-flash`，预置模型目录，临时测试跑完已删）：点名结果与上次相同（只有 Bo、只有 Ada，回答正确，10 个 Run 全部 completed）；Ada 与 Bo 都用 `openwork reply <room> "<text>"` 发布成功，没有再因正文写法失败。新出现的多余步骤：Bo 先猜了 `--body`，又调 `openwork reply --help`（子命令后的 `--help` 被当作房间 id，只返回错误）；Ada 在 glance 看到 `(no messages)` 后调了 `openwork glance --help; openwork --help` 和 `openwork messages`。两人回复都以 `@local-user` 开头。

- 2026-09-24 CLI 三处修正（子命令 `--help`、glance 空结果文案、`AGENTS.md` 写明发消息方式与 `<command> --help`）：新增 `help_after_a_subcommand_shows_only_that_usage`、`empty_glance_says_nothing_new_since_the_last_read`，扩展 `acc_08_standing_contract_names_the_addressing_rules`，均先失败后通过。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过；其余全部通过。真实模型复测（`deepseek/deepseek-flash`）：点名结果不变（只有 Bo、只有 Ada，回答正确，10 个 Run 全部 completed）；命令只有 Ada 的 `glance` + `reply` 和 Bo 的 `reply`，没有 `--help`、没有失败重试、没有 `participants` / `members`。

- 2026-09-24 提交前审查（/review-branch）两条阻塞已修：`home.rs` 中 acc_08 测试插在 acc_10 的文档注释与 `cfg(macos)` 之间，已移到前面；collaboration.md §7.1 补上发消息写法与 `<command> --help`，并记为 E14。修后 `scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。

- 2026-09-24 E15：`derived_configs_keep_the_chosen_models_active_even_when_the_catalog_deprecates_them` 与更新后的 `main_turn_config_loads_the_managed_agents_file_as_instructions` 先失败后通过。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。真实模型实测（不预置模型目录，OpenCode 1.18.18；`brew upgrade` 后 tap 的稳定版仍是 1.18.18）：`deepseek/deepseek-v4-flash` 两轮都成功（此前第二轮全部失败），运行后缓存里的 `models.json` 确实把它标为 deprecated，派生配置里是 `status: active`；`deepseek/deepseek-flash` 首次运行也成功（自带快照里没有它）。两次点名结果都正确，10 个 Run 全部 completed。

- 2026-09-24 K5：验收 §16 #12 → `messaging::acc_12_quotes_stay_in_the_room_and_reach_a_muted_author`（先因字段不存在编译失败，实现后通过）、`computer::shim::tests::acc_12_reply_takes_a_quote_anywhere_outside_the_body`、`acc_12_message_listings_show_ids_and_quoted_originals`、`computer::prompt::tests::acc_12_quoted_messages_show_the_original_under_the_reply`、`bridge/collab.test.ts` 的 quoted send 用例。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。唤醒路径（`wake_recipients`）的引用例外没有独立测试，由 inbox 路径覆盖投递正确性。
- 2026-09-24 K5 真实模型实测（`deepseek/deepseek-flash`，不预置目录，临时测试跑完已删）：点名结果不变（只有 Bo、只有 Ada，回答正确，10 个 Run 全部 completed）；Bo 与 Ada 都主动用 `openwork reply <room> --quote <被回答的用户消息 id> <text>` 一次发布成功，没有多余命令。

- 2026-09-24 K2：验收 §16 #9 → `server::routing::tests::acc_09_only_messages_naming_other_agents_are_routed`、`mention_boundaries_match_the_mute_exception`、`routing_request_lists_named_and_other_agents`、`computer::triage::tests::acc_09_only_an_explicit_me_narrows_the_route`、`messaging::acc_09_a_message_naming_one_agent_asks_the_others_to_route_it`。纯函数测试与实现同时写成，没有先看到失败；服务端验收测试写完后直接通过。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。
- 2026-09-24 K2 真实模型实测（`deepseek/deepseek-flash`，三 Agent 群，临时测试跑完已删）：`@bo …` 提问 → Ada、Cy 路由题答 `me`，以 `routing` 跳过、不跑主模型，只有 Bo 回复；`@bo suggested … What does everyone think?` → Ada、Cy 答 `each`，三人都回复。之后 Ada、Cy 各自又接了一轮（Agent 消息经 triage 模型判为 actionable），这是 K3 lap floor 要处理的情况。12 个 Run 全部 completed。

- 2026-09-24 K3：验收 §16 #10 → `server::triage::tests::acc_10_a_lapping_agent_run_is_skipped_without_a_model`（先因字段不存在编译失败）、`messaging::acc_10_a_second_lap_is_skipped_until_the_user_looks_again`、`rooms/roomViewed.test.ts`、`bridge/collab.test.ts` 的 `collab_room_viewed` 用例。现有测试 `direct_room_reads_and_private_directional_climate_form_one_loop` 改了预期：两人私聊第 8 条的检查点现在以 `lap_floor` 跳过（8 条只来自 2 个 Agent），Climate 进入 triage 输入的断言移到群聊场景。文档 §8.3 修正：硬上限先于私聊检查点（恢复原规定）。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。
- 2026-09-24 K3 真实模型实测（`deepseek/deepseek-flash`，同 K2 第二个场景）：第一轮三人各答一句后，三人在 n = k = 3 的同一时刻被唤醒，triage 模型都判为 actionable，同时各接一句（第 7–9 条）；之后 3 次唤醒以 `lap_floor` 跳过，讨论停止。缺口来自唤醒时的快照，Cumora 相同，按 E16 接受。

- 2026-09-24 K4：验收 §16 #11 → `messaging::acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected`（先失败：带 HELD token 的重复被发布；实现后通过），覆盖去首尾空白比较、HELD token 不能绕过、被拒时消息与 delivery 不变、两个 Agent 同时私聊同一句只有一条成功。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。
- 2026-09-24 K4 真实模型实测（`deepseek/deepseek-flash`，三 Agent 数 1 到 6）：结果 1–6 各一条、顺序正确，没有重复发布；撞车都先被 HELD 拦下后改发下一个数，本次没有触发 `DUPLICATE`。Ada、Bo 各有一次把 `--held-token` 写在正文之后被拒（`put --held-token before the message body`），重发后成功。
- 2026-09-24 E17：`computer::shim::tests::held_replies_say_a_plain_resend_goes_through`（先失败）、`acc_12_reply_takes_a_quote_anywhere_outside_the_body`（原 `acc_12_reply_takes_a_quote_before_the_body`，改为断言正文后的选项被接受、`--` 之后照原文、重复选项报错，先失败后通过）；HELD 后不带 token 直接重发由 `messaging::acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected` 里 Beta 发 `2` 那一步覆盖。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。
- 2026-09-24 E17 真实模型实测（`deepseek/deepseek-flash`，同 K4 数数场景）：1–6 各一条、顺序正确；3 次 HELD 后模型都直接 `openwork reply <room> <下一个数>` 重发成功，没有用 `--held-token`、没有参数被拒；最后一次 Ada 看到 6 已发出后停手（Run 结果 `unpublished`），之后 4 次唤醒以 `lap_floor` 跳过。
- 2026-09-24 提交前审查（/review-branch）一条阻塞已修：`home.rs` 契约仍写着 HELD 后带 token 重试改稿，与 §9.1 相反；已改为直接重发，`acc_08_standing_contract_names_the_addressing_rules` 增加逐字断言（先失败后通过），collaboration.md §7.1 同步。

- 2026-09-24 K8：验收 §16 #11 → `posting::acc_11_a_verbatim_repeat_of_the_last_peer_message_is_rejected`（改为私聊不拦、`--continue` 也拦）；#20 → `posting::acc_20_an_agent_cannot_post_twice_in_a_row_until_someone_else_speaks`、`computer::shim::parse::tests::acc_20_reply_takes_continue_anywhere_outside_the_body`；#21 → `posting::acc_21_held_lists_eight_messages_and_listing_counts_as_seen`、`computer::shim::render::tests::acc_21_listings_cut_long_bodies_like_cumora`、`acc_21_messages_json_prints_full_bodies`、`computer::shim::parse::tests::acc_21_only_messages_takes_json`、`server::triage::tests::acc_21_triage_input_keeps_the_latest_forty_messages_cut_to_500_chars`；#22 → `computer::runner::classify::tests::acc_22_triage_failures_back_off_or_fail_closed_like_cumora`、`posting::acc_22_a_failed_triage_model_fails_closed_for_agent_only_messages`；§7.1/§7.2 → `computer::home::tests::acc_08_standing_contract_names_the_addressing_rules`、`computer::prompt::tests::acc_08_message_turn_prompt_renders_the_documented_delta`；collaboration-desktop.md §12 #10 → `rooms/messageStore.test.ts`。先看到失败的：契约原文、增量开头、Desktop 前台补报、`messages` 推进 seen（临时去掉修复后 `acc_21` 失败）；其余服务端、shim 渲染与 fail closed 的测试与实现同时写成，没有先看到失败。`scripts/check.sh` 只有 `reported_rate_limit_terminates_a_still_running_opencode_process` 失败，单独重跑通过。
- 2026-09-24 K8 提交前审查（/review-branch）三条阻塞已修：`reply` 超过 60 行，拆出 `check_gates` 与 `quote`；§16 #11 的并发条目改用群聊 `--continue` 并发同一句覆盖（`posting::acc_11_concurrent_identical_group_posts_publish_only_once`）——临时去掉 `reply_context_in` 的 `FOR UPDATE OF room` 后连跑 5 次仍通过，两个请求很难真正交错，这条测试只验证结果、证明不了锁；§9.1 第 3 步改为“seen 推进到列出的最后一条（最多 8 条）”。
- 2026-09-24 K8 真实模型实测（`deepseek/deepseek-flash`）：数 1 到 6 结果正确，HELD 1 次后直接重发，没有触发连发；讨论场景（K2 的两问 + “先发一句在做、再单独发 3 步计划”）中，Ada 在自己的消息是房间最后一条时 20 秒后想再补一句，被 `MONOLOGUE` 拒绝后没有用 `--continue`，讨论停止（K3 实测时三人各多接一轮）；Ada 同一 Run 里的“On it”与计划两条都发出。

## 5. 待定


- 重写 collaboration-desktop.md 时新定的界面细节（2026-09-24 用户已确认）：Agent 之间的房间只读并显示提示；卡片详情的“在房间中讨论”打开与负责人的私聊并预填卡片引用；识别色按 Agent ID 稳定哈希取 6 档；协作界面的小号说明文字用 `ink-soft`；新增 `collab_room_open` 返回房间快照，取代 `collab_message_list`。
