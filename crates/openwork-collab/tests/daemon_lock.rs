use openwork_collab::daemon::bind_single_instance;

#[tokio::test]
async fn second_daemon_gets_a_readable_socket_occupied_error() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let first = bind_single_instance(&socket).await.unwrap();
    let error = bind_single_instance(&socket).await.unwrap_err().to_string();

    assert!(error.contains("already running"));
    assert!(error.contains("daemon.sock"));
    drop(first);
}
