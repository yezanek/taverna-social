use activitypub_federation::activity_sending::SendActivityTask;
use anyhow::Result;
use async_trait::async_trait;
use loco_rs::{Result as LocoResult, app::AppContext, bgworker::BackgroundWorker};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    FromQueryResult, QueryFilter, Set, Statement,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;

use crate::federation::activities::SendActivity;
use crate::federation::traits::{DbCommunity, DbUser};
use crate::models::_entities::{communities, outbound_activities, users};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliverActivityArgs {
    pub activity_id: uuid::Uuid,
}

pub struct DeliverActivityWorker {
    pub db: DatabaseConnection,
}

#[async_trait]
impl BackgroundWorker<DeliverActivityArgs> for DeliverActivityWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { db: ctx.db.clone() }
    }

    #[instrument(skip(self))]
    async fn perform(&self, args: DeliverActivityArgs) -> LocoResult<()> {
        let row = outbound_activities::Entity::find_by_id(args.activity_id)
            .one(&self.db)
            .await
            .map_err(|e| loco_rs::Error::string(&format!("DB error: {}", e)))?
            .ok_or_else(|| loco_rs::Error::string("Activity not found"))?;

        if row.state != "pending" {
            return Ok(());
        }

        // Build SendActivity wrapper from stored JSON
        let activity_json = &row.activity_json;
        let activity_id_str = activity_json
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| loco_rs::Error::string("Activity JSON missing 'id'"))?;

        let activity_id = Url::parse(activity_id_str)
            .map_err(|e| loco_rs::Error::string(&format!("Invalid activity id URL: {}", e)))?;

        let actor_id = Url::parse(&row.actor_ap_id)
            .map_err(|e| loco_rs::Error::string(&format!("Invalid actor id URL: {}", e)))?;

        let send_activity = SendActivity {
            id: activity_id,
            actor: actor_id.clone(),
            json: activity_json.clone(),
        };

        // Build DomainConfig using env vars
        let domain = std::env::var("TAVERNA_DOMAIN")
            .or_else(|_| std::env::var("DOMAIN"))
            .unwrap_or_else(|_| "localhost:3000".to_string());

        let federation_config = activitypub_federation::config::FederationConfig::builder()
            .domain(&domain)
            .app_data(self.db.clone())
            .debug(cfg!(debug_assertions))
            .build()
            .await
            .map_err(|e| loco_rs::Error::string(&format!("Federation config error: {}", e)))?;

        let data = federation_config.to_request_data();

        let signer = match resolve_signer(&self.db, &row.actor_ap_id).await {
            Ok(signer) => signer,
            Err(e) => {
                return update_failure(&self.db, &row, e).await;
            }
        };

        let mut any_failed = false;
        for inbox_str in &row.target_inboxes {
            let inbox_url = match Url::parse(inbox_str) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!("Invalid inbox URL {}: {}", inbox_str, e);
                    continue;
                }
            };

            let prepared = match &signer {
                Signer::User(actor) => {
                    SendActivityTask::prepare(&send_activity, actor, vec![inbox_url], &data).await
                }
                Signer::Community(actor) => {
                    SendActivityTask::prepare(&send_activity, actor, vec![inbox_url], &data).await
                }
            };
            let tasks = match prepared {
                Ok(tasks) => tasks,
                Err(e) => {
                    tracing::warn!("Failed to prepare delivery to {}: {:#}", inbox_str, e);
                    any_failed = true;
                    continue;
                }
            };

            for task in &tasks {
                match task.sign_and_send(&data).await {
                    Ok(_) => tracing::info!("Delivered to {}", inbox_str),
                    Err(e) => {
                        tracing::warn!("Failed to deliver to {}: {:#}", inbox_str, e);
                        any_failed = true;
                    }
                }
            }
        }

        if any_failed {
            return update_failure(&self.db, &row, anyhow::anyhow!("Some deliveries failed")).await;
        }

        mark_sent(&self.db, &row).await
    }
}

/// The local actor whose key signs an outbound activity.
enum Signer {
    User(DbUser),
    Community(DbCommunity),
}

