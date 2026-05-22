-- Adds a nullable `completed_at` column to `runs`, mirroring the existing
-- column on `jobs`. Backfills already-completed rows from `updated_at` so
-- the next snapshot read after a rolling deploy can apply the display-TTL
-- cutoff against a populated value rather than treating every legacy row
-- as "no cutoff applies yet" (permissive — the WHERE clause keeps NULL
-- rows visible, so the absence of a backfill would only cause a slow
-- decay rather than incorrect filtering, but the backfill removes that
-- ambiguity).
--
-- A composite `(status, completed_at)` index keeps the display-TTL
-- WHERE clause cheap: snapshot reads at `/v1/state` filter on `status =
-- 'Completed' AND (completed_at IS NULL OR completed_at >= cutoff)`,
-- matching the existing `jobs_status_completed_at_idx` shape.
--
-- All three statements run inside the migration's transaction; the
-- ALTER + UPDATE + CREATE INDEX briefly hold write-blocking locks on the
-- `runs` table. Acceptable for the homelab deployment posture (small
-- table, brief redeploy window). Large deployments would split these
-- into separate transactions with `CREATE INDEX CONCURRENTLY` — see
-- ADR-0009 for the trade-off.

ALTER TABLE runs ADD COLUMN completed_at TIMESTAMPTZ NULL;

UPDATE runs
   SET completed_at = updated_at
 WHERE status = 'Completed'
   AND completed_at IS NULL;

CREATE INDEX runs_status_completed_at_idx
    ON runs (status, completed_at);
