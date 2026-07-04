-- Adds a nullable repo_id column to `runs`, carrying GitHub's immutable
-- numeric repository identifier -- the authorization key the native-auth
-- initiative filters on (ADR-0014). No backfill: a legacy NULL row
-- self-heals on the next run-event UPSERT (COALESCE preserves an existing
-- value; a NULL EXCLUDED value never overwrites a known one). No index:
-- per-repo filtering happens in-memory in the request handler after the
-- snapshot read, never in a SQL WHERE clause.

ALTER TABLE runs ADD COLUMN repo_id BIGINT;
