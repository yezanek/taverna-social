use std::net::SocketAddr;

use loco_rs::boot::BootResult;
use loco_rs::testing::request::boot_test;
use taverna::app::App;

static BOOT: tokio::sync::OnceCell<BootResult> = tokio::sync::OnceCell::const_new();

pub async fn shared_boot() -> &'static BootResult {
    BOOT.get_or_init(|| async {
        dotenvy::dotenv().ok();
        unsafe { std::env::set_var("RATE_LIMITING_ENABLED", "false") };
        boot_test::<App>().await.unwrap()
    })
    .await
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
