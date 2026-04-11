//! AC5: TTL eviction tests.

use super::*;
use chrono::TimeDelta;

#[tokio::test]
async fn test_ac5_1_completed_job_within_ttl_retained() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1700);
    let job_id = JobId(1701);

    // Create and complete a job at t0
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock to t0 + 30 minutes (within 1-hour TTL)
    clock.advance(TimeDelta::minutes(30));

    // Call evict_expired()
    store.evict_expired().await;

    // Verify job still exists
    let job = store.get_job(&job_id).await;
    assert!(job.is_some(), "Job should be retained within TTL");
}

#[tokio::test]
async fn test_ac5_2_completed_job_past_ttl_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1800);
    let job_id = JobId(1801);

    // Create and complete a job at t0
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock to t0 + 2 hours (past 1-hour TTL)
    clock.advance(TimeDelta::hours(2));

    // Call evict_expired()
    store.evict_expired().await;

    // Verify job is removed from primary map
    let job = store.get_job(&job_id).await;
    assert!(
        job.is_none(),
        "Completed job past TTL should be evicted from primary map"
    );

    // Verify job is removed from jobs_for_run
    let jobs = store.jobs_for_run(&run_id).await;
    assert!(
        !jobs.contains(&job_id),
        "Evicted job should not be in jobs_for_run"
    );

    // Verify job is removed from jobs_for_repo
    let repo = RepoKey::new("org", "repo");
    let jobs = store.jobs_for_repo(&repo).await;
    assert!(
        !jobs.contains(&job_id),
        "Evicted job should not be in jobs_for_repo"
    );
}

#[tokio::test]
async fn test_ac5_3_run_with_no_jobs_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1900);
    let job_id = JobId(1901);

    // Create a run with one job
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock past TTL and evict
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify both job and run are evicted
    let job = store.get_job(&job_id).await;
    assert!(job.is_none(), "Job should be evicted");
    let run = store.get_run(&run_id).await;
    assert!(
        run.is_none(),
        "Run with no remaining jobs should be evicted"
    );
}

#[tokio::test]
async fn test_ac5_3_run_with_active_job_retained() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(2000);
    let completed_job_id = JobId(2001);
    let active_job_id = JobId(2002);

    // Create a run with two jobs
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Complete one job
    let completed_envelope = make_job_event_with_completed_at(
        completed_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner.clone()),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(completed_envelope).await.unwrap();

    // Create an active (running) job
    let active_envelope = make_job_event(
        active_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(active_envelope).await.unwrap();

    // Advance clock past TTL and evict
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify completed job is evicted but run and active job remain
    let completed_job = store.get_job(&completed_job_id).await;
    assert!(
        completed_job.is_none(),
        "Completed job past TTL should be evicted"
    );

    let active_job = store.get_job(&active_job_id).await;
    assert!(active_job.is_some(), "Active job should be retained");

    let run = store.get_run(&run_id).await;
    assert!(
        run.is_some(),
        "Run should be retained because it still has an active job"
    );
}

#[tokio::test]
async fn test_ac5_4_active_jobs_never_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(2100);
    let queued_job_id = JobId(2101);
    let running_job_id = JobId(2102);
    let waiting_job_id = JobId(2103);

    // Create a run
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create queued job
    let queued_envelope = make_job_event(
        queued_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(queued_envelope).await.unwrap();

    // Create running job
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let running_envelope = make_job_event(
        running_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(running_envelope).await.unwrap();

    // Create waiting job
    let waiting_envelope = make_job_event(
        waiting_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(waiting_envelope).await.unwrap();

    // Advance clock well past TTL and evict
    clock.advance(TimeDelta::days(10));
    store.evict_expired().await;

    // Verify all active jobs are retained
    let queued_job = store.get_job(&queued_job_id).await;
    assert!(queued_job.is_some(), "Queued job should never be evicted");

    let running_job = store.get_job(&running_job_id).await;
    assert!(running_job.is_some(), "Running job should never be evicted");

    let waiting_job = store.get_job(&waiting_job_id).await;
    assert!(waiting_job.is_some(), "Waiting job should never be evicted");
}

#[tokio::test]
async fn test_ac5_5_ttl_configurable() {
    let start_time = Utc::now();
    let one_hour_clock = Arc::new(TestClock::new(start_time));
    let five_min_clock = Arc::new(TestClock::new(start_time));

    // Store with 1-hour TTL
    let store_one_hour = StateStore::new(one_hour_clock.clone(), Duration::from_secs(3600));
    // Store with 5-minute TTL
    let store_five_min = StateStore::new(five_min_clock.clone(), Duration::from_secs(300));

    let run_id_one_hour = RunId(2200);
    let job_id_one_hour = JobId(2201);
    let run_id_five_min = RunId(2300);
    let job_id_five_min = JobId(2301);

    // Setup store with 1-hour TTL
    let run_envelope_one_hour = make_run_event(run_id_one_hour, RunEvent::Requested);
    store_one_hour
        .apply_run_event(run_envelope_one_hour)
        .await
        .unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope_one_hour = make_job_event_with_completed_at(
        job_id_one_hour,
        run_id_one_hour,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner.clone()),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store_one_hour
        .apply_job_event(job_envelope_one_hour)
        .await
        .unwrap();

    // Setup store with 5-minute TTL
    let run_envelope_five_min = make_run_event(run_id_five_min, RunEvent::Requested);
    store_five_min
        .apply_run_event(run_envelope_five_min)
        .await
        .unwrap();

    let job_envelope_five_min = make_job_event_with_completed_at(
        job_id_five_min,
        run_id_five_min,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store_five_min
        .apply_job_event(job_envelope_five_min)
        .await
        .unwrap();

    // Advance both clocks to t0 + 30 minutes
    one_hour_clock.advance(TimeDelta::minutes(30));
    five_min_clock.advance(TimeDelta::minutes(30));

    // Evict from both stores
    store_one_hour.evict_expired().await;
    store_five_min.evict_expired().await;

    // Verify: 1-hour store retains job, 5-minute store evicts it
    let job_one_hour = store_one_hour.get_job(&job_id_one_hour).await;
    assert!(
        job_one_hour.is_some(),
        "Job in 1-hour store should be retained"
    );

    let job_five_min = store_five_min.get_job(&job_id_five_min).await;
    assert!(
        job_five_min.is_none(),
        "Job in 5-minute store should be evicted"
    );
}
