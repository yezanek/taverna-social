use std::net::SocketAddr;

use loco_rs::boot::BootResult;
use loco_rs::testing::request::boot_test;
use taverna::app::App;

/// Boots a fresh app for the current test.
///
/// Every `#[tokio::test]` runs on its own runtime, and a database pool created
/// on one runtime cannot be used from another (connections die with the
/// runtime that opened them, which showed up as `ConnectionAcquire(Timeout)`).
/// So each test boots its own app. The test config sets
/// `dangerously_recreate`, so this also gives every test a clean database.
/// Tests touching the database are `#[serial]`, so recreating is safe.
///
/// The boot result is leaked so existing callers can keep a `&'static`
/// reference; test processes are short-lived.
pub async fn shared_boot() -> &'static BootResult {
    dotenvy::dotenv().ok();
    unsafe { std::env::set_var("RATE_LIMITING_ENABLED", "false") };
    Box::leak(Box::new(boot_test::<App>().await.unwrap()))
}

#[allow(dead_code)]
pub async fn test_server_and_ctx() -> (axum_test::TestServer, loco_rs::app::AppContext) {
    let boot = shared_boot().await;
    let routes = boot.router.clone().unwrap();
    let server = axum_test::TestServer::new_with_config(
        routes.into_make_service_with_connect_info::<SocketAddr>(),
        axum_test::TestServerConfig {
            save_cookies: false,
            default_content_type: None,
            ..Default::default()
        },
    )
    .unwrap();
    (server, boot.app_context.clone())
}
