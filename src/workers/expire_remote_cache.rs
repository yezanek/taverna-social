//! Daily expiry of the remote federated content cache (PHASE_8.md Part A).
use async_trait::async_trait;
use loco_rs::{Result as LocoResult, app::AppContext, bgworker::BackgroundWorker};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExpireRemoteCacheArgs {}

pub struct ExpireRemoteCacheWorker {
    pub db: DatabaseConnection,
}

#[async_trait]
impl BackgroundWorker<ExpireRemoteCacheArgs> for ExpireRemoteCacheWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { db: ctx.db.clone() }
    }

    #[instrument(skip(self))]
    async fn perform(&self, _args: ExpireRemoteCacheArgs) -> LocoResult<()> {
        let (comments, posts, actors, received) =
            crate::models::remote::expire_remote_cache(&self.db)
                .await
                .map_err(|e| loco_rs::Error::string(&format!("remote cache expiry failed: {e}")))?;
        tracing::info!(
            comments,
            posts,
            actors,
            received,
            "expired remote federated cache"
        );
        Ok(())
    }
}
