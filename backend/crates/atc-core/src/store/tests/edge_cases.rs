//! AC6.5: Out-of-order, duplicate, and unknown-ID event edge cases.

use super::*;
use chrono::TimeDelta;

#[tokio::test]
async fn test_ac6_5_out_of_order_job_before_run() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1);
    let job_id = JobId(1);

    // Send JobEvent::Completed before RunEvent::Requested
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: None,
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Then send RunEvent::Requested
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Verify job exists in completed state
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    // Verify run exists in queued state
    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Queued);

    // Verify indexes are consistent
    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_out_of_order_completed_before_queued() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(2);
    let job_id = JobId(2);

    // First send JobEvent::Completed
    let completed_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: None,
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(completed_envelope).await.unwrap();

    // Then try to send JobEvent::Queued for same job
    let queued_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    let result = store.apply_job_event(queued_envelope).await;

    // Second event should return error (backward transition)
    assert!(result.is_err(), "Backward transition should be rejected");

    // Job should still be completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_duplicate_queued_events() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(3);
    let job_id = JobId(3);

    // Send JobEvent::Queued twice
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );

    store.apply_job_event(envelope.clone()).await.unwrap();
    let result = store.apply_job_event(envelope).await;

    // Second send should not error (idempotent)
    assert!(
        result.is_ok(),
        "Duplicate same-status event should be idempotent"
    );

    // Job should still be queued, and only appear once in indexes
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Queued);

    let jobs_for_run = store.jobs_for_run(&run_id).await;
    assert_eq!(
        jobs_for_run.len(),
        1,
        "Job should appear exactly once in jobs_by_run"
    );

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_duplicate_completed_events() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(4);
    let job_id = JobId(4);

    // Send JobEvent::Completed twice
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: None,
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );

    store.apply_job_event(envelope.clone()).await.unwrap();
    let result = store.apply_job_event(envelope).await;

    // Second send should not error (idempotent)
    assert!(
        result.is_ok(),
        "Duplicate completed event should be idempotent"
    );

    // Job should still be completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_unknown_run_id_on_job() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(99); // No run event for this ID
    let job_id = JobId(5);

    // Send JobEvent::Queued with run_id that has no corresponding RunEvent
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    // Job should be created successfully with the unknown run_id
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.run_id, run_id);
    assert_eq!(job.status, JobStatus::Queued);

    // Verify indexes are consistent (job is in jobs_by_run even though run doesn't exist)
    let jobs_for_run = store.jobs_for_run(&run_id).await;
    assert!(jobs_for_run.contains(&job_id));

    // Run itself should not exist
    let run = store.get_run(&run_id).await;
    assert!(
        run.is_none(),
        "Run should not exist if no RunEvent was sent"
    );

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_rapid_status_cycling() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(5);
    let job_id = JobId(6);

    // Send Queued
    let queued = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(queued).await.unwrap();

    // Send InProgress
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let in_progress = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: Some(runner.clone()),
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(in_progress).await.unwrap();

    // Send Completed
    let completed = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(completed).await.unwrap();

    // Try to send Queued again
    let queued_again = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    let result = store.apply_job_event(queued_again).await;

    // Should be rejected
    assert!(result.is_err(), "Cannot go from Completed back to Queued");

    // Job should remain completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_interleaved_multi_job() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run1_id = RunId(10);
    let run2_id = RunId(11);

    // Create two runs
    store
        .apply_run_event(make_run_event(run1_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(run2_id, RunEvent::Requested))
        .await
        .unwrap();
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Add jobs 1-3 to run 1
    for i in 1..=3 {
        let job_id = JobId(i * 100);
        let queued = make_job_event(
            job_id,
            run1_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued).await.unwrap();

        let in_progress = make_job_event(
            job_id,
            run1_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner: Some(runner.clone()),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(in_progress).await.unwrap();
    }

    // Add jobs 4-5 to run 2
    for i in 4..=5 {
        let job_id = JobId(i * 100);
        let queued = make_job_event(
            job_id,
            run2_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued).await.unwrap();
    }

    // Verify both runs' indexes are correct and no cross-contamination
    let run1_jobs = store.jobs_for_run(&run1_id).await;
    assert_eq!(run1_jobs.len(), 3);
    for i in 1..=3 {
        assert!(run1_jobs.contains(&JobId(i * 100)));
    }

    let run2_jobs = store.jobs_for_run(&run2_id).await;
    assert_eq!(run2_jobs.len(), 2);
    for i in 4..=5 {
        assert!(run2_jobs.contains(&JobId(i * 100)));
    }

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_eviction_with_mixed_state() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(20);

    // Create run
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Create some completed jobs
    for i in 1..=3 {
        let job_id = JobId(i);
        let completed_envelope = make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner.clone()),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(completed_envelope).await.unwrap();
    }

    // Create some active jobs
    for i in 4..=6 {
        let job_id = JobId(i);
        let queued_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued_envelope).await.unwrap();
    }

    // Advance time past TTL
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify: completed jobs (1-3) evicted, active jobs (4-6) retained
    for i in 1..=3 {
        let job = store.get_job(&JobId(i)).await;
        assert!(job.is_none(), "Completed job {i} should be evicted");
    }

    for i in 4..=6 {
        let job = store.get_job(&JobId(i)).await;
        assert!(job.is_some(), "Active job {i} should be retained");
    }

    // Verify indexes are consistent
    store.assert_invariants().await;
}
