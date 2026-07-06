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
//!
//! **Never `stop()` or `rm()` the returned container** — it is shared across
//! every concurrently running nextest test process, and `#[serial]` cannot
//! protect it (in-process lock; nextest runs one process per test). A test
//! that needs an unreachable database must boot its own private, unnamed,
//! non-reused container.

use std::time::Duration;

/// Age past which a `test_*` database is assumed to be an abandoned leftover
/// from a prior `cargo nextest run`, not a sibling of the current one. See
/// `reap_stale_test_databases`'s doc comment for the full rationale
/// (identical logic to `atc-server`'s `common::reap_stale_test_databases`).
const STALE_TEST_DB_AGE_NANOS: u64 = 60 * 60 * 1_000_000_000;

/// Drop `test_*` databases from prior runs older than
/// [`STALE_TEST_DB_AGE_NANOS`] -- `start_pg` creates one database per test
/// call and never drops it, so left unchecked these accumulate without
/// bound in the long-lived reused container. Best-effort: a failed drop is
/// logged and skipped, never panics.
async fn reap_stale_test_databases(admin_conn: &mut sqlx::PgConnection, now_nanos: u64) {
    use sqlx::Row;

    let rows = match sqlx::query(
        "SELECT datname FROM pg_database WHERE datname ~ '^test_[0-9]+_[0-9]+_[0-9]+$'",
    )
    .fetch_all(&mut *admin_conn)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("[start_pg] reap: failed to list test databases: {e}");
            return;
        }
    };

    for row in rows {
        let datname: String = row.get("datname");
        let Some(db_nanos) = datname
            .split('_')
            .nth(2)
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if now_nanos.saturating_sub(db_nanos) < STALE_TEST_DB_AGE_NANOS {
            continue;
        }
        if let Err(e) = sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{datname}\" WITH (FORCE)"
        ))
        .execute(&mut *admin_conn)
        .await
        {
            eprintln!("[start_pg] reap: failed to drop {datname}: {e}");
        }
    }
}

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
            .with_tag("18-alpine")
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
        reap_stale_test_databases(&mut admin_conn, nanos).await;
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
