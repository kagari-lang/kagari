//! Bounded detached async provenance never owns code versions or script values.
use crate::{
    Runtime,
    error_trace::{ErrorFrame, ErrorTrace},
    task::{ScopeId, TaskId},
};
use kagari_bytecode::{artifact::ArtifactFingerprint, module::CallableTarget, program::ModuleRef};
use std::{fmt, slice};

pub const MAX_ASYNC_BOUNDARIES: usize = 32;

/// Identifies the checked factory even for host admission without script frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallableOrigin {
    pub epoch: u64,
    pub code_fingerprint: ArtifactFingerprint,
    pub module: ModuleRef,
    pub target: CallableTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnOrigin {
    pub factory: CallableOrigin,
    /// None for host admission or unavailable diagnostics; missing source maps alone
    /// do not remove the portable script frame identity.
    pub site: Option<ErrorFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsyncBoundary {
    Spawn {
        task: TaskId,
        scope: ScopeId,
        origin: SpawnOrigin,
    },
    Await {
        task: TaskId,
        site: Option<ErrorFrame>,
    },
}

impl ErrorTrace {
    pub(crate) fn append_async(&mut self, boundary: AsyncBoundary) {
        if self.async_boundaries.len() >= MAX_ASYNC_BOUNDARIES
            || self.async_boundaries.try_reserve(1).is_err()
        {
            self.omitted_async_boundaries = self.omitted_async_boundaries.saturating_add(1);
            self.incomplete = true;
        } else {
            self.async_boundaries.push(boundary);
        }
    }
}

impl Runtime {
    pub(crate) fn capture_async_site(&self) -> Option<ErrorFrame> {
        let session = self.resources().active_session()?;
        let frames = self.resources().sessions.frames(session.id)?;
        // Admission's native wrapper is not the script's logical spawn site.
        let frame = frames
            .iter()
            .rev()
            .find(|frame| matches!(frame.target(), CallableTarget::Script(_)))?;
        ErrorTrace::from_frames(slice::from_ref(frame))
            .frames
            .first()
            .cloned()
    }
}

impl fmt::Display for AsyncBoundary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let site = match self {
            Self::Spawn {
                task,
                scope,
                origin,
            } => {
                write!(
                    f,
                    "spawned {task:?} in {scope:?}; factory {:?}/{:?}, epoch {}, code {:?}",
                    origin.factory.module,
                    origin.factory.target,
                    origin.factory.epoch,
                    origin.factory.code_fingerprint
                )?;
                &origin.site
            }
            Self::Await { task, site } => {
                write!(f, "awaited {task:?}")?;
                site
            }
        };
        if let Some(site) = site {
            write!(
                f,
                " at {} ({}; {:?}/{:?} instruction {}; epoch {})",
                site.function_name,
                site.source_uri,
                site.module,
                site.target,
                site.instruction_offset,
                site.epoch
            )?;
        } else {
            write!(f, " [host entry or site unavailable]")?;
        }
        Ok(())
    }
}
