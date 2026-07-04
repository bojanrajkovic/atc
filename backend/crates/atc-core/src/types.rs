//! Core newtypes and shared types for the ATC domain model.

use std::collections::BTreeSet;
use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Unique identifier for a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunId(pub i64);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a job within a workflow run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JobId(pub i64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// GitHub's immutable numeric repository identifier.
///
/// Unlike `RepoKey` (an `org/repo` display pair), this identity survives
/// repository renames and owner transfers — the authorization key for
/// per-repo filtering must be this, not the string pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepoId(pub i64);

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Repository identifier as an (org, repo) pair.
///
/// Used as the primary filter key for access-controlled queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RepoKey {
    /// The GitHub organization or user name.
    pub org: String,
    /// The repository name.
    pub repo: String,
}

impl RepoKey {
    /// Creates a new repository key.
    pub fn new(org: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            org: org.into(),
            repo: repo.into(),
        }
    }
}

impl fmt::Display for RepoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.org, self.repo)
    }
}

/// Normalized set of runner labels for grouping and comparison.
///
/// Wraps a [`BTreeSet<String>`] to provide deterministic ordering and
/// deduplication. Two label sets with the same labels in different
/// order compare as equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LabelSet(BTreeSet<String>);

impl Hash for LabelSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for label in &self.0 {
            label.hash(state);
        }
    }
}

impl LabelSet {
    /// Creates a new label set from an iterator of strings.
    ///
    /// Labels are normalized by sorting and deduplication via the
    /// underlying `BTreeSet`.
    pub fn new(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(labels.into_iter().map(Into::into).collect())
    }

    /// Returns an iterator over the labels in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Returns the number of unique labels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the label set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<S: Into<String>> FromIterator<S> for LabelSet {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::new(iter)
    }
}

/// Three-state representation of a runner pool's declared total capacity.
///
/// Encodes the three operator outcomes on a single field:
///
/// - `Bounded(n)` — operator declared an integer capacity.
/// - `Unbounded` — operator declared the pool with `capacity: null`.
/// - `Undeclared` — pool observed in webhook traffic but absent from the
///   operator's `runner_pools` config.
///
/// Adjacent tagging keeps the discriminator on a single field and works for
/// both payload-bearing and unit variants across serde + ts-rs (internal
/// tagging breaks on unit variants in ts-rs 12.x; external produces a less
/// ergonomic shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "value")]
#[ts(export)]
pub enum RunnerPoolTotal {
    /// Operator declared an integer ceiling for the pool.
    Bounded(u32),
    /// Operator declared the pool with `capacity: null` — no renderable ceiling.
    Unbounded,
    /// Pool observed in webhook traffic but absent from the operator's
    /// `runner_pools` config.
    Undeclared,
}

/// Derived runner pool statistics.
///
/// Computed on read from live job state — not stored separately.
/// Each entry represents a unique label set with aggregated counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunnerPoolStats {
    /// The set of labels identifying this pool.
    pub labels: LabelSet,
    /// Number of jobs queued for this label set.
    pub queued: usize,
    /// Number of jobs running on runners with this label set.
    pub running: usize,
    /// Runner group name from the most recently observed `RunnerInfo`
    /// for this label set, if available.
    pub group_name: Option<String>,
    /// Three-state declared total for this pool. Populated by the frontend
    /// by merging operator-declared `RunnerPoolCapacity` entries from the
    /// snapshot rail against the derived label set.
    pub total: RunnerPoolTotal,
}

/// Operator-declared capacity for a runner pool, keyed by label set.
///
/// Loaded server-side from the YAML config file's `runner_pools` block,
/// composed into each `StateSnapshot` response, and merged by the frontend
/// into the `total` field of the matching derived `RunnerPoolStats`.
/// Unaffected by webhook events — this is configuration, not observed state.
///
/// `capacity` is `Option<u32>`: `Some(n)` is a declared ceiling, `None` is
/// `capacity: null` in YAML and marks the pool as unbounded. The `capacity`
/// key is required on the wire — the custom `Deserialize` impl rejects
/// inputs that omit it (mirrors the operator-config strictness).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct RunnerPoolCapacity {
    /// Canonical (sorted, deduped) label set identifying the pool.
    pub labels: LabelSet,
    /// Declared upper-bound runner count, or `None` for an unbounded pool.
    pub capacity: Option<u32>,
}

