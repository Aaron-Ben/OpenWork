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

#[test]
fn event_payload_fields_match_the_desktop_camel_case_contract() {
    let envelope = openwork_collab::event::CollabEventEnvelope {
        version: 1,
        sequence: 7,
        event: CollabEventKind::ReplyHeld {
            agent_id: "alice".to_string(),
            room_id: "general".to_string(),
            peer_sequence: 4,
        },
    };

    assert_eq!(
        serde_json::to_value(envelope).unwrap(),
        serde_json::json!({
            "version": 1,
            "sequence": 7,
            "type": "reply_held",
            "agentId": "alice",
            "roomId": "general",
            "peerSequence": 4
        })
    );
}
