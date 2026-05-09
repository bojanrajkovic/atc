//! Repository-scoped query tests.

use super::*;

#[tokio::test]
async fn test_query_returns_only_queried_repos() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(700);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create jobs in two different repos
    let job_id_alpha = JobId(701);
    let job_id_beta = JobId(702);

    let alpha_envelope = make_job_event(
        job_id_alpha,
        run_id,
        "org",
        "alpha",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(alpha_envelope).await.unwrap();

    let beta_envelope = make_job_event(
        job_id_beta,
        run_id,
        "org",
        "beta",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(beta_envelope).await.unwrap();

    // Query only alpha repo
    let alpha_repo = RepoKey::new("org", "alpha");
    let result = store.query_by_repos(&[alpha_repo]).await;

    // Verify only alpha's job is returned
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.jobs[0].id, job_id_alpha);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_query_returns_owned_snapshots() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));

    // Create a run and job
    let run_id = RunId(800);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let job_id = JobId(801);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query and hold the result
    let repo = RepoKey::new("org", "repo");
    let result = store.query_by_repos(&[repo]).await;

    // Verify we can use the result without holding the store lock
    // (This is implicitly tested by the API returning owned types,
    // but we verify the data is present and usable)
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.jobs[0].id, job_id);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_empty_repos_returns_empty_result() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));

    // Create a run and job
    let run_id = RunId(900);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let job_id = JobId(901);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query with empty repos slice
    let result = store.query_by_repos(&[]).await;

    // Verify empty result
    assert_eq!(result.jobs.len(), 0);
    assert_eq!(result.runs.len(), 0);
}

#[tokio::test]
async fn test_multi_repo_query_isolation() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(1000);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create jobs in repo A
    let job_alpha_1 = JobId(1001);
    let job_alpha_2 = JobId(1002);

    let envelope_alpha_1 = make_job_event(
        job_alpha_1,
        run_id,
        "org",
        "repoA",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_alpha_1).await.unwrap();

    let envelope_alpha_2 = make_job_event(
        job_alpha_2,
        run_id,
        "org",
        "repoA",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_alpha_2).await.unwrap();

    // Create jobs in repo B
    let job_beta_1 = JobId(1003);
    let job_beta_2 = JobId(1004);

    let envelope_beta_1 = make_job_event(
        job_beta_1,
        run_id,
        "org",
        "repoB",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_beta_1).await.unwrap();

    let envelope_beta_2 = make_job_event(
        job_beta_2,
        run_id,
        "org",
        "repoB",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_beta_2).await.unwrap();

    // Query both repos
    let repo_a = RepoKey::new("org", "repoA");
    let repo_b = RepoKey::new("org", "repoB");
    let result = store
        .query_by_repos(&[repo_a.clone(), repo_b.clone()])
        .await;

    // Verify both repos' jobs are returned (4 jobs total)
    assert_eq!(result.jobs.len(), 4);

    // Verify all expected job IDs are present
    let job_ids: HashSet<JobId> = result.jobs.iter().map(|job| job.id).collect();
    assert!(job_ids.contains(&job_alpha_1), "Job A1 should be in result");
    assert!(job_ids.contains(&job_alpha_2), "Job A2 should be in result");
    assert!(job_ids.contains(&job_beta_1), "Job B1 should be in result");
    assert!(job_ids.contains(&job_beta_2), "Job B2 should be in result");

    // Verify repo A query alone returns only repo A's jobs
    let result_a = store.query_by_repos(&[repo_a]).await;
    assert_eq!(result_a.jobs.len(), 2);
    let job_ids_a: HashSet<JobId> = result_a.jobs.iter().map(|job| job.id).collect();
    assert!(job_ids_a.contains(&job_alpha_1));
    assert!(job_ids_a.contains(&job_alpha_2));
    assert!(!job_ids_a.contains(&job_beta_1));
    assert!(!job_ids_a.contains(&job_beta_2));

    // Verify repo B query alone returns only repo B's jobs
    let result_b = store.query_by_repos(&[repo_b]).await;
    assert_eq!(result_b.jobs.len(), 2);
    let job_ids_b: HashSet<JobId> = result_b.jobs.iter().map(|job| job.id).collect();
    assert!(!job_ids_b.contains(&job_alpha_1));
    assert!(!job_ids_b.contains(&job_alpha_2));
    assert!(job_ids_b.contains(&job_beta_1));
    assert!(job_ids_b.contains(&job_beta_2));

    // Verify run is included
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_query_includes_parent_runs() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(1100);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Add a job to the run
    let job_id = JobId(1101);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query by repo
    let repo = RepoKey::new("org", "repo");
    let result = store.query_by_repos(&[repo]).await;

    // Verify both job and run are present
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.jobs[0].run_id, run_id);
    assert_eq!(result.runs[0].id, run_id);
}
