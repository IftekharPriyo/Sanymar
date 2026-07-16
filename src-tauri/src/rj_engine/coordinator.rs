use crate::{errors::AppError, playback::CommentaryJob};
use tokio_util::sync::CancellationToken;

use super::BroadcastState;

#[derive(Clone, Debug)]
pub struct BroadcastCoordinator {
    state: BroadcastState,
    active_job: Option<CommentaryJob>,
    cancellation: Option<CancellationToken>,
}

impl Default for BroadcastCoordinator {
    fn default() -> Self {
        Self {
            state: BroadcastState::Idle,
            active_job: None,
            cancellation: None,
        }
    }
}

impl BroadcastCoordinator {
    pub fn start(&mut self, job: CommentaryJob) -> CancellationToken {
        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }
        let token = CancellationToken::new();
        self.active_job = Some(job);
        self.cancellation = Some(token.clone());
        self.transition(BroadcastState::Monitoring);
        token
    }

    pub fn transition(&mut self, next: BroadcastState) {
        tracing::info!(from = ?self.state, to = ?next, "broadcast state transition");
        self.state = next;
    }

    pub fn cancel_if_track_changed(&mut self, current_track_id: &str) -> bool {
        let next_track_id = self
            .active_job
            .as_ref()
            .and_then(|job| job.next_track_id.clone());
        self.cancel_if_playback_changed(current_track_id, next_track_id.as_deref())
    }

    pub fn cancel_if_playback_changed(
        &mut self,
        current_track_id: &str,
        next_track_id: Option<&str>,
    ) -> bool {
        let changed = self.active_job.as_ref().is_some_and(|job| {
            let expected_handoff = job.next_track_id.as_deref() == Some(current_track_id)
                && matches!(
                    self.state,
                    BroadcastState::WaitingForTransition
                        | BroadcastState::PausingMusic
                        | BroadcastState::Speaking
                        | BroadcastState::ResumingMusic
                );
            !expected_handoff
                && (job.current_track_id != current_track_id
                    || job
                        .next_track_id
                        .as_deref()
                        .is_some_and(|expected| Some(expected) != next_track_id))
        });
        if changed {
            if let Some(token) = self.cancellation.take() {
                token.cancel();
            }
            self.active_job = None;
            self.transition(BroadcastState::Cancelled);
        }
        changed
    }

    pub fn handoff_to_next(&mut self, job_id: &str, next_track_id: &str) -> Result<(), AppError> {
        let job = self.active_job.as_mut().ok_or(AppError::StaleJob)?;
        if job.job_id != job_id || job.next_track_id.as_deref() != Some(next_track_id) {
            return Err(AppError::StaleJob);
        }
        job.current_track_id = next_track_id.to_owned();
        job.next_track_id = None;
        Ok(())
    }

    pub fn cancel(&mut self) -> bool {
        let had_active_job = self.active_job.take().is_some();
        if let Some(token) = self.cancellation.take() {
            token.cancel();
        }
        if had_active_job {
            self.transition(BroadcastState::Cancelled);
        }
        had_active_job
    }

    pub fn ensure_current(&self, job_id: &str, current_track_id: &str) -> Result<(), AppError> {
        match &self.active_job {
            Some(job) if job.job_id == job_id && job.current_track_id == current_track_id => Ok(()),
            _ => Err(AppError::StaleJob),
        }
    }

    pub fn cancellation_for(
        &self,
        job_id: &str,
        current_track_id: &str,
    ) -> Result<CancellationToken, AppError> {
        self.ensure_current(job_id, current_track_id)?;
        self.cancellation.clone().ok_or(AppError::StaleJob)
    }

    pub fn state(&self) -> &BroadcastState {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_skip_cancels_and_rejects_stale_job() {
        let mut coordinator = BroadcastCoordinator::default();
        let cancellation = coordinator.start(CommentaryJob {
            job_id: "job-a".into(),
            current_track_id: "track-a".into(),
            next_track_id: Some("track-b".into()),
        });
        assert!(coordinator.cancel_if_track_changed("track-b"));
        assert!(cancellation.is_cancelled());
        assert_eq!(coordinator.state(), &BroadcastState::Cancelled);
        assert!(matches!(
            coordinator.ensure_current("job-a", "track-a"),
            Err(AppError::StaleJob)
        ));
    }

    #[test]
    fn queue_change_cancels_prepared_transition() {
        let mut coordinator = BroadcastCoordinator::default();
        let cancellation = coordinator.start(CommentaryJob {
            job_id: "job-a".into(),
            current_track_id: "track-a".into(),
            next_track_id: Some("track-b".into()),
        });

        assert!(coordinator.cancel_if_playback_changed("track-a", Some("track-c")));
        assert!(cancellation.is_cancelled());
        assert_eq!(coordinator.state(), &BroadcastState::Cancelled);
    }

    #[test]
    fn expected_track_handoff_remains_valid_during_transition() {
        let mut coordinator = BroadcastCoordinator::default();
        coordinator.start(CommentaryJob {
            job_id: "job-a".into(),
            current_track_id: "track-a".into(),
            next_track_id: Some("track-b".into()),
        });
        coordinator.transition(BroadcastState::PausingMusic);

        assert!(!coordinator.cancel_if_playback_changed("track-b", Some("track-c")));
        assert!(coordinator.handoff_to_next("job-a", "track-b").is_ok());
        assert!(coordinator.ensure_current("job-a", "track-b").is_ok());
    }
}
