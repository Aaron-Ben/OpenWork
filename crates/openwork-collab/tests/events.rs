use openwork_collab::event::{CollabEventKind, CollabEventPublisher};

#[tokio::test]
async fn event_publisher_sequences_and_replays_missed_invalidations() {
    let publisher = CollabEventPublisher::new(8);
    let first = publisher
        .publish(CollabEventKind::RoomsChanged {
            room_id: "general".to_string(),
        })
        .await;
    let second = publisher.publish(CollabEventKind::AgentsChanged).await;

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    let subscription = publisher.subscribe(1).await;
    assert_eq!(subscription.replay, vec![second]);
}
