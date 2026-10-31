use activitypub_federation::config::Data;
use activitypub_federation::protocol::verification::verify_domains_match;
use activitypub_federation::traits::{Actor, Object};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use url::Url;

use crate::federation::protocol::{Group, Person, RemoteActorJson};
use crate::models::_entities::{communities, remote_actors, users};
use crate::models::remote::RemoteActorModel;
use chrono::{DateTime, Utc};
use sea_orm::Set;

/// Newtype wrapper for local user that implements Object + Actor.
/// Stores the parsed AP ID URL to satisfy `id() -> &Url` without allocation.
#[derive(Clone, Debug)]
pub struct DbUser(pub users::Model, pub Url);

impl DbUser {
    pub fn new(model: users::Model) -> Result<Self, url::ParseError> {
        let url = model.ap_id.parse()?;
        Ok(DbUser(model, url))
    }
}

#[async_trait::async_trait]
impl Object for DbUser {
    type DataType = DatabaseConnection;
    type Kind = Person;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.1
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let db = data.app_data();
        let user = users::Entity::find()
            .filter(users::Column::ApId.eq(object_id.as_str()))
            .one(db)
            .await?;
        match user {
            Some(u) => Ok(Some(DbUser::new(u)?)),
            None => Ok(None),
        }
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        let id_url = self.1.clone();
        Ok(Person {
            kind: crate::federation::protocol::PersonKind::Person,
            id: id_url.clone(),
            url: Some(id_url.clone()),
            preferred_username: Some(self.0.username),
            name: self.0.display_name.clone(),
            summary: self.0.bio.clone(),
            icon: self.0.avatar_url.as_ref().and_then(|u| u.parse().ok()),
            inbox: self.0.inbox_url.parse()?,
            outbox: Some(format!("{}/outbox", self.0.ap_id.trim_end_matches('/')).parse()?),
            followers: Some(format!("{}/followers", self.0.ap_id.trim_end_matches('/')).parse()?),
            following: Some(format!("{}/following", self.0.ap_id.trim_end_matches('/')).parse()?),
            public_key: Some(serde_json::json!({
                "id": format!("{}#main-key", self.0.ap_id),
                "owner": self.0.ap_id,
                "publicKeyPem": self.0.public_key,
            })),
            manually_approves_followers: Some(false),
            published: Some(self.0.created_at.format("%+").to_string()),
        })
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        _data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        // ★ CRITICAL: verify_domains_match prevents cross-domain actor spoofing
        verify_domains_match(&json.id, expected_domain)?;
        Ok(())
    }

    /// `users` holds local accounts only (ADR-017). Remote people are
    /// [`RemoteActor`]s, so this only resolves ids of our own users.
    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        let db = data.app_data();
        let user = users::Entity::find()
            .filter(users::Column::ApId.eq(json.id.as_str()))
            .one(db)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} is not a local user; remote actors live in remote_actors",
                    json.id
                )
            })?;
        Ok(DbUser::new(user)?)
    }
}

#[async_trait::async_trait]
impl Actor for DbUser {
    fn public_key_pem(&self) -> &str {
        &self.0.public_key
    }

    fn private_key_pem(&self) -> Option<String> {
        self.0.private_key.clone()
    }

    fn inbox(&self) -> Url {
        self.0.inbox_url.parse().unwrap_or_else(|e| {
            tracing::warn!("Invalid inbox URL for {}: {}", self.0.ap_id, e);
            Url::parse("https://localhost/inbox").expect("inbox fallback URL")
        })
    }

    fn shared_inbox(&self) -> Option<Url> {
        self.0
            .shared_inbox_url
            .as_ref()
            .and_then(|u| u.parse().ok())
    }
}

// ── Community (Group) wrapper ────────────────────────────────

/// Newtype wrapper for community that implements Object + Actor.
#[derive(Clone, Debug)]
pub struct DbCommunity(pub communities::Model, pub Url);

impl DbCommunity {
    pub fn new(model: communities::Model) -> Result<Self, url::ParseError> {
        let url = model.ap_id.parse()?;
        Ok(DbCommunity(model, url))
    }
}

