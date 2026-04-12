use super::*;
use crate::job::RunnerInfo;
use chrono::Utc;
use proptest::prelude::*;

/// All possible test actions that can be applied to the store.
#[derive(Debug, Clone)]
enum TestAction {
    RequestRun(i64),
    StartRun(i64),
    CompleteRun(i64),
    QueueJob(i64, i64),
    WaitJob(i64, i64),     // (run_id, job_id)
    StartJob(i64, i64),    // (run_id, job_id)
    CompleteJob(i64, i64), // (run_id, job_id)
    AdvanceTimeAndEvict,
}

/// Generate a strategy for test actions.
fn test_action_strategy() -> impl Strategy<Value = TestAction> {
    prop_oneof![
        (1i64..=3).prop_map(TestAction::RequestRun),
        (1i64..=3).prop_map(TestAction::StartRun),
        (1i64..=3).prop_map(TestAction::CompleteRun),
        (1i64..=3, 1i64..=10).prop_map(|(run_id, job_id)| TestAction::QueueJob(run_id, job_id)),
        (1i64..=3, 1i64..=10).prop_map(|(run_id, job_id)| TestAction::WaitJob(run_id, job_id)),
        (1i64..=3, 1i64..=10).prop_map(|(run_id, job_id)| TestAction::StartJob(run_id, job_id)),
        (1i64..=3, 1i64..=10).prop_map(|(run_id, job_id)| TestAction::CompleteJob(run_id, job_id)),
        Just(TestAction::AdvanceTimeAndEvict),
    ]
}

/// Create a `RunEventEnvelope` with standard test defaults.
fn make_run_envelope(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = Utc::now();
    RunEventEnvelope {
        run_id,
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        workflow_name: Some("test-workflow".to_string()),
        workflow_path: Some(".github/workflows/test.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("test commit".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Test Run".to_string(),
        html_url: "https://example.com/run".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        action,
    }
}

/// Apply a test action to the store, silently ignoring errors.
#[allow(clippy::too_many_lines)]
async fn apply_action(
    store: &StateStore,
    clock: &Arc<crate::clock::TestClock>,
    action: &TestAction,
) {
    match action {
        TestAction::RequestRun(run_id) => {
            let run_id = RunId(*run_id);
            let envelope = make_run_envelope(run_id, RunEvent::Requested);
            let _ = store.apply_run_event(envelope).await;
        }
        TestAction::StartRun(run_id) => {
            let run_id = RunId(*run_id);
            let envelope = make_run_envelope(run_id, RunEvent::InProgress);
            let _ = store.apply_run_event(envelope).await;
        }
        TestAction::CompleteRun(run_id) => {
            let run_id = RunId(*run_id);
            let envelope = make_run_envelope(
                run_id,
                RunEvent::Completed {
                    conclusion: crate::run::RunConclusion::Success,
                },
            );
            let _ = store.apply_run_event(envelope).await;
        }
        TestAction::QueueJob(run_id, job_id) => {
            let run_id = RunId(*run_id);
            let job_id = JobId(*job_id);
            let now = Utc::now();
            let envelope = JobEventEnvelope {
                job_id,
                run_id,
                org: "test-org".to_string(),
                repo: "test-repo".to_string(),
                name: "test-job".to_string(),
                created_at: now,
                started_at: None,
                completed_at: None,
                action: JobEvent::Queued {
                    labels: vec!["linux".to_string()],
                    steps: vec![],
                },
            };
            let _ = store.apply_job_event(envelope).await;
        }
        TestAction::StartJob(run_id, job_id) => {
            let run_id = RunId(*run_id);
            let job_id = JobId(*job_id);
            let now = Utc::now();
            let runner = RunnerInfo {
                id: 1,
                name: "runner-1".to_string(),
                group_id: None,
                group_name: None,
            };
            let envelope = JobEventEnvelope {
                job_id,
                run_id,
                org: "test-org".to_string(),
                repo: "test-repo".to_string(),
                name: "test-job".to_string(),
                created_at: now,
                started_at: None,
                completed_at: None,
                action: JobEvent::InProgress {
                    runner: Some(runner),
                    labels: vec!["linux".to_string()],
                    steps: vec![],
                },
            };
            let _ = store.apply_job_event(envelope).await;
        }
        TestAction::WaitJob(run_id, job_id) => {
            let run_id = RunId(*run_id);
            let job_id = JobId(*job_id);
            let now = Utc::now();
            let envelope = JobEventEnvelope {
                job_id,
                run_id,
                org: "test-org".to_string(),
                repo: "test-repo".to_string(),
                name: "test-job".to_string(),
                created_at: now,
                started_at: None,
                completed_at: None,
                action: JobEvent::Waiting {
                    labels: vec![],
                    steps: vec![],
                },
            };
            let _ = store.apply_job_event(envelope).await;
        }
        TestAction::CompleteJob(run_id, job_id) => {
            let run_id = RunId(*run_id);
            let job_id = JobId(*job_id);
            let now = Utc::now();
            let runner = RunnerInfo {
                id: 1,
                name: "runner-1".to_string(),
                group_id: None,
                group_name: None,
            };
            let envelope = JobEventEnvelope {
                job_id,
                run_id,
                org: "test-org".to_string(),
                repo: "test-repo".to_string(),
                name: "test-job".to_string(),
                created_at: now,
                started_at: None,
                completed_at: Some(now),
                action: JobEvent::Completed {
                    conclusion: crate::job::JobConclusion::Success,
                    runner: Some(runner),
                    labels: vec!["linux".to_string()],
                    steps: vec![],
                },
            };
            let _ = store.apply_job_event(envelope).await;
        }
        TestAction::AdvanceTimeAndEvict => {
            clock.advance(chrono::TimeDelta::hours(2));
            store.evict_expired().await;
        }
    }
}

proptest! {
    #[test]
    fn store_invariants_hold(
        actions in prop::collection::vec(test_action_strategy(), 10..100)
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let clock = Arc::new(crate::clock::TestClock::new(Utc::now()));
            let store = StateStore::new(
                clock.clone(),
                Duration::from_secs(3600),
            );

            for action in &actions {
                // Apply action, ignore errors (invalid transitions expected)
                apply_action(&store, &clock, action).await;
            }

            // After all actions, invariants must hold
            store.assert_invariants().await;
        });
    }
}
