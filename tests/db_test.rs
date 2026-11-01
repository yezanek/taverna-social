mod common;

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;

    #[tokio::test]
    async fn test_db_connection() {
        let boot = crate::common::shared_boot().await;
        let db = &boot.app_context.db;
        let result = sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1".to_string(),
        );
        db.execute_raw(result).await.unwrap();
    }
}
