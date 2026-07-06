//! Pool initialization and embedded-migration runner for the PG store.
//!
//! [`init_pool`] connects an [`sqlx::PgPool`] to the supplied URL and runs the
//! four embedded migrations under `migrations/` against it. Failures fan out
//! into [`DbInitError`] so callers can distinguish "migration ran but failed"
//! from "could not connect" without naming the `sqlx::Error` enum directly —
//! this is how `atc-server` keeps `sqlx` out of its production-source dep set
//! after the PG-store extraction (#169, ADR-0008).

use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use sqlx::ConnectOptions;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// The embedded migrator, anchored on the four SQL files under
/// `atc-store-pg/migrations/`. Exposed publicly so test fixtures can run
/// migrations against a caller-managed pool (e.g. one configured with
/// `PgPoolOptions::acquire_timeout(...)` for readyz-down tests) without
/// re-deriving the migrator path. `init_pool` uses this same migrator
/// internally.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Discriminated error surface for [`init_pool`].
///
/// `sqlx 0.8.6` defines `Error::Migrate(Box<MigrateError>)`, so the boxed
/// shape is preserved here. `Connect` covers every non-migration `sqlx::Error`
/// the pool's connect path can surface.
#[derive(Debug)]
pub enum DbInitError {
    /// Embedded-migration application failed. The boxed [`sqlx::migrate::MigrateError`]
    /// is preserved verbatim so `error.source()` exposes the inner cause for log
    /// formatters and operator runbooks.
    Migrate(Box<sqlx::migrate::MigrateError>),
    /// Pool connect (or any other non-migration error path inside
    /// [`sqlx::PgPool::connect`]) failed.
    Connect(sqlx::Error),
}

impl fmt::Display for DbInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbInitError::Migrate(e) => write!(f, "database migration failed: {e}"),
            DbInitError::Connect(e) => write!(f, "database connection failed: {e}"),
        }
    }
}

impl Error for DbInitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            DbInitError::Migrate(e) => Some(e.as_ref()),
            DbInitError::Connect(e) => Some(e),
        }
    }
}

/// Connect to PostgreSQL and run the embedded migrations.
///
/// On success returns a `PgPool` ready for the rest of `atc-store-pg` to use.
/// On failure returns a [`DbInitError`] whose discriminant tells the caller
/// whether migrations or the connect path was at fault.
///
/// `sqlx::migrate!("./migrations")` resolves the migration directory relative
/// to `CARGO_MANIFEST_DIR` at compile time, so this anchor binds to the four
/// SQL files co-located with this crate (`backend/crates/atc-store-pg/migrations/`).
pub async fn init_pool(database_url: &str) -> Result<crate::TracedPool, DbInitError> {
    let pool = PgPoolOptions::new()
        // Fail fast when Postgres is unreachable: sqlx's default 30s
        // acquire_timeout would stall every in-flight handler for 30s
        // before the transient-failure 503 path fires. 5s is generous for
        // a healthy in-cluster hop and short enough that an outage degrades
        // to prompt 503s instead of piled-up handlers.
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(connect_options(database_url)?)
        .await
        .map_err(DbInitError::Connect)?;
    MIGRATOR
        .run(&pool)
        .await
        .map_err(|e| DbInitError::Migrate(Box::new(e)))?;
    // Wrap once at startup; downstream consumers thread `crate::TracedPool`
    // (cloneable, transparently implements `sqlx::Executor`). Bind values
    // are physically inaccessible through the wrapper, so per-query spans
    // are safe to enable by default — see `crate::TracedPool` doc-comment.
    Ok(sqlx_tracing::Pool::from(pool))
}

