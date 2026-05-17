use async_trait::async_trait;
use axum::Router as AxumRouter;
use loco_rs::{
    Result,
    app::{AppContext, Hooks, Initializer},
    boot::{BootResult, StartMode, create_app},
    config::Config,
    controller::AppRoutes,
    environment::Environment,
    task::Tasks,
};
use migration::Migrator;
use tracing::info;

use crate::common::config::TavernaConfig;
use crate::controllers::{auth, communities, federation, home, posts, theme};
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

        let home_routes = loco_rs::controller::Routes::new().add("/", get(home::index));

        let community_routes = loco_rs::controller::Routes::new()
            .add("/c/", get(communities::list))
            .add("/c/{slug}", get(communities::feed))
            .add("/c/{slug}/new", get(communities::new_post_form))
            .add("/c/{slug}/posts", post(communities::create_post))
            .add("/c/{slug}/join", post(communities::join))
            .add("/c/{slug}/leave", post(communities::leave))
            .add(
                "/c/{slug}/settings",
                get(communities::settings_form).post(communities::update_settings),
            )
            .add("/c/{slug}/inbox", post(federation::inbox::community_inbox))
            .add(
                "/c/{slug}/outbox",
                get(federation::actors::community_outbox),
            )
            .add(
                "/c/{slug}/followers",
                get(federation::actors::community_followers),
            );

        let post_routes = loco_rs::controller::Routes::new()
            .add("/c/{slug}/posts/{id}", get(posts::detail))
            .add("/c/{slug}/posts/{id}/vote", post(posts::vote))
            .add("/c/{slug}/posts/{id}/delete", post(posts::delete))
            .add("/c/{slug}/posts/{id}/comments", post(posts::create_comment))
            .add(
                "/c/{slug}/posts/{id}/comments/{cid}/vote",
                post(posts::vote_comment),
            );

        let user_routes = loco_rs::controller::Routes::new()
            .add("/u/{username}", get(federation::actors::user_actor))
            .add("/u/{username}/inbox", post(federation::inbox::user_inbox))
            .add("/u/{username}/outbox", get(federation::actors::user_outbox))
            .add(
                "/u/{username}/followers",
                get(federation::actors::user_followers),
            )
            .add(
                "/u/{username}/following",
                get(federation::actors::user_following),
            );

        let webfinger_routes = loco_rs::controller::Routes::new().add(
            "/.well-known/webfinger",
            get(federation::webfinger::webfinger),
        );

        let nodeinfo_routes = loco_rs::controller::Routes::new()
            .add(
                "/.well-known/nodeinfo",
                get(federation::nodeinfo::well_known_nodeinfo),
            )
            .add("/nodeinfo/2.0", get(federation::nodeinfo::nodeinfo_v2));

        let note_routes = loco_rs::controller::Routes::new()
            .add("/posts/{id}", get(federation::notes::post_note))
            .add("/comments/{id}", get(federation::notes::comment_note));

        AppRoutes::with_default_routes()
            .add_route(auth_routes)
            .add_route(theme_routes)
            .add_route(home_routes)
            .add_route(community_routes)
            .add_route(post_routes)
            .add_route(user_routes)
            .add_route(webfinger_routes)
            .add_route(nodeinfo_routes)
            .add_route(note_routes)
    }

    async fn after_routes(router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {
        let taverna_config = TavernaConfig::from_env()
            .map_err(|e| loco_rs::Error::string(&format!("Config error: {}", e)))?;

        let mut router = router.layer(axum::middleware::from_fn(
            initializers::content_negotiation::content_negotiation_middleware,
        ));

        let domain = taverna_config.domain.clone();
        let federation_config = activitypub_federation::config::FederationConfig::builder()
            .domain(&domain)
            .app_data(ctx.db.clone())
            .debug(cfg!(debug_assertions))
            .build()
            .await
            .map_err(|e| loco_rs::Error::string(&format!("Federation config error: {}", e)))?;

        info!("Federation configured for domain: {}", domain);

        router = router.layer(activitypub_federation::config::FederationMiddleware::new(
            federation_config,
        ));

        let router = router.layer(axum::middleware::from_fn(
            initializers::theme_middleware::theme_middleware,
        ));

        let router = router.layer(Extension(taverna_config));
        Ok(router)
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn connect_workers(ctx: &AppContext, queue: &loco_rs::bgworker::Queue) -> Result<()> {
        queue
            .register(crate::workers::deliver_activity::DeliverActivityWorker {
                db: ctx.db.clone(),
            })
            .await?;
        Ok(())
    }

    async fn truncate(_ctx: &AppContext) -> Result<()> {
        Ok(())
    }

    async fn seed(_ctx: &AppContext, _path: &std::path::Path) -> Result<()> {
        Ok(())
    }
}
