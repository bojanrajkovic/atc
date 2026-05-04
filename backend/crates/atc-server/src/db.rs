use sqlx::PgPool;

/// Connects to PostgreSQL and runs embedded migrations.
pub async fn init_pool(database_url: &str) -> sqlx::Result<PgPool> {
    let pool = PgPool::connect(database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
