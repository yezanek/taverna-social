use activitypub_federation::config::Data;
use activitypub_federation::protocol::verification::verify_domains_match;
use activitypub_federation::traits::{Actor, Object};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use url::Url;

use crate::federation::protocol::{Group, Note, NoteKind, Person};
use crate::models::_entities::{comments, communities, posts, users};
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

    async fn from_json(json: Self::Kind, data: &Data<Self::DataType>) -> Result<Self, Self::Error> {
        let db = data.app_data();
        let id = json.id.as_str();

        if let Some(existing) = users::Entity::find()
            .filter(users::Column::ApId.eq(id))
            .one(db)
            .await?
        {
            return Ok(DbUser::new(existing)?);
        }

        // Create remote user
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
                anyhow::anyhow!("Missing or empty publicKeyPem for remote user {}", id)
            })?;

        if !public_key_raw.starts_with("-----BEGIN") {
            anyhow::bail!(
                "Invalid PEM format for remote user {}: does not start with -----BEGIN",
                id
            );
        }

        let public_key = public_key_raw.to_string();

        let new_user = users::ActiveModel {
            id: Set(uuid),
            username: Set(format!("remote_{}", &uuid.to_string()[..8])),
            email: Set(None),
            password_hash: Set(None),
            display_name: Set(json.name.clone()),
            bio: Set(json.summary.clone()),
            avatar_url: Set(json.icon.map(|u| u.to_string())),
            ap_id: Set(ap_id),
            public_key: Set(public_key),
            private_key: Set(None),
            is_local: Set(false),
            is_admin: Set(false),
            is_banned: Set(false),
            email_verified: Set(false),
            standing_score: Set(0),
            suspended_at: Set(None),
            inbox_url: Set(inbox_url),
            shared_inbox_url: Set(None),
            followers_url: Set(json
                .followers
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default()),
            last_fetched_at: Set(None),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let saved = new_user.insert(db).await?;
        Ok(DbUser::new(saved)?)
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

// ── Post (Note) wrapper ──────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DbPost(pub posts::Model, pub Url);

impl DbPost {
    pub fn new(model: posts::Model) -> Result<Self, url::ParseError> {
        let url = model.ap_id.parse()?;
        Ok(DbPost(model, url))
    }
}

#[async_trait::async_trait]
impl Object for DbPost {
    type DataType = DatabaseConnection;
    type Kind = Note;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.1
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let db = data.app_data();
        let post = posts::Entity::find()
            .filter(posts::Column::ApId.eq(object_id.as_str()))
            .one(db)
            .await?;
        match post {
            Some(p) => Ok(Some(DbPost::new(p)?)),
            None => Ok(None),
        }
    }

    async fn into_json(self, data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        let db = data.app_data();
        let author = users::Entity::find_by_id(self.0.author_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Author not found for post {}", self.0.id))?;

        Ok(Note {
            kind: NoteKind::Note,
            id: self.1.clone(),
            attributed_to: author.ap_id.parse()?,
            to: vec!["https://www.w3.org/ns/activitystreams#Public".parse()?],
            cc: vec![],
            content: self.0.body.clone(),
            name: Some(self.0.title.clone()),
            url: self.0.url.as_ref().and_then(|u| u.parse().ok()),
            in_reply_to: None,
            published: Some(self.0.published_at.format("%+").to_string()),
            media_type: None,
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

        if let Some(existing) = posts::Entity::find()
            .filter(posts::Column::ApId.eq(id))
            .one(db)
            .await?
        {
            return Ok(DbPost::new(existing)?);
        }

        let community_id = {
            let mut found = None;
            for to in &json.to {
                if let Ok(Some(c)) = communities::Entity::find()
                    .filter(communities::Column::FollowersUrl.eq(to.as_str()))
                    .one(db)
                    .await
                {
                    found = Some(c.id);
                    break;
                }
            }
            found
        };

        let author_ap_id = json.attributed_to.as_str();
        let author = crate::federation::activities::ensure_remote_user(db, author_ap_id).await?;

        let title = json.name.as_deref().unwrap_or("Untitled");
        let content = json.content.as_deref().unwrap_or("");

        let new_post = posts::ActiveModel {
            id: Set(uuid::Uuid::new_v4()),
            ap_id: Set(id.to_string()),
            community_id: Set(community_id),
            author_id: Set(author.id),
            title: Set(title.to_string()),
            body: Set(if content.is_empty() {
                None
            } else {
                Some(content.to_string())
            }),
            url: Set(json.url.map(|u| u.to_string())),
            score: Set(0),
            comment_count: Set(0),
            is_deleted: Set(false),
            is_removed: Set(false),
            is_locked: Set(false),
            is_pinned: Set(false),
            published_at: Set(chrono::Utc::now().into()),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let saved = new_post.insert(db).await?;
        Ok(DbPost::new(saved)?)
    }
}

// ── Comment (Note) wrapper ───────────────────────────────────

#[derive(Clone, Debug)]
pub struct DbComment(pub comments::Model, pub Url);

impl DbComment {
    pub fn new(model: comments::Model) -> Result<Self, url::ParseError> {
        let url = model.ap_id.parse()?;
        Ok(DbComment(model, url))
    }
}

#[async_trait::async_trait]
impl Object for DbComment {
    type DataType = DatabaseConnection;
    type Kind = Note;
    type Error = anyhow::Error;

    fn id(&self) -> &Url {
        &self.1
    }

    async fn read_from_id(
        object_id: Url,
        data: &Data<Self::DataType>,
    ) -> Result<Option<Self>, Self::Error> {
        let db = data.app_data();
        let comment = comments::Entity::find()
            .filter(comments::Column::ApId.eq(object_id.as_str()))
            .one(db)
            .await?;
        match comment {
            Some(c) => Ok(Some(DbComment::new(c)?)),
            None => Ok(None),
        }
    }

    async fn into_json(self, data: &Data<Self::DataType>) -> Result<Self::Kind, Self::Error> {
        let db = data.app_data();
        let author = users::Entity::find_by_id(self.0.author_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Author not found for comment {}", self.0.id))?;
        let post = posts::Entity::find_by_id(self.0.post_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Post not found for comment {}", self.0.id))?;

        Ok(Note {
            kind: NoteKind::Note,
            id: self.1.clone(),
            attributed_to: author.ap_id.parse()?,
            to: vec!["https://www.w3.org/ns/activitystreams#Public".parse()?],
            cc: vec![],
            content: Some(self.0.body.clone()),
            name: None,
            url: Some(self.1.clone()),
            in_reply_to: Some(post.ap_id.parse()?),
            published: Some(self.0.published_at.format("%+").to_string()),
            media_type: None,
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

        if let Some(existing) = comments::Entity::find()
            .filter(comments::Column::ApId.eq(id))
            .one(db)
            .await?
        {
            return Ok(DbComment::new(existing)?);
        }

        let in_reply_to = json
            .in_reply_to
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Comment without inReplyTo"))?;

        let parent_post = posts::Entity::find()
            .filter(posts::Column::ApId.eq(in_reply_to.as_str()))
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Parent post not found: {}", in_reply_to))?;

        // Check if in_reply_to points to another comment (vs. directly to the post)
        let parent_comment = comments::Entity::find()
            .filter(comments::Column::ApId.eq(in_reply_to.as_str()))
            .one(db)
            .await?;

        let author_ap_id = json.attributed_to.as_str();
        let author = crate::federation::activities::ensure_remote_user(db, author_ap_id).await?;

        let content = json.content.as_deref().unwrap_or("");
        let comment_id = uuid::Uuid::new_v4();
        let comment_seg = format!("c{}", &comment_id.to_string().replace('-', "")[..8]);

        let (parent_id, path) = if let Some(ref pc) = parent_comment {
            let path = format!("{}.{}", pc.path, comment_seg);
            (Set(Some(pc.id)), Set(path))
        } else {
            let post_seg = format!("c{}", &parent_post.id.to_string().replace('-', "")[..8]);
            let path = format!("{}.{}", post_seg, comment_seg);
            (Set(None), Set(path))
        };

        let new_comment = comments::ActiveModel {
            id: Set(comment_id),
            ap_id: Set(id.to_string()),
            post_id: Set(parent_post.id),
            author_id: Set(author.id),
            parent_id,
            path,
            body: Set(content.to_string()),
            score: Set(0),
            is_deleted: Set(false),
            is_removed: Set(false),
            published_at: Set(chrono::Utc::now().into()),
            created_at: Set(chrono::Utc::now().into()),
            updated_at: Set(chrono::Utc::now().into()),
        };

        let saved = new_comment.insert(db).await?;
        crate::models::posts::PostModel::increment_comment_count(db, parent_post.id).await?;
        Ok(DbComment::new(saved)?)
    }
}
