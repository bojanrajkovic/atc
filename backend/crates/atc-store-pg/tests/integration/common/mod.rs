//! Shared testcontainers helper for `atc-store-pg`'s own integration tests.
//!
//! Deliberately a slimmed-down copy of
//! `atc-server/tests/integration/common::start_pg` (a fresh uniquely-named
//! database per test on a single reused Postgres container), not a call
//! into a shared crate — the workspace's existing cross-crate test-helper
//! precedent is `atc-core`'s `test-support` feature (library code gated
//! behind a feature flag), which doesn't fit a `tests/`-only fixture
//! function like this one, and the full app-router fixture `start_pg`'s
//! caller also builds in `atc-server` isn't needed here; only a migrated
//! pool is.

use std::time::Duration;

pub async fn start_pg() -> (
    atc_store_pg::TracedPool,
    testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use testcontainers::ImageExt;
    use testcontainers::ReuseDirective;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    // Retry on container-creation race — see the identical comment in
    // atc-server's `common::start_pg` for the full rationale.
    let mut container_delay_ms: u64 = 50;
    let container = loop {
        match Postgres::default()
            .with_tag("17-alpine")
            // Distinct name from atc-server's own "atc-test-pg" reused
            // container — sharing one container across two crates' test
            // binaries would double the concurrent connection load on a
            // single Postgres instance when `cargo nextest run --workspace`
            // runs both binaries' tests in parallel.
            .with_container_name("atc-store-pg-test-pg")
            .with_reuse(ReuseDirective::Always)
            .start()
            .await
        {
            Ok(c) => break c,
            Err(e) if container_delay_ms < 4_000 => {
                tokio::time::sleep(Duration::from_millis(container_delay_ms)).await;
                container_delay_ms *= 2;
                eprintln!(
                    "[start_pg] container start retry after {container_delay_ms}ms (last error: {e})"
                );
            }
            Err(e) => panic!("failed to start postgres container after retries: {e}"),
        }
    };
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");

    use sqlx::Connection;
    let admin_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    #[allow(clippy::disallowed_methods)]
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let db_name = format!("test_{pid}_{nanos}_{counter}");
    let mut delay_ms: u64 = 50;
    let admin_conn = loop {
        match sqlx::PgConnection::connect(&admin_url).await {
            Ok(conn) => break conn,
            Err(e) if delay_ms < 4_000 => {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                eprintln!("[start_pg] admin connect retry after {delay_ms}ms (last error: {e})");
            }
            Err(e) => panic!("admin connect failed after retries: {e}"),
        }
    };
    {
        let mut admin_conn = admin_conn;
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
            .execute(&mut admin_conn)
            .await
            .expect("CREATE DATABASE failed");
    }

    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/{db_name}");
    let pool = atc_store_pg::init_pool(&db_url)
        .await
        .expect("init_pool failed");
    (pool, container)
}
