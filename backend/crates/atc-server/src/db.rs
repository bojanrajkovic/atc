use sqlx::PgPool;

/// Connects to PostgreSQL and runs embedded migrations.
///
/// Callers (i.e. main) are responsible for interpreting errors — connecting and
/// migrating are separated in logging terms but unified here so the path is
/// exercisable as a library function in integration tests.
pub async fn init_pool(
    database_url: &str,
) -> Result<PgPool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = PgPool::connect(database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
