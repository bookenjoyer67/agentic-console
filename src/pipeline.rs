//! The CI pipeline file, parsed rather than assumed.
//!
//! The FLOW screen's lane A is the pull-request map: five jobs and their `needs` chain. The jobs and
//! the chain are read out of the workflow YAML the config points at
//! (`artifacts.pipeline`, `.github/workflows/ci.yml` here), not from a table baked into the console,
//! so a fork's pipeline draws its own shape. The file's own header comment is parsed too, because the
//! file is where this repository records which jobs may fail the build.

/// One job read out of the pipeline file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// The YAML job key, e.g. `policy-gate`.
    pub key: String,
    /// The `name:` value, when the job carries one.
    pub display_name: Option<String>,
    /// The `needs` entries, normalised to a list.
    pub needs: Vec<String>,
    /// `continue-on-error: true` on the job, which is what makes it advisory.
    pub continue_on_error: bool,
    /// The 1-based line number the job key sits on.
    pub line: usize,
}

impl Job {
    /// Whether this job can fail the build, from the file alone.
    pub fn can_block_merge(&self) -> bool {
        !self.continue_on_error
    }
}

/// A pipeline parsed from the workflow YAML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub jobs: Vec<Job>,
    /// Job keys named on the file's `#   gating` line, when the file carries one.
    pub declared_gating: Vec<String>,
    /// Job keys named on the file's `#   advisory` line.
    pub declared_advisory: Vec<String>,
    /// The `name:` of the workflow.
    pub workflow_name: Option<String>,
}

fn clean(value: &str) -> String {
    value.trim().trim_matches(['"', '\'']).to_string()
}

/// Split `needs: [a, b]` or `needs: a` into a list of job keys.
fn parse_needs(value: &str) -> Vec<String> {
    let body = value.trim().trim_start_matches('[').trim_end_matches(']');
    body.split(',')
        .map(clean)
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// Parse the workflow's `jobs:` block plus its `#   gating` / `#   advisory` header comment.
pub fn parse_pipeline(text: &str) -> Pipeline {
    let mut jobs: Vec<Job> = Vec::new();
    let mut declared_gating: Vec<String> = Vec::new();
    let mut declared_advisory: Vec<String> = Vec::new();
    let mut workflow_name: Option<String> = None;
    let mut in_jobs = false;
    let mut index = 0usize;

    for (position, line) in text.lines().enumerate() {
        let number = position + 1;
        let trimmed = line.trim_end();
        if let Some(comment) = trimmed.trim().strip_prefix('#') {
            let comment = comment.trim();
            if let Some(rest) = comment.strip_prefix("gating") {
                declared_gating = rest
                    .split(',')
                    .map(clean)
                    .filter(|e| !e.is_empty())
                    .collect();
            } else if let Some(rest) = comment.strip_prefix("advisory") {
                declared_advisory = rest
                    .split(',')
                    .map(clean)
                    .filter(|e| !e.is_empty())
                    .collect();
            }
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        if trimmed == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            // Before the jobs block, a top-level `name:` is the workflow's name.
            if indent == 0 && workflow_name.is_none() {
                if let Some(rest) = trimmed.strip_prefix("name:") {
                    workflow_name = Some(clean(rest));
                }
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if indent == 0 {
            in_jobs = false;
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("name:") {
            if indent == 4 {
                if let Some(job) = jobs.get_mut(index) {
                    job.display_name = Some(clean(name));
                }
            }
            continue;
        }
        // A job key sits at two spaces of indentation and ends in a colon.
        if indent == 2 && trimmed.ends_with(':') && !trimmed.contains(": ") {
            let key = clean(trimmed.trim_end_matches(':'));
            if !key.is_empty() {
                jobs.push(Job {
                    key,
                    display_name: None,
                    needs: Vec::new(),
                    continue_on_error: false,
                    line: number,
                });
                index = jobs.len() - 1;
            }
            continue;
        }
        if indent >= 4 {
            if let Some(rest) = trimmed.trim_start().strip_prefix("needs:") {
                if let Some(job) = jobs.get_mut(index) {
                    job.needs.extend(parse_needs(rest));
                }
            } else if let Some(rest) = trimmed.trim_start().strip_prefix("continue-on-error:") {
                let value = clean(rest);
                if let Some(job) = jobs.get_mut(index) {
                    job.continue_on_error |= value == "true";
                }
            }
        }
    }
    Pipeline {
        jobs,
        declared_gating,
        declared_advisory,
        workflow_name,
    }
}

impl Pipeline {
    /// The job key at the index `i`, or `None`.
    pub fn job(&self, i: usize) -> Option<&Job> {
        self.jobs.get(i)
    }
}

/// The `needs` chain of a job as one display string, e.g. `needs: change-type-check + policy-gate`.
pub fn needs_text(job: &Job) -> String {
    if job.needs.is_empty() {
        "needs: nothing (root job, every run)".to_string()
    } else {
        format!("needs: {}", job.needs.join(" + "))
    }
}
