use anyhow::Result;
use async_trait::async_trait;
use loco_rs::{Result as LocoResult, app::AppContext, bgworker::BackgroundWorker};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use url::Url;

use crate::federation::activities::SendActivity;
use crate::federation::traits::DbUser;
use crate::models::_entities::outbound_activities;

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

        let (_public_key_pem, private_key_pem) = fetch_actor_keys(&self.db, &row.actor_ap_id)
            .await
            .map_err(|e| loco_rs::Error::string(&e.to_string()))?;

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

        let mut any_failed = false;
        for inbox_str in &row.target_inboxes {
            let inbox_url = match Url::parse(inbox_str) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!("Invalid inbox URL {}: {}", inbox_str, e);
                    continue;
                }
            };

            // We need to provide a temporary actor with the correct private key.
            // Since the actor is the key owner, we create a DbUser with the correct key.
            // The private key needs to be set on the actor for the crate to use it for signing.
            let temp_user =
                match create_temp_user(&self.db, &row.actor_ap_id, &private_key_pem).await {
                    Ok(u) => u,
                    Err(e) => {
                        tracing::warn!("Failed to create temp user for {}: {}", row.actor_ap_id, e);
                        any_failed = true;
                        continue;
                    }
                };
            let actor = match DbUser::new(temp_user) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!("Failed to create DbUser for {}: {}", row.actor_ap_id, e);
                    any_failed = true;
                    continue;
                }
            };

            let tasks = activitypub_federation::activity_sending::SendActivityTask::prepare(
                &send_activity,
                &actor,
                vec![inbox_url.clone()],
                &data,
            )
            .await
            .map_err(|e| loco_rs::Error::string(&format!("Prepare error: {:#}", e)))?;

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

/// Create a temporary users::Model with the correct private key for signing.
/// This user may not exist in the DB - we create it temporarily for the Actor trait impl.
async fn create_temp_user(
    db: &DatabaseConnection,
    actor_ap_id: &str,
    private_key_pem: &str,
) -> Result<crate::models::_entities::users::Model> {
    // First try to find existing user
    if let Ok(Some(user)) = crate::models::_entities::users::Entity::find()
        .filter(crate::models::_entities::users::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await
    {
        return Ok(user);
    }

    // Try finding as community
    if let Ok(Some(community)) = crate::models::_entities::communities::Entity::find()
        .filter(crate::models::_entities::communities::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await
    {
        // Return a synthetic user-like model for signing purposes
        let public_key = community.public_key.clone();
        let fake_model = crate::models::_entities::users::Model {
            id: uuid::Uuid::nil(),
            username: community.slug.clone(),
            email: None,
            password_hash: None,
            display_name: Some(community.name.clone()),
            bio: community.description.clone(),
            avatar_url: None,
            ap_id: actor_ap_id.to_string(),
            public_key,
            private_key: Some(private_key_pem.to_string()),
            is_local: true,
            is_admin: false,
            is_banned: false,
            email_verified: false,
            standing_score: 0,
            suspended_at: None,
            inbox_url: community.inbox_url.clone(),
            shared_inbox_url: None,
            followers_url: community.followers_url.clone(),
            last_fetched_at: None,
            created_at: chrono::Utc::now().into(),
            updated_at: chrono::Utc::now().into(),
        };
        return Ok(fake_model);
    }

    anyhow::bail!("No actor found for: {}", actor_ap_id)
}

async fn fetch_actor_keys(db: &DatabaseConnection, actor_ap_id: &str) -> Result<(String, String)> {
    if let Ok(Some(user)) = crate::models::_entities::users::Entity::find()
        .filter(crate::models::_entities::users::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await
        && let Some(pk) = &user.private_key
    {
        return Ok((user.public_key.clone(), pk.clone()));
    }

    if let Ok(Some(community)) = crate::models::_entities::communities::Entity::find()
        .filter(crate::models::_entities::communities::Column::ApId.eq(actor_ap_id))
        .one(db)
        .await
        && let Some(pk) = &community.private_key
    {
        return Ok((community.public_key.clone(), pk.clone()));
    }

    anyhow::bail!("No private key found for actor: {}", actor_ap_id)
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

pub async fn enqueue_to_followers(
    db: &DatabaseConnection,
    activity: &serde_json::Value,
    actor_ap_id: &str,
    follower_inboxes: Vec<String>,
) -> Result<()> {
    let mut unique_inboxes: Vec<String> = follower_inboxes
        .into_iter()
        .filter(|i| !i.is_empty())
        .collect();
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

pub async fn get_follower_inboxes_for_community(
    db: &DatabaseConnection,
    community_id: uuid::Uuid,
) -> Result<Vec<String>> {
    use crate::models::_entities::{follows, users};
    use sea_orm::QueryFilter;

    let follow_relations = follows::Entity::find()
        .filter(follows::Column::CommunityId.eq(community_id))
        .filter(follows::Column::Accepted.eq(true))
        .all(db)
        .await?;

    let mut inboxes = Vec::new();
    for f in &follow_relations {
        if let Ok(Some(u)) = users::Entity::find_by_id(f.follower_id).one(db).await
            && !u.is_local
            && !u.inbox_url.is_empty()
        {
            inboxes.push(u.inbox_url.clone());
        }
    }

    inboxes.sort();
    inboxes.dedup();
    Ok(inboxes)
}

pub async fn get_user_follower_inboxes(
    db: &DatabaseConnection,
    user_id: uuid::Uuid,
) -> Result<Vec<String>> {
    use crate::models::_entities::{follows, users};
    use sea_orm::QueryFilter;

    let follow_relations = follows::Entity::find()
        .filter(follows::Column::FollowingId.eq(user_id))
        .filter(follows::Column::Accepted.eq(true))
        .all(db)
        .await?;

    let mut inboxes = Vec::new();
    for f in &follow_relations {
        if let Ok(Some(u)) = users::Entity::find_by_id(f.follower_id).one(db).await
            && !u.is_local
            && !u.inbox_url.is_empty()
        {
            inboxes.push(u.inbox_url.clone());
        }
    }

    inboxes.sort();
    inboxes.dedup();
    Ok(inboxes)
}
