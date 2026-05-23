use serde::{Deserialize, Serialize};
use url::Url;

pub const AP_CONTEXT: &[&str] = &[
    "https://www.w3.org/ns/activitystreams",
    "https://w3id.org/security/v1",
];

// ── Type markers ────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum NoteKind {
    #[serde(rename = "Note")]
    Note,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PersonKind {
    #[serde(rename = "Person")]
    Person,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GroupKind {
    #[serde(rename = "Group")]
    Group,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CreateKind {
    #[serde(rename = "Create")]
    Create,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UpdateKind {
    #[serde(rename = "Update")]
    Update,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeleteKind {
    #[serde(rename = "Delete")]
    Delete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FollowKind {
    #[serde(rename = "Follow")]
    Follow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LikeKind {
    #[serde(rename = "Like")]
    Like,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DislikeKind {
    #[serde(rename = "Dislike")]
    Dislike,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnnounceKind {
    #[serde(rename = "Announce")]
    Announce,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AcceptKind {
    #[serde(rename = "Accept")]
    Accept,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UndoKind {
    #[serde(rename = "Undo")]
    Undo,
}

// ── Actor types ─────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Person {
    #[serde(rename = "type")]
    pub kind: PersonKind,
    pub id: Url,
    pub url: Option<Url>,
    #[serde(rename = "preferredUsername")]
    pub preferred_username: Option<String>,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub icon: Option<Url>,
    pub inbox: Url,
    pub outbox: Option<Url>,
    pub followers: Option<Url>,
    pub following: Option<Url>,
    #[serde(rename = "publicKey")]
    pub public_key: Option<serde_json::Value>,
    #[serde(rename = "manuallyApprovesFollowers")]
    pub manually_approves_followers: Option<bool>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    #[serde(rename = "type")]
    pub kind: GroupKind,
    pub id: Url,
    #[serde(rename = "preferredUsername")]
    pub preferred_username: Option<String>,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub inbox: Url,
    pub outbox: Option<Url>,
    pub followers: Option<Url>,
    #[serde(rename = "publicKey")]
    pub public_key: Option<serde_json::Value>,
    pub published: Option<String>,
    #[serde(rename = "attributedTo")]
    pub attributed_to: Option<Url>,
    #[serde(rename = "postingRestrictedToMods")]
    pub posting_restricted_to_mods: Option<bool>,
    pub sensitive: Option<bool>,
}

// ── Object (Note) type ──────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    #[serde(rename = "type")]
    pub kind: NoteKind,
    pub id: Url,
    #[serde(rename = "attributedTo")]
    pub attributed_to: Url,
    pub to: Vec<Url>,
    pub cc: Vec<Url>,
    pub content: Option<String>,
    pub name: Option<String>,
    pub url: Option<Url>,
    #[serde(rename = "inReplyTo")]
    pub in_reply_to: Option<Url>,
    pub published: Option<String>,
    #[serde(rename = "mediaType")]
    pub media_type: Option<String>,
}

// ── Activity types ──────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Create {
    #[serde(rename = "type")]
    pub kind: CreateKind,
    pub id: Url,
    pub actor: Url,
    pub object: Note,
    pub to: Vec<Url>,
    pub cc: Vec<Url>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Update {
    #[serde(rename = "type")]
    pub kind: UpdateKind,
    pub id: Url,
    pub actor: Url,
    pub object: Note,
    pub to: Vec<Url>,
    pub cc: Vec<Url>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delete {
    #[serde(rename = "type")]
    pub kind: DeleteKind,
    pub id: Url,
    pub actor: Url,
    pub object: Url,
    pub to: Vec<Url>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Follow {
    #[serde(rename = "type")]
    pub kind: FollowKind,
    pub id: Url,
    pub actor: Url,
    pub object: Url,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcceptFollow {
    #[serde(rename = "type")]
    pub kind: AcceptKind,
    pub id: Url,
    pub actor: Url,
    pub object: Follow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Like {
    #[serde(rename = "type")]
    pub kind: LikeKind,
    pub id: Url,
    pub actor: Url,
    pub object: Url,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dislike {
    #[serde(rename = "type")]
    pub kind: DislikeKind,
    pub id: Url,
    pub actor: Url,
    pub object: Url,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Announce {
    #[serde(rename = "type")]
    pub kind: AnnounceKind,
    pub id: Url,
    pub actor: Url,
    pub object: serde_json::Value,
    pub to: Vec<Url>,
    pub cc: Vec<Url>,
    pub published: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UndoFollow {
    #[serde(rename = "type")]
    pub kind: UndoKind,
    pub id: Url,
    pub actor: Url,
    pub object: Follow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UndoLike {
    #[serde(rename = "type")]
    pub kind: UndoKind,
    pub id: Url,
    pub actor: Url,
    pub object: Like,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UndoDislike {
    #[serde(rename = "type")]
    pub kind: UndoKind,
    pub id: Url,
    pub actor: Url,
    pub object: Dislike,
}

// ── Dispatch enum ───────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TavernaActivity {
    CreateActivity(Create),
    UpdateActivity(Update),
    DeleteActivity(Delete),
    FollowActivity(Follow),
    AcceptFollowActivity(AcceptFollow),
    LikeActivity(Like),
    DislikeActivity(Dislike),
    AnnounceActivity(Announce),
    UndoFollowActivity(UndoFollow),
    UndoLikeActivity(UndoLike),
    UndoDislikeActivity(UndoDislike),
}

// ── Helper builders for outbound activities ─────────────────

pub fn build_create_id(base_url: &str, post_id: &uuid::Uuid) -> String {
    format!(
        "{}/activities/create/{}",
        base_url.trim_end_matches('/'),
        post_id
    )
}

pub fn build_announce_id(
    base_url: &str,
    community_id: &uuid::Uuid,
    post_id: &uuid::Uuid,
) -> String {
    format!(
        "{}/activities/announce/{}/{}",
        base_url.trim_end_matches('/'),
        community_id,
        post_id
    )
}

pub fn build_like_id(base_url: &str, user_id: &uuid::Uuid, post_id: &uuid::Uuid) -> String {
    format!(
        "{}/activities/like/{}/{}",
        base_url.trim_end_matches('/'),
        user_id,
        post_id
    )
}

pub fn build_accept_id(base_url: &str, follow_id: &uuid::Uuid) -> String {
    format!(
        "{}/activities/accept/{}",
        base_url.trim_end_matches('/'),
        follow_id
    )
}

pub fn build_delete_id(base_url: &str, post_id: &uuid::Uuid) -> String {
    format!(
        "{}/activities/delete/{}",
        base_url.trim_end_matches('/'),
        post_id
    )
}

pub fn build_undo_id(base_url: &str, original_id: &str) -> String {
    format!(
        "{}/activities/undo/{}",
        base_url.trim_end_matches('/'),
        original_id.trim_start_matches('/').replace('/', "_")
    )
}