/// Custom `Deserialize` for `RunnerPoolCapacity`.
///
/// A field-level `Option<u32>` cannot distinguish a missing `capacity` key
/// from an explicit `capacity: null` — both would deserialize as `None`.
/// This visitor walks the input map manually and tracks whether the
/// `capacity` key was seen at all (`capacity_seen`). A missing key is
/// rejected with an operator-facing remediation message; `capacity: null`
/// (seen-but-null) maps to `None` (unbounded pool). Unknown keys are
/// rejected to prevent silent operator misconfiguration.
impl<'de> Deserialize<'de> for RunnerPoolCapacity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RunnerPoolCapacity;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "struct RunnerPoolCapacity")
            }

            fn visit_map<M>(self, mut map: M) -> Result<RunnerPoolCapacity, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut labels: Option<LabelSet> = None;
                let mut capacity: Option<u32> = None;
                let mut capacity_seen = false;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "labels" => {
                            if labels.is_some() {
                                return Err(serde::de::Error::duplicate_field("labels"));
                            }
                            labels = Some(map.next_value()?);
                        }
                        "capacity" => {
                            if capacity_seen {
                                return Err(serde::de::Error::duplicate_field("capacity"));
                            }
                            capacity_seen = true;
                            capacity = map.next_value::<Option<u32>>()?;
                        }
                        other => {
                            return Err(serde::de::Error::unknown_field(
                                other,
                                &["labels", "capacity"],
                            ));
                        }
                    }
                }

                let labels = labels.ok_or_else(|| serde::de::Error::missing_field("labels"))?;
                if !capacity_seen {
                    return Err(serde::de::Error::custom(
                        "capacity is required (use `capacity: null` for an unbounded pool)",
                    ));
                }
                Ok(RunnerPoolCapacity { labels, capacity })
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_id_distinct_from_job_id() {
        let run_id = RunId(42);
        let job_id = JobId(42);

        let _: RunId = run_id;
        let _: JobId = job_id;

        assert_ne!(run_id, RunId(43), "RunId equality check");
        assert_ne!(job_id, JobId(43), "JobId equality check");
    }

    #[test]
    fn test_run_id_self_equality() {
        let run_id = RunId(42);
        assert_eq!(run_id, RunId(42));
    }

    #[test]
    fn test_job_id_self_equality() {
        let job_id = JobId(42);
        assert_eq!(job_id, JobId(42));
    }

    #[test]
    fn test_run_id_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let run_id = RunId(42);
        set.insert(run_id);
        assert!(set.contains(&RunId(42)));
    }

    #[test]
    fn test_job_id_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let job_id = JobId(42);
        set.insert(job_id);
        assert!(set.contains(&JobId(42)));
    }

    #[test]
    fn test_run_id_display() {
        let run_id = RunId(42);
        assert_eq!(format!("{run_id}"), "42");
    }

    #[test]
    fn test_job_id_display() {
        let job_id = JobId(42);
        assert_eq!(format!("{job_id}"), "42");
    }

    #[test]
    fn test_label_set_equality_regardless_of_order() {
        let set1 = LabelSet::new(["linux", "self-hosted"]);
        let set2 = LabelSet::new(["self-hosted", "linux"]);
        assert_eq!(set1, set2, "Label sets should be equal regardless of order");
    }

    #[test]
    fn test_label_set_deduplication() {
        let set = LabelSet::new(["a", "a", "b"]);
        assert_eq!(set.len(), 2, "Duplicate labels should be deduplicated");
    }

    #[test]
    fn test_label_set_empty() {
        let set = LabelSet::new(Vec::<String>::new());
        assert!(
            set.is_empty(),
            "Empty label set should report is_empty() == true"
        );
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_label_set_iter() {
        // Labels should iterate in sorted order due to BTreeSet
        let set = LabelSet::new(["zebra", "apple", "banana"]);
        let labels: Vec<&str> = set.iter().collect();
        assert_eq!(labels, vec!["apple", "banana", "zebra"]);
    }

    #[test]
    fn test_label_set_from_iter() {
        let labels = vec!["linux", "self-hosted"];
        let set: LabelSet = labels.into_iter().collect();
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_label_set_hashable() {
        // LabelSet should be hashable despite containing mutable content
        use std::collections::HashSet;
        let set1 = LabelSet::new(["linux", "self-hosted"]);
        let set2 = LabelSet::new(["linux", "self-hosted"]);
        let mut hash_set = HashSet::new();
        hash_set.insert(set1);
        assert!(hash_set.contains(&set2));
    }

    #[test]
    fn label_set_orders_lexicographically() {
        let set1 = LabelSet::new(["self-hosted", "linux"]);
        let set2 = LabelSet::new(["self-hosted", "x86_64"]);
        let set3 = LabelSet::new(["ubuntu-latest"]);

        // Create a vec in non-sorted order and sort it
        let mut sets = [set3.clone(), set1.clone(), set2.clone()];
        sets.sort();

        // After sorting, should be in lexicographic order by elements
        assert_eq!(sets[0], set1, "set1 should come first");
        assert_eq!(sets[1], set2, "set2 should come second");
        assert_eq!(sets[2], set3, "set3 should come last");

        // Verify: LabelSet::new(["b", "a"]) equals LabelSet::new(["a", "b"])
        // (internal ordering independence)
        let set_ab = LabelSet::new(["a", "b"]);
        let set_ba = LabelSet::new(["b", "a"]);
        assert_eq!(
            set_ab, set_ba,
            "Label sets with same labels in different order should be equal"
        );
        // And they should have the same ordering
        assert!(
            set_ab >= set_ba && set_ba >= set_ab,
            "Equal sets should not be ordered"
        );
    }

    #[test]
    fn test_run_id_serde() {
        let run_id = RunId(42);
        let json = serde_json::to_string(&run_id).expect("serialize");
        let deserialized: RunId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(run_id, deserialized);
    }

    #[test]
    fn test_job_id_serde() {
        let job_id = JobId(42);
        let json = serde_json::to_string(&job_id).expect("serialize");
        let deserialized: JobId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(job_id, deserialized);
    }

    #[test]
    fn test_repo_id_self_equality() {
        let repo_id = RepoId(1_190_105_052);
        assert_eq!(repo_id, RepoId(1_190_105_052));
        assert_ne!(repo_id, RepoId(1));
    }

    #[test]
    fn test_repo_id_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let repo_id = RepoId(1_190_105_052);
        set.insert(repo_id);
        assert!(set.contains(&RepoId(1_190_105_052)));
    }

    #[test]
    fn test_repo_id_display() {
        let repo_id = RepoId(1_190_105_052);
        assert_eq!(format!("{repo_id}"), "1190105052");
    }

    #[test]
    fn test_repo_id_serde() {
        let repo_id = RepoId(1_190_105_052);
        let json = serde_json::to_string(&repo_id).expect("serialize");
        let deserialized: RepoId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(repo_id, deserialized);
    }

    #[test]
    fn test_repo_key_serde() {
        let repo_key = RepoKey::new("myorg", "myrepo");
        let json = serde_json::to_string(&repo_key).expect("serialize");
        let deserialized: RepoKey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(repo_key, deserialized);
    }

    #[test]
    fn test_repo_key_serde_camel_case() {
        let repo_key = RepoKey::new("myorg", "myrepo");
        let json = serde_json::to_string(&repo_key).expect("serialize");
        assert!(json.contains("\"org\""), "Should serialize org field");
        assert!(json.contains("\"repo\""), "Should serialize repo field");
    }

    #[test]
    fn test_label_set_serde() {
        let label_set = LabelSet::new(["linux", "self-hosted"]);
        let json = serde_json::to_string(&label_set).expect("serialize");
        let deserialized: LabelSet = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(label_set, deserialized);
    }

    // ============================================================================
    // Additional roundtrip and edge cases
    // ============================================================================

    #[test]
    fn test_repo_key_display() {
        let repo_key = RepoKey::new("myorg", "myrepo");
        assert_eq!(format!("{repo_key}"), "myorg/myrepo");
    }

    #[test]
    fn test_repo_key_new_with_string_refs() {
        let repo_key = RepoKey::new("org", "repo");
        assert_eq!(repo_key.org, "org");
        assert_eq!(repo_key.repo, "repo");
    }

    #[test]
    fn test_repo_key_new_with_owned_strings() {
        let org = "org".to_string();
        let repo = "repo".to_string();
        let repo_key = RepoKey::new(org, repo);
        assert_eq!(repo_key.org, "org");
        assert_eq!(repo_key.repo, "repo");
    }

    #[test]
    fn test_label_set_large_deduplication() {
        // Test with many duplicates
        let labels = vec!["a", "b", "a", "c", "b", "a", "d"];
        let set = LabelSet::new(labels);
        assert_eq!(set.len(), 4, "Should have 4 unique labels");
    }

    #[test]
    fn runner_pool_capacity_serde_round_trip() {
        let capacity = RunnerPoolCapacity {
            labels: LabelSet::new(["self-hosted", "linux", "x64"]),
            capacity: Some(10),
        };
        let json = serde_json::to_string(&capacity).expect("serialize");
        let back: RunnerPoolCapacity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(capacity, back);
    }

    #[test]
    fn runner_pool_capacity_null_round_trips() {
        let capacity = RunnerPoolCapacity {
            labels: LabelSet::new(["ubuntu-latest"]),
            capacity: None,
        };
        let json = serde_json::to_string(&capacity).expect("serialize");
        assert!(
            json.contains(r#""capacity":null"#),
            "explicit null on the wire, got: {json}"
        );
        let back: RunnerPoolCapacity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(capacity, back);
    }

    #[test]
    fn runner_pool_capacity_serializes_labels_in_canonical_order() {
        // Wire payload must reflect BTreeSet's sorted order so frontend
        // consumers see canonical-form labels regardless of insertion order.
        let capacity = RunnerPoolCapacity {
            labels: LabelSet::new(["x64", "self-hosted", "linux"]),
            capacity: Some(5),
        };
        let json = serde_json::to_string(&capacity).expect("serialize");
        assert!(
            json.contains(r#"["linux","self-hosted","x64"]"#),
            "labels should serialize sorted, got: {json}"
        );
    }

    #[test]
    fn runner_pool_capacity_accepts_explicit_null() {
        // Wire-snapshot strictness mirrors the operator-config side: an
        // explicit null is the canonical way to declare a pool unbounded.
        let json = r#"{"labels":["ubuntu-latest"],"capacity":null}"#;
        let cap: RunnerPoolCapacity = serde_json::from_str(json).expect("deserialize");
        assert_eq!(cap.capacity, None);
    }

    #[test]
    fn runner_pool_capacity_rejects_missing_capacity_key() {
        let json = r#"{"labels":["ubuntu-latest"]}"#;
        let err = serde_json::from_str::<RunnerPoolCapacity>(json)
            .expect_err("missing capacity should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("capacity is required"),
            "error should explain key requirement, got: {msg}"
        );
    }

    #[test]
    fn runner_pool_capacity_rejects_unknown_field() {
        let json = r#"{"labels":["a"],"capacity":1,"elastic":true}"#;
        let err = serde_json::from_str::<RunnerPoolCapacity>(json)
            .expect_err("unknown field should fail");
        assert!(
            err.to_string().contains("elastic"),
            "error should mention the unknown field, got: {err}"
        );
    }

    #[test]
    fn runner_pool_total_round_trips_all_three_variants() {
        let bounded = RunnerPoolTotal::Bounded(10);
        let bounded_json = serde_json::to_string(&bounded).expect("serialize bounded");
        assert_eq!(bounded_json, r#"{"kind":"Bounded","value":10}"#);
        let back: RunnerPoolTotal = serde_json::from_str(&bounded_json).expect("deserialize");
        assert_eq!(back, bounded);

        let unbounded = RunnerPoolTotal::Unbounded;
        let unbounded_json = serde_json::to_string(&unbounded).expect("serialize unbounded");
        assert_eq!(unbounded_json, r#"{"kind":"Unbounded"}"#);
        let back: RunnerPoolTotal = serde_json::from_str(&unbounded_json).expect("deserialize");
        assert_eq!(back, unbounded);

        let undeclared = RunnerPoolTotal::Undeclared;
        let undeclared_json = serde_json::to_string(&undeclared).expect("serialize undeclared");
        assert_eq!(undeclared_json, r#"{"kind":"Undeclared"}"#);
        let back: RunnerPoolTotal = serde_json::from_str(&undeclared_json).expect("deserialize");
        assert_eq!(back, undeclared);
    }
}