/// Build the [`PgConnectOptions`] used by [`init_pool`].
///
/// Tuned for operator observability:
///
/// - `log_statements(Debug)` — every query emits a `tracing::event!(Level::DEBUG)`
///   with the SQL statement, bind values, and elapsed time. Visible when the
///   `logFilter` chart value is bumped to `debug` (or scoped to
///   `info,sqlx::query=debug`). This is sqlx's default but stated explicitly
///   so future contributors don't have to read the upstream defaults.
///
/// - `log_slow_statements(Warn, 200ms)` — any query taking longer than 200ms
///   emits at `WARN` level (visible under the default `info` filter). 200ms is
///   ~10x the expected outbox-write hot-path budget on a healthy CNPG cluster,
///   so a steady stream of WARN events here is an operational signal that
///   either the DB is overloaded or the query plan regressed.
///
/// Note: sqlx 0.8 emits these as `tracing::event!`s, not as spans. Application-
/// level spans for write-path observability live with the `upsert_*_in_txn` /
/// `insert_outbox_*_in_txn` helpers in `store/writes.rs`.
fn connect_options(database_url: &str) -> Result<PgConnectOptions, DbInitError> {
    let mut options = PgConnectOptions::from_str(database_url).map_err(DbInitError::Connect)?;
    options = options
        .log_statements(tracing::log::LevelFilter::Debug)
        .log_slow_statements(tracing::log::LevelFilter::Warn, Duration::from_millis(200));
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DbInitError::Migrate` must surface its inner `sqlx::migrate::MigrateError`
    /// via [`std::error::Error::source`] so log formatters and operator
    /// dashboards can reach the underlying migration failure without naming
    /// the variant directly.
    #[test]
    fn migrate_variant_exposes_inner_via_source() {
        // `MigrateError::VersionTooNew(latest, current)` is one of the
        // cheapest publicly-constructible variants — two `i64`s with no
        // runtime context. We pick (latest=42, current=7) so the rendered
        // string contains the version we assert on.
        let inner = sqlx::migrate::MigrateError::VersionTooNew(42, 7);
        let err = DbInitError::Migrate(Box::new(inner));

        let source = std::error::Error::source(&err)
            .expect("Migrate variant must expose its inner error via source()");
        let rendered = source.to_string();
        assert!(
            rendered.contains("42"),
            "source() should surface the inner MigrateError details; got {rendered:?}",
        );

        // Discriminator must be observable via the variant pattern — this is
        // how `atc-server::main` decides whether to log "migrations failed" vs
        // "connect failed".
        assert!(
            matches!(err, DbInitError::Migrate(_)),
            "Migrate variant must be matchable as DbInitError::Migrate(_)",
        );
        assert!(
            !matches!(err, DbInitError::Connect(_)),
            "Migrate variant must NOT match the Connect arm",
        );
    }

    /// `DbInitError::Connect` must surface its inner `sqlx::Error` via
    /// [`std::error::Error::source`] for the same operator-visibility reason.
    #[test]
    fn connect_variant_exposes_inner_via_source() {
        // `sqlx::Error::PoolTimedOut` is a no-payload variant — the cheapest
        // way to fabricate a `sqlx::Error` in a unit test.
        let inner = sqlx::Error::PoolTimedOut;
        let err = DbInitError::Connect(inner);

        let source = std::error::Error::source(&err)
            .expect("Connect variant must expose its inner error via source()");
        // The rendered string content is sqlx's responsibility; we just verify
        // the source is reachable and non-empty.
        assert!(
            !source.to_string().is_empty(),
            "source() should yield a non-empty rendering of the inner sqlx::Error",
        );

        assert!(
            matches!(err, DbInitError::Connect(_)),
            "Connect variant must be matchable as DbInitError::Connect(_)",
        );
        assert!(
            !matches!(err, DbInitError::Migrate(_)),
            "Connect variant must NOT match the Migrate arm",
        );
    }

    /// `Display` impls must mention the failure category — this is the string
    /// `main.rs` logs under `error.message = %e`. The inner-error contents come through
    /// `source()`, not `Display`, so we only check the leading label here.
    #[test]
    fn display_includes_failure_category() {
        let migrate_err =
            DbInitError::Migrate(Box::new(sqlx::migrate::MigrateError::VersionTooNew(7, 0)));
        let migrate_str = format!("{migrate_err}");
        assert!(
            migrate_str.starts_with("database migration failed"),
            "Migrate Display must lead with the category; got {migrate_str:?}",
        );

        let connect_err = DbInitError::Connect(sqlx::Error::PoolTimedOut);
        let connect_str = format!("{connect_err}");
        assert!(
            connect_str.starts_with("database connection failed"),
            "Connect Display must lead with the category; got {connect_str:?}",
        );
    }
}
