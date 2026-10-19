//! Jobs: batches of a project's images assigned to one annotator, and the project statistics.

use serde::{Deserialize, Serialize};

use crate::user::User;

/// A job with its progress. Jobs are shown as "Job {number}".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub number: i64,
    pub assignee: Option<User>,
    pub images: i64,
    /// Images marked annotated.
    pub annotated: i64,
    pub annotations: i64,
}

/// Puts images into new jobs: one job, or jobs of `per_job` images each in path order. Images
/// already in a job move to the new one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewJobs {
    pub image_ids: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_job: Option<u32>,
    /// The user every new job is assigned to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<i64>,
}

/// Assigns a job to a user, or (with none) to nobody.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobChange {
    pub assignee: Option<i64>,
}

/// Annotation counts of a project per label, per job and per user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statistics {
    pub labels: Vec<LabelCount>,
    pub jobs: Vec<Job>,
    pub users: Vec<UserCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelCount {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub annotations: i64,
    /// Images with at least one annotation or tag of the label.
    pub images: i64,
}

/// What the jobs assigned to one user hold; `user` is none for images in no job or in an
/// unassigned one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCount {
    pub user: Option<User>,
    pub images: i64,
    pub annotated: i64,
    pub annotations: i64,
}
