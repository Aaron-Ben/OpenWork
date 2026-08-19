//! 启动自检的验收：探的是行为，不是版本号。
//!
//! 每个用例起一个假 opencode——一个打印 listening 行然后挂住的脚本，
//! 加一个只提供指定路由的 HTTP 服务——用来构造"健康但契约已变"的服务器。

#![cfg(unix)]

use std::{net::SocketAddr, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use axum::{Json, Router, http::HeaderMap, routing::get};
use openwork_collab::opencode::{DIRECTORY_HEADER, OpenCodeClient, OpenCodeSupervisor};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

fn healthy() -> Router {
    Router::new().route(
        "/global/health",
        get(|| async { Json(serde_json::json!({"healthy": true, "version": "1.18.18"})) }),
    )
}

async fn serve(router: Router) -> (SocketAddr, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (address, handle)
}

/// 写一个假 opencode 二进制：宣告 listening 行，然后不退出。
async fn fake_binary(directory: &tempfile::TempDir, address: SocketAddr) -> PathBuf {
    let binary = directory.path().join("fake-opencode");
    tokio::fs::write(
        &binary,
        format!(
            "#!/bin/sh\necho 'opencode server listening on http://{address}'\nwhile true; do sleep 1; done\n"
        ),
    )
    .await
    .unwrap();
    let mut permissions = tokio::fs::metadata(&binary).await.unwrap().permissions();
    permissions.set_mode(0o700);
    tokio::fs::set_permissions(&binary, permissions)
        .await
        .unwrap();
    binary
}

/// 返回 token：成功路径下 supervise 任务只在 cancel 时退出，调用方必须持有它，
/// 否则 `shutdown()` 会永远等下去。
async fn start_against(router: Router) -> (Result<OpenCodeSupervisor, String>, CancellationToken) {
    let (address, server) = serve(router).await;
    let directory = tempfile::tempdir().unwrap();
    let binary = fake_binary(&directory, address).await;
    let cancel = CancellationToken::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(20),
        OpenCodeSupervisor::start_with_binary(&binary, cancel.clone()),
    )
    .await
    .expect("startup must not hang")
    .map_err(|error| error.to_string());
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        server.abort();
        let _ = server.await;
    })
    .await;
    (outcome, cancel)
}

#[tokio::test]
async fn self_check_rejects_a_server_that_is_healthy_but_lacks_the_v1_session_family() {
    let (outcome, _cancel) = start_against(healthy()).await;
    let error = outcome
        .err()
        .expect("a healthy server without /session must not be accepted");

    // 诊断必须指名是哪一条探测不满足，否则等于没查。
    assert!(error.contains("self-check"), "{error}");
    assert!(error.contains("GET /session"), "{error}");
}

#[tokio::test]
async fn self_check_rejects_a_v2_shaped_session_response() {
    // v2 的 /api/session 返回 {data, cursor}；形状不对说明 v1 路径族已经变了，
    // 而 prompt_async 只存在于 v1。
    let router = healthy().route(
        "/session",
        get(|| async { Json(serde_json::json!({"data": [], "cursor": null})) }),
    );

    let (outcome, _cancel) = start_against(router).await;
    let error = outcome
        .err()
        .expect("a v2-shaped /session response must not be accepted");

    assert!(error.contains("GET /session"), "{error}");
    assert!(error.contains("expected a JSON array"), "{error}");
}

#[tokio::test]
async fn self_check_accepts_a_server_that_satisfies_every_probe() {
    let router = healthy()
        .route("/session", get(|| async { Json(serde_json::json!([])) }))
        .route("/agent", get(|| async { Json(serde_json::json!([])) }))
        .route("/global/event", get(|| async { "" }));

    let (outcome, cancel) = start_against(router).await;
    let supervisor = outcome.expect("a server satisfying every probe must be accepted");

    // 版本只进诊断，不作判据——这里只确认它被记录下来了。
    assert_eq!(supervisor.current().unwrap().version.to_string(), "1.18.18");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), supervisor.shutdown())
        .await
        .expect("cancelled supervisor must stop");
}

#[tokio::test]
async fn instance_event_stream_uses_the_agent_directory_and_unwraps_the_event() {
    let router = Router::new().route(
        "/event",
        get(|headers: HeaderMap| async move {
            assert_eq!(
                headers
                    .get(DIRECTORY_HEADER)
                    .and_then(|value| value.to_str().ok()),
                Some("/tmp/alice-home")
            );
            ([
                ("content-type", "text/event-stream"),
                ("cache-control", "no-cache"),
            ], "data: {\"type\":\"session.status\",\"properties\":{\"sessionID\":\"ses_alice\",\"status\":{\"type\":\"busy\"}}}\n\n")
        }),
    );
    let (address, server) = serve(router).await;
    let client = OpenCodeClient::new(format!("http://{address}"));
    let mut stream = client
        .events(std::path::Path::new("/tmp/alice-home"))
        .await
        .unwrap();

    let event = stream.next().await.unwrap();
    assert_eq!(
        event.directory.as_deref(),
        Some(std::path::Path::new("/tmp/alice-home"))
    );
    assert_eq!(event.event_type(), Some("session.status"));
    assert_eq!(event.session_id(), Some("ses_alice"));
    assert_eq!(event.session_status(), Some("busy"));

    server.abort();
}
