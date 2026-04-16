//! AC4 (partial): Runner pool stats derivation tests.

use super::*;

#[tokio::test]
async fn test_ac4_3_basic_pool_counts() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1200);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create 2 queued jobs with labels ["linux", "self-hosted"]
    let labels = vec!["linux".to_string(), "self-hosted".to_string()];
    let job_id_1 = JobId(1201);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    let job_id_2 = JobId(1202);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Create 1 running job with same labels
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_id_3 = JobId(1203);
    let envelope_3 = make_job_event(
        job_id_3,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_3).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify one entry with correct counts
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 2);
    assert_eq!(stats[0].running, 1);
    assert!(!stats[0].is_elastic);
    assert_eq!(stats[0].total, None);
}

#[tokio::test]
async fn test_ac4_3_multiple_pools() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1300);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create queued jobs with labels ["linux"]
    let job_id_1 = JobId(1301);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create queued jobs with labels ["macos"]
    let job_id_2 = JobId(1302);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["macos".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify two entries
    assert_eq!(stats.len(), 2);
    let mut counts: Vec<(usize, usize)> = stats.iter().map(|s| (s.queued, s.running)).collect();
    counts.sort_unstable();
    assert_eq!(counts, vec![(1, 0), (1, 0)]);
    for stat in &stats {
        assert!(!stat.is_elastic);
        assert_eq!(stat.total, None);
    }
}

#[tokio::test]
async fn test_ac4_3_label_normalization() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1400);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create job with ["self-hosted", "linux"]
    let job_id_1 = JobId(1401);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["self-hosted".to_string(), "linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create job with ["linux", "self-hosted"] (different order)
    let job_id_2 = JobId(1402);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string(), "self-hosted".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify single entry (same label set regardless of order)
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 2);
    assert_eq!(stats[0].running, 0);
    assert!(!stats[0].is_elastic);
    assert_eq!(stats[0].total, None);
}

#[tokio::test]
async fn test_ac4_3_excludes_completed() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1500);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let labels = vec!["linux".to_string()];

    // Create a queued job
    let job_id_1 = JobId(1501);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create a completed job
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_id_2 = JobId(1502);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify completed job is not counted
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 1);
    assert_eq!(stats[0].running, 0);
    assert!(!stats[0].is_elastic);
    assert_eq!(stats[0].total, None);
}

#[tokio::test]
async fn test_ac4_4_group_name_from_runner_info() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1600);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create a running job with group_name
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: Some(42),
        group_name: Some("default".to_string()),
    };
    let job_id = JobId(1601);
    let labels = vec!["linux".to_string()];
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels,
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify group_name is captured
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].group_name, Some("default".to_string()));
    assert!(!stats[0].is_elastic);
    assert_eq!(stats[0].total, None);
}

#[tokio::test]
async fn test_ac3_3_elastic_pool_from_github_hosted_runner() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(2000);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create a running job with GitHub-hosted runner (group_id: Some(0))
    let runner = RunnerInfo {
        id: 1,
        name: "github-runner-1".to_string(),
        group_id: Some(0),
        group_name: Some("GitHub Actions".to_string()),
    };
    let job_id = JobId(2001);
    let labels = vec!["ubuntu-latest".to_string()];
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels,
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify is_elastic is true for GitHub-hosted runner (group_id == 0)
    assert_eq!(stats.len(), 1);
    assert!(stats[0].is_elastic);
    assert_eq!(stats[0].total, None);
}

#[tokio::test]
async fn test_ac3_3_elastic_sticky_with_queued_jobs() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(2100);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create a queued job with labels ["ubuntu-latest"]
    let job_id_queued = JobId(2101);
    let labels = vec!["ubuntu-latest".to_string()];
    let envelope_queued = make_job_event(
        job_id_queued,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_queued).await.unwrap();

    // Create an InProgress job with same labels and GitHub-hosted runner
    let runner = RunnerInfo {
        id: 2,
        name: "github-runner-2".to_string(),
        group_id: Some(0),
        group_name: Some("GitHub Actions".to_string()),
    };
    let job_id_running = JobId(2102);
    let envelope_running = make_job_event(
        job_id_running,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner),
            labels,
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_running).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify single pool with sticky is_elastic=true, queued=1, running=1
    assert_eq!(stats.len(), 1);
    assert!(stats[0].is_elastic);
    assert_eq!(stats[0].queued, 1);
    assert_eq!(stats[0].running, 1);
    assert_eq!(stats[0].total, None);
}
