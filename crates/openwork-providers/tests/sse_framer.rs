use openwork_providers::SseFramer;

#[test]
fn sse_framer_handles_utf8_split_and_multiline_data() {
    let mut framer = SseFramer::default();
    let text = "event: message\ndata: 你\ndata: 好\n\n".as_bytes();
    let split = text.iter().position(|byte| *byte >= 0x80).unwrap() + 1;

    assert!(framer.push(&text[..split]).unwrap().is_empty());
    let frames = framer.push(&text[split..]).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].event.as_deref(), Some("message"));
    assert_eq!(frames[0].data, "你\n好");
}
