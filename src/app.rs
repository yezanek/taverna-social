use async_trait::async_trait;
use axum::Router as AxumRouter;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    environment::Environment,
    task::Tasks,
    Result,
};
use migration::Migrator;
use tracing::info;

use crate::common::config::TavernaConfig;
use crate::controllers::{auth, theme};
use crate::initializers;
use axum::Extension;

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(
            initializers::view_engine::ViewEngineInitializer,
        )])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        use axum::routing::{get, post};

        let auth_routes = loco_rs::controller::Routes::new()
            .add("/register", get(auth::register_form).post(auth::register))
            .add("/login", get(auth::login_form).post(auth::login))
            .add("/logout", post(auth::logout));

        let theme_routes = loco_rs::controller::Routes::new().add("/theme", post(theme::set_theme));

        AppRoutes::with_default_routes()
            .add_route(auth_routes)
            .add_route(theme_routes)
    }

    async fn after_routes(router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {
        let taverna_config = TavernaConfig::from_env()
            .map_err(|e| loco_rs::Error::string(&format!("Config error: {}", e)))?;

        if taverna_config.federation.enabled {
            let domain = taverna_config.domain.clone();
            let federation_config = activitypub_federation::config::FederationConfig::builder()
                .domain(&domain)
                .app_data(ctx.db.clone())
                .debug(cfg!(debug_assertions))
                .build()
                .await
                .map_err(|e| loco_rs::Error::string(&format!("Federation config error: {}", e)))?;

            info!("Federation configured for domain: {}", domain);

            let router = router.layer(activitypub_federation::config::FederationMiddleware::new(
                federation_config,
            ));

            let router = router.layer(Extension(taverna_config));
            return Ok(router);
        }

        let router = router.layer(Extension(taverna_config));
        Ok(router)
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn connect_workers(_ctx: &AppContext, _queue: &loco_rs::bgworker::Queue) -> Result<()> {
        Ok(())
    }

    async fn truncate(_ctx: &AppContext) -> Result<()> {
        Ok(())
    }

    async fn seed(_ctx: &AppContext, _path: &std::path::Path) -> Result<()> {
        Ok(())
    }
}
