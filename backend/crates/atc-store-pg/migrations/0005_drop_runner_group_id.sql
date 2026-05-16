-- Drops the `runner_group_id` column from `jobs`. The field was captured from
-- GitHub webhook payloads but never consumed in any production code path —
-- the only logical branch that read it (a frontend elasticity heuristic) was
-- retired in #176 in favor of operator-declared `capacity: null`.
--
-- This is an atomic forward-only schema change. Operators running a
-- multi-replica deploy will see a brief window during rollout where old
-- replicas issue `SELECT ... runner_group_id ...` against the post-migration
-- schema and surface a "column does not exist" error on `/v1/state`. The
-- rolling-deploy strategy replaces those replicas; the window closes when
-- the last old replica is drained.

ALTER TABLE jobs DROP COLUMN runner_group_id;