#[async_trait::async_trait]
impl Object for DbCommunity {
    type DataType = DatabaseConnection;
    type Kind = Group;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.1
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let db = data.app_data();
        let community = communities::Entity::find()
            .filter(communities::Column::ApId.eq(object_id.as_str()))
            .one(db)
            .await?;
        match community {
            Some(c) => Ok(Some(DbCommunity::new(c)?)),
            None => Ok(None),
        }
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        let id_url = self.1.clone();
        Ok(Group {
            kind: crate::federation::protocol::GroupKind::Group,
            id: id_url.clone(),
            preferred_username: Some(self.0.slug),
            name: Some(self.0.name),
            summary: self.0.description.clone(),
            inbox: self.0.inbox_url.parse()?,
            outbox: Some(format!("{}/outbox", self.0.ap_id.trim_end_matches('/')).parse()?),
            followers: Some(format!("{}/followers", self.0.ap_id.trim_end_matches('/')).parse()?),
            public_key: Some(serde_json::json!({
                "id": format!("{}#main-key", self.0.ap_id),
                "owner": self.0.ap_id,
                "publicKeyPem": self.0.public_key,
            })),
            published: Some(self.0.created_at.format("%+").to_string()),
            attributed_to: Some(id_url),
            posting_restricted_to_mods: Some(self.0.posting_restricted),
            sensitive: Some(self.0.is_nsfw),
        })
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        _data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        verify_domains_match(&json.id, expected_domain)?;
        Ok(())
    }

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        let db = data.app_data();
        let id = json.id.as_str();

        if let Some(existing) = communities::Entity::find()
            .filter(communities::Column::ApId.eq(id))
            .one(db)
            .await?
        {
            return Ok(DbCommunity::new(existing)?);
        }

        let uuid = uuid::Uuid::new_v4();
        let inbox_url = json.inbox.as_str().to_string();
        let ap_id = id.to_string();
        let public_key_raw = json
            .public_key
            .as_ref()
            .and_then(|pk| pk.get("publicKeyPem"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("Missing or empty publicKeyPem for remote community {}", id)
            })?;

        if !public_key_raw.starts_with("-----BEGIN") {
            anyhow::bail!(
                "Invalid PEM format for remote community {}: does not start with -----BEGIN",
                id
            );
        }

        let public_key = public_key_raw.to_string();

        // Communities from remote don't have a user-defined slug; derive one from AP ID
        let slug = format!("remote_{}", &uuid.to_string()[..8]);
        let new_community = communities::ActiveModel {
            id: Set(uuid),
            slug: Set(slug),
            name: Set(json
                .name
                .clone()
                .unwrap_or_else(|| "Remote Community".to_string())),
            description: Set(json.summary.clone()),
            ap_id: Set(ap_id),
            public_key: Set(public_key),
            private_key: Set(None),
            inbox_url: Set(inbox_url),
            followers_url: Set(json
                .followers
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default()),
            is_local: Set(false),
            is_nsfw: Set(false),
            posting_restricted: Set(false),
            last_fetched_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let saved = new_community.insert(db).await?;
        Ok(DbCommunity::new(saved)?)
    }
}

#[async_trait::async_trait]
impl Actor for DbCommunity {
    fn public_key_pem(&self) -> &str {
        &self.0.public_key
    }

    fn private_key_pem(&self) -> Option<String> {
        self.0.private_key.clone()
    }

    fn inbox(&self) -> Url {
        self.0.inbox_url.parse().unwrap_or_else(|e| {
            tracing::warn!("Invalid inbox URL for {}: {}", self.0.ap_id, e);
            Url::parse("https://localhost/inbox").expect("inbox fallback URL")
        })
    }

    fn shared_inbox(&self) -> Option<Url> {
        None
    }
}

// ── Remote actor (any type) ──────────────────────────────────

/// A cached actor from another instance (`remote_actors`, Phase 8).
///
/// This is the actor type inboxes verify HTTP signatures against: the crate
/// resolves the signer through [`RemoteActor::read_from_id`] and, when the
/// cached copy is missing or stale, fetches it and stores it via
/// [`RemoteActor::from_json`].
#[derive(Clone, Debug)]
pub struct RemoteActor(pub remote_actors::Model, pub Url);

impl RemoteActor {
    pub fn new(model: remote_actors::Model) -> Result<Self, url::ParseError> {
        let url = model.ap_id.parse()?;
        Ok(RemoteActor(model, url))
    }
}

#[async_trait::async_trait]
impl Object for RemoteActor {
    type DataType = DatabaseConnection;
    type Kind = RemoteActorJson;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.1
    }

    fn last_refreshed_at(&self) -> Option<DateTime<Utc>> {
        Some(self.0.fetched_at.with_timezone(&Utc))
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        match RemoteActorModel::find(data.app_data(), object_id.as_str()).await? {
            Some(actor) if !actor.public_key.is_empty() => Ok(Some(RemoteActor::new(actor)?)),
            // Placeholder rows have no key yet: report them missing so the
            // crate fetches the real actor before verifying a signature.
            _ => Ok(None),
        }
    }

    async fn into_json(self, _data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        Ok(serde_json::from_value(self.0.raw_json)?)
    }

    async fn verify(
        json: &Self::Kind,
        expected_domain: &Url,
        _data: &Data<Self::DataType>,
    ) -> Result<(), Self::Error> {
        verify_domains_match(&json.id, expected_domain)?;
        Ok(())
    }

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        let raw = serde_json::to_value(&json)?;
        let model = RemoteActorModel::upsert_from_json(data.app_data(), &json, raw).await?;
        Ok(RemoteActor::new(model)?)
    }
}

#[async_trait::async_trait]
impl Actor for RemoteActor {
    fn public_key_pem(&self) -> &str {
        &self.0.public_key
    }

    fn private_key_pem(&self) -> Option<String> {
        None
    }

    fn inbox(&self) -> Url {
        self.0.inbox_url.parse().unwrap_or_else(|e| {
            tracing::warn!("Invalid inbox URL for {}: {}", self.0.ap_id, e);
            Url::parse("https://localhost/inbox").expect("inbox fallback URL")
        })
    }

    fn shared_inbox(&self) -> Option<Url> {
        self.0
            .shared_inbox_url
            .as_ref()
            .and_then(|u| u.parse().ok())
    }
}
