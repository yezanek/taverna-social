#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;

mod m001_extensions;
mod m002_users;
mod m003_communities;
mod m004_posts_comments;
mod m005_votes_follows;
mod m006_federation;
mod m007_bans;
mod m008_eligibility;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m001_extensions::Migration),
            Box::new(m002_users::Migration),
            Box::new(m003_communities::Migration),
            Box::new(m004_posts_comments::Migration),
            Box::new(m005_votes_follows::Migration),
            Box::new(m006_federation::Migration),
            Box::new(m007_bans::Migration),
            Box::new(m008_eligibility::Migration),
        ]
    }
}
