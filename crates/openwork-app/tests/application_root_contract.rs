use openwork_app::{ApplicationConfig, OpenWorkApplication};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn application_root_is_the_single_send_sync_host_entrypoint() {
    assert_send_sync::<OpenWorkApplication>();

    let _config = ApplicationConfig::from_env_or_local();
}

#[test]
fn application_root_exposes_explicit_provider_thread_and_turn_services() {
    fn assert_api(application: &OpenWorkApplication) {
        let _ = application.providers();
        let _ = application.threads();
        let _ = application.turns();
    }

    let _ = assert_api;
}
