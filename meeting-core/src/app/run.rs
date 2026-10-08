//! F5-FR10 / F5-FR7: starting, running and cancelling a job for `nativeTranscribe`,
//! `nativeSummarise` and `nativeCancel`. Blocking: never call these on a runtime thread.
use super::App;
use crate::api::error_json;
use crate::jobs;
use crate::keyfile::{self, Provider, ProviderKey};
use crate::library::LibError;
use crate::pipeline::{self, JobError, JobPlan};
use crate::providers;
use crate::settings::{self, Role, Settings};
use crate::status::Phase;
use serde_json::json;

/// Which job to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// Pending segments, then summarise.
    Transcribe,
}

impl Job {
    fn label(self) -> &'static str {
        match self {
            Job::Transcribe => "Transcribe",
        }
    }
}

/// The user-facing reason a provider can't be used now (F4-FR5 wording).
fn key_problem(p: Provider, e: keyfile::KeyError) -> String {
    let ks = settings::key_status_of(p, &Err(e));
    ks.detail().map(str::to_owned).unwrap_or_default()
}

fn phase_word(p: Phase) -> &'static str {
    match p {
        Phase::Recording => "recording",
        Phase::Recorded => "recorded",
        Phase::Transcribing => "transcribing",
        Phase::Transcribed => "transcribed",
        Phase::Done => "done",
        Phase::Failed(_) => "failed",
    }
}

impl App {
    /// The role's provider, key and model, read now (F4-FR3, F10-FR4).
    fn pick(&self, s: &Settings, role: Role) -> Result<(ProviderKey, String), String> {
        let usable = |p| settings::key_status(&self.core.key_dir, p).usable();
        let p = settings::effective_provider(s, role, usable);
        let key = keyfile::load(&self.core.key_dir, p).map_err(|e| key_problem(p, e))?;
        let from_key = Some((key.model.as_str(), key.transcribe_model.as_deref()));
        // Always Some with a key: summarise has the file's model, transcribe a default.
        let model = settings::effective_model(role, p, s.role(role).model.as_deref(), from_key)
            .map_or_else(String::new, |(m, _)| m);
        Ok((key, model))
    }

    /// F5-FR10: snapshot what `job` needs. A later Settings change or key-file edit does not
    /// reach the running job.
    pub fn plan(&self, job: Job) -> Result<JobPlan, String> {
        let (s, _) = self.settings();
        let transcribe = match job {
            Job::Transcribe => Some(self.pick(&s, Role::Transcribe)?),
        };
        Ok(JobPlan {
            http: providers::http_client(jobs::CONNECT_TIMEOUT, self.core.https_only)?,
            endpoints: self.core.endpoints.clone(),
            transcribe,
            default_template: s.default_template.clone(),
        })
    }

    /// Run `job` on `id` to its end and reply `{"meeting": <sidecar>}`, or `{"error"}` when
    /// nothing was started (busy, no key, not offered in this status). The final status is in
    /// the sidecar before this returns (F5-FR7).
    pub fn run_job_json(&self, id: &str, job: Job) -> String {
        let Some(guard) = self.jobs.start(id) else {
            return error_json("meeting is busy");
        };
        let plan = match self.plan(job) {
            Ok(p) => p,
            Err(e) => return error_json(&e),
        };
        let lib = self.library();
        let fut = match job {
            Job::Transcribe => pipeline::transcribe(&lib, &self.write, id, &plan, &guard.cancel),
        };
        let r = crate::runtime::block_on(fut);
        drop(guard);
        match r {
            Err(e) => error_json(&format!("runtime: {e}")),
            Ok(Err(JobError::NotStarted(LibError::Illegal(i)))) => error_json(&format!(
                "{} isn't available while {}",
                job.label(),
                phase_word(i.from)
            )),
            Ok(Err(JobError::NotStarted(e))) => error_json(&e.to_string()),
            Ok(_) => match lib.get(id) {
                Ok(v) => json!({"meeting": v.meeting}).to_string(),
                Err(e) => error_json(&e.to_string()),
            },
        }
    }

    /// `nativeCancel`: `{"cancelled": bool}`. Never blocks on the job; the job writes
    /// failed "Cancelled" itself, keeping finished segments (F5-AC13).
    pub fn cancel_json(&self, id: &str) -> String {
        json!({"cancelled": self.jobs.cancel(id)}).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::FailedAt;

    #[test]
    fn phase_words() {
        let words = Phase::ALL.map(phase_word);
        let want = [
            "recording",
            "recorded",
            "transcribing",
            "transcribed",
            "done",
            "failed",
            "failed",
        ];
        assert_eq!(words, want);
        assert_eq!(phase_word(Phase::Failed(FailedAt::Summarising)), "failed");
    }
}