/// Finds the local user or community that owns `actor_ap_id` and has a private key.
async fn resolve_signer(db: &DatabaseConnection, actor_ap_id: &str) -> Result<Signer> {
    if let Some(user) = users::Entity::find()
        .filter(users::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await?
        && user.private_key.is_some()
    {
        return Ok(Signer::User(DbUser::new(user)?));
    }
    if let Some(community) = communities::Entity::find()
        .filter(communities::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await?
        && community.private_key.is_some()
    {
        return Ok(Signer::Community(DbCommunity::new(community)?));
    }
    anyhow::bail!("No local actor with a private key for: {}", actor_ap_id)
}

async fn update_failure(
    db: &DatabaseConnection,
    row: &outbound_activities::Model,
    error: anyhow::Error,
) -> LocoResult<()> {
    let mut active: outbound_activities::ActiveModel = row.clone().into();
    let new_attempts = row.attempts + 1;
    active.attempts = Set(new_attempts);
    let error_str = format!("{:#}", error);
    if new_attempts >= 5 {
        active.state = Set("failed".to_string());
        active.last_error = Set(Some(error_str));
    } else {
        let delay = backoff_seconds(new_attempts);
        active.next_attempt_at = Set(Some(
            (chrono::Utc::now() + chrono::Duration::seconds(delay as i64)).into(),
        ));
        active.last_error = Set(Some(error_str));
    }
    ActiveModelTrait::update(active, db)
        .await
        .map_err(|e| loco_rs::Error::string(&format!("DB update error: {}", e)))?;
    Err(loco_rs::Error::string(&format!(
        "Delivery failed after {} attempts",
        new_attempts
    )))
}

async fn mark_sent(db: &DatabaseConnection, row: &outbound_activities::Model) -> LocoResult<()> {
    let mut active: outbound_activities::ActiveModel = row.clone().into();
    active.state = Set("sent".to_string());
    ActiveModelTrait::update(active, db)
        .await
        .map_err(|e| loco_rs::Error::string(&format!("DB update error: {}", e)))?;
    Ok(())
}

fn backoff_seconds(attempt: i32) -> u64 {
    match attempt {
        1 => 10,
        2 => 30,
        3 => 120,
        4 => 600,
        _ => 3600,
    }
}

/// Queues an activity for delivery (ADR-019). Request handlers call this and
/// return immediately; [`dispatch_due`] hands due rows to the worker.
pub async fn enqueue_activity(
    db: &DatabaseConnection,
    activity: &serde_json::Value,
    actor_ap_id: &str,
    inboxes: Vec<String>,
) -> Result<()> {
    let mut unique_inboxes: Vec<String> = inboxes.into_iter().filter(|i| !i.is_empty()).collect();
    unique_inboxes.sort();
    unique_inboxes.dedup();

    if unique_inboxes.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().into();
    let active = outbound_activities::ActiveModel {
        id: Set(uuid::Uuid::new_v4()),
        activity_json: Set(activity.clone()),
        target_inboxes: Set(unique_inboxes),
        actor_ap_id: Set(actor_ap_id.to_string()),
        state: Set("pending".to_string()),
        attempts: Set(0),
        next_attempt_at: Set(Some(now)),
        last_error: Set(None),
        created_at: Set(now),
    };

    active.insert(db).await?;

    Ok(())
}

/// How long a dispatched row is leased before it can be picked up again.
const DISPATCH_LEASE_SECONDS: i64 = 300;

/// Claims pending activities that are due and hands each to the delivery worker.
///
/// Claiming pushes `next_attempt_at` forward by a lease, under
/// `FOR UPDATE SKIP LOCKED`, so concurrent pollers never pick the same row.
/// The worker then marks the row sent, or reschedules it with backoff.
pub async fn dispatch_due(ctx: &AppContext, batch_size: u64) -> Result<usize> {
    let db = &ctx.db;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            db.get_database_backend(),
            r#"
            UPDATE outbound_activities
               SET next_attempt_at = NOW() + make_interval(secs => $1)
             WHERE id IN (
               SELECT id FROM outbound_activities
                WHERE state = 'pending' AND next_attempt_at <= NOW()
                ORDER BY next_attempt_at
                LIMIT $2
                FOR UPDATE SKIP LOCKED)
            RETURNING id
            "#,
            [
                (DISPATCH_LEASE_SECONDS as f64).into(),
                (batch_size as i64).into(),
            ],
        ))
        .await?;

    let mut dispatched = 0;
    for row in rows {
        let activity_id: uuid::Uuid = row.try_get("", "id")?;
        DeliverActivityWorker::perform_later(ctx, DeliverActivityArgs { activity_id })
            .await
            .map_err(|e| anyhow::anyhow!("failed to dispatch {}: {}", activity_id, e))?;
        dispatched += 1;
    }
    Ok(dispatched)
}

/// One deduplicated delivery inbox.
#[derive(Debug, FromQueryResult)]
struct FollowerInbox {
    inbox: String,
}

/// Inboxes of accepted remote followers of `target_id`. Each follower's
/// shared inbox is preferred and duplicates are removed in SQL, so one
/// request goes to each instance instead of one per follower (Part B7) and
/// only one row per instance leaves the database.
async fn follower_inboxes(
    db: &DatabaseConnection,
    target_column: &str,
    target_id: uuid::Uuid,
) -> Result<Vec<Url>> {
    let sql = format!(
        r#"
        SELECT DISTINCT COALESCE(NULLIF(ra.shared_inbox_url, ''), ra.inbox_url) AS inbox
          FROM follows f
          JOIN remote_actors ra ON ra.ap_id = f.remote_follower_ap_id
         WHERE f.{target_column} = $1 AND f.accepted = true
           AND COALESCE(NULLIF(ra.shared_inbox_url, ''), ra.inbox_url) <> ''
        "#
    );
    let rows = FollowerInbox::find_by_statement(Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        [target_id.into()],
    ))
    .all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match Url::parse(&row.inbox) {
            Ok(u) => Some(u),
            Err(e) => {
                tracing::warn!("Skipping invalid follower inbox {}: {}", row.inbox, e);
                None
            }
        })
        .collect())
}

/// Deduplicated inboxes of a local community's remote followers.
pub async fn get_follower_inboxes_deduplicated(
    db: &DatabaseConnection,
    community_id: uuid::Uuid,
) -> Result<Vec<String>> {
    Ok(follower_inboxes(db, "community_id", community_id)
        .await?
        .into_iter()
        .map(String::from)
        .collect())
}

/// Deduplicated inboxes of a local user's remote followers.
pub async fn get_user_follower_inboxes_deduplicated(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
) -> Result<Vec<String>> {
    Ok(follower_inboxes(db, "following_id", user_id)
        .await?
        .into_iter()
        .map(String::from)
        .collect())
}
