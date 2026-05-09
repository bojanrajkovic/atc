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
}
