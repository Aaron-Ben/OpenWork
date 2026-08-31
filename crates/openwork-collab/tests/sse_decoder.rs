use openwork_collab::protocol::sse::SseDecoder;

#[test]
fn decoder_reassembles_fragmented_utf8_and_multiple_sse_events() {
    let mut decoder = SseDecoder::default();
    let bytes = "event: wake\nid: one\ndata: {\"body\":\"你好\"}\n\n: keepalive\n\nevent: wake\ndata: first\ndata: second\n\n".as_bytes();
    let split = bytes.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
    assert!(decoder.push(&bytes[..split]).unwrap().is_empty());
    let events = decoder.push(&bytes[split..]).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event.as_deref(), Some("wake"));
    assert_eq!(events[0].id.as_deref(), Some("one"));
    assert_eq!(events[0].data, "{\"body\":\"你好\"}");
    assert_eq!(events[1].data, "first\nsecond");
}

#[test]
fn decoder_accepts_crlf_boundaries() {
    let mut decoder = SseDecoder::default();
    let events = decoder.push(b"event: wake\r\ndata: {}\r\n\r\n").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data, "{}");
}

#[test]
fn decoder_rejects_an_unbounded_event_without_a_delimiter() {
    let mut decoder = SseDecoder::default();
    let oversized = vec![b'x'; 1024 * 1024 + 1];

    let error = decoder.push(&oversized).unwrap_err();

    assert!(error.to_string().contains("decoder limit"));
}
