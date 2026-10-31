//! Background loops for federation (Phase 8):
//! - every few seconds, hand due `outbound_activities` rows to the delivery
//!   worker (ADR-019: handlers only enqueue);
//! - once a day, expire the remote content cache.
//!
//! These run inside the app process and use PostgreSQL only (ADR-001). They are
//! not started in the test environment, where tests drive the workers directly.
use async_trait::async_trait;
use loco_rs::{
    Result,
    app::{AppContext, Initializer},
    bgworker::BackgroundWorker,
    environment::Environment,
};
use std::time::Duration;

use crate::workers::deliver_activity::dispatch_due;
use crate::workers::expire_remote_cache::{ExpireRemoteCacheArgs, ExpireRemoteCacheWorker};

const DELIVERY_POLL_INTERVAL: Duration = Duration::from_secs(5);
const DELIVERY_BATCH_SIZE: u64 = 100;
const EXPIRY_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct FederationJobsInitializer;

#[async_trait]
impl Initializer for FederationJobsInitializer {
    fn name(&self) -> String {
        "federation-jobs".to_string()
    }

    async fn before_run(&self, ctx: &AppContext) -> Result<()> {
        if ctx.environment == Environment::Test {
            return Ok(());
        }

        let delivery_ctx = ctx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(DELIVERY_POLL_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                match dispatch_due(&delivery_ctx, DELIVERY_BATCH_SIZE).await {
                    Ok(0) => {}
                    Ok(n) => tracing::debug!(dispatched = n, "dispatched outbound activities"),
                    Err(e) => tracing::warn!(error = %e, "outbound dispatch failed"),
                }
            }
        });

        let expiry_ctx = ctx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(EXPIRY_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Err(e) = ExpireRemoteCacheWorker::perform_later(
                    &expiry_ctx,
                    ExpireRemoteCacheArgs::default(),
                )
                .await
                {
                    tracing::warn!(error = %e, "failed to schedule remote cache expiry");
                }
            }
        });

        Ok(())
    }
}
