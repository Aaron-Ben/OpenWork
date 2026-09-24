//! 卡片 Turn（collaboration.md §11.4）：收件箱带回待处理的卡片唤醒时不经过 triage，直接打开 Run
//! 跑主模型；同批的未读消息随这个 Run 一起交付。结算由 Server 在 Run 成功后完成。

use std::time::Instant;

use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use super::{AgentRunner, RunHeartbeat, RunnerError};
use crate::computer::prompt::{CardTurn, card_turn_prompt};
use crate::protocol::{InboxResponse, OpenRunRequest};

impl AgentRunner {
    /// `inbox` 的 trigger 为 `card` 时调用；没有 trigger 时什么也不做。
    pub(super) async fn run_card_turn(
        &mut self,
        inbox: InboxResponse,
        cancellation: CancellationToken,
    ) -> Result<(), RunnerError> {
        let Some(trigger) = inbox.trigger else {
            return Ok(());
        };
        let prompt = card_turn_prompt(&CardTurn {
            self_id: &self.assignment.id,
            now: OffsetDateTime::now_utc(),
            cards: &inbox.cards,
            more_cards: inbox.more_cards,
            messages: &inbox.messages,
            rooms: &inbox.rooms,
            team: &inbox.team,
            carried_over: inbox.carried_over,
        });
        self.quiet_since = Instant::now();
        let run = self.client.open_run(&OpenRunRequest { trigger }).await?;
        let _run_heartbeat = RunHeartbeat::start(self.client.clone(), run.id.clone());
        self.execute_main_run(run.id, prompt, cancellation).await?;
        self.quiet_since = Instant::now();
        Ok(())
    }
}
