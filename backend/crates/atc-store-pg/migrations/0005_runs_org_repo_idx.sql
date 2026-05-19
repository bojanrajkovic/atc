-- Repository-scoping index for `read_snapshot_for_repos`.
--
-- Joins runs and jobs by (org, repo) keep PG mode's snapshot read aligned
-- with the in-memory secondary `jobs_by_repo` index. The runs table is the
-- only one keyed by (org, repo); jobs reach the same scope via their parent
-- run's id (PK index) once runs are filtered.
CREATE INDEX runs_org_repo_idx ON runs(org, repo);
