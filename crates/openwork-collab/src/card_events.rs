//! Publishes one committed card mutation to scheduler and Desktop consumers.

use tokio::sync::mpsc;

use crate::{
    event::{CollabEventKind, CollabEventPublisher},
    mcp::MessageNotice,
    model::CardMutation,
};

pub async fn publish(
    notices: &mpsc::UnboundedSender<MessageNotice>,
    events: &CollabEventPublisher,
    mutation: &CardMutation,
) {
    let _ = notices.send(MessageNotice {
        room_id: mutation.message.room_id.clone(),
        author_id: mutation.message.author_id.clone(),
        body: mutation.message.body.clone(),
        sequence: mutation.message.sequence,
    });
    events
        .publish(CollabEventKind::RoomsChanged {
            room_id: mutation.message.room_id.clone(),
        })
        .await;
    events
        .publish(CollabEventKind::BoardsChanged {
            room_id: mutation.message.room_id.clone(),
        })
        .await;
}
