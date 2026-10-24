//! Shared hard admission and fixed-point work limits. Requests may only tighten them.
use crate::{types::ConcreteFunctionIdentity, updates::BodyApplication};
use kagari_common::{cancellation::CancellationToken, identity::mapping::DefinitionMappingError};
use kagari_types::ty::substitution::TypeTransformError;
use std::{collections::HashSet, error::Error, fmt};

pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_RECORDS: usize = 1_000_000;
pub const MAX_APPLICATIONS: usize = 65_536;
pub const MAX_NESTED: usize = 4_096;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub bytes: usize,
    pub records: usize,
    pub visits: usize,
    pub applications: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: MAX_BYTES,
            records: MAX_RECORDS,
            visits: MAX_RECORDS,
            applications: MAX_APPLICATIONS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    Cancelled,
    LimitExceeded,
    InvalidEvidence,
    Identity(DefinitionMappingError),
    MissingBody(ConcreteFunctionIdentity),
    MissingTarget(ConcreteFunctionIdentity),
    /// No exact registered/verified native application was supplied by the caller.
    MissingNative(ConcreteFunctionIdentity),
    /// A symbolic direct target cannot be matched to a checked current shared route.
    UnresolvedSymbolicRoute(ConcreteFunctionIdentity),
    MissingCoverage,
    ProtectedChange {
        root: ConcreteFunctionIdentity,
        unknown_edge: Option<u32>,
    },
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "update evidence: {self:?}")
    }
}

impl Error for UpdateError {}

impl From<DefinitionMappingError> for UpdateError {
    fn from(value: DefinitionMappingError) -> Self {
        match value {
            DefinitionMappingError::Cancelled => Self::Cancelled,
            DefinitionMappingError::LimitExceeded => Self::LimitExceeded,
            other => Self::Identity(other),
        }
    }
}

impl From<TypeTransformError> for UpdateError {
    fn from(value: TypeTransformError) -> Self {
        match value {
            TypeTransformError::Cancelled => Self::Cancelled,
            TypeTransformError::LimitExceeded => Self::LimitExceeded,
            TypeTransformError::InvalidContract => Self::InvalidEvidence,
        }
    }
}

/// One budget must span the combined protected evidence and retained envelope.
/// Counters are charged before queue/set insertion, never after truncating output.
pub struct Budget<'a> {
    pub(crate) cancel: &'a CancellationToken,
    remaining: Limits,
    applications: HashSet<BodyApplication>,
}

impl<'a> Budget<'a> {
    pub fn new(limits: Limits, cancel: &'a CancellationToken) -> Result<Self, UpdateError> {
        if limits.bytes > MAX_BYTES
            || limits.records > MAX_RECORDS
            || limits.visits > MAX_RECORDS
            || limits.applications > MAX_APPLICATIONS
        {
            return Err(UpdateError::LimitExceeded);
        }
        let value = Self {
            cancel,
            remaining: limits,
            applications: HashSet::new(),
        };
        value.check()?;
        Ok(value)
    }

    pub fn check(&self) -> Result<(), UpdateError> {
        self.cancel.check().map_err(|_| UpdateError::Cancelled)
    }

    /// Charge canonical records/edges before allocating or retaining them.
    pub fn records(&mut self, count: usize) -> Result<(), UpdateError> {
        self.check()?;
        self.remaining.records = self
            .remaining
            .records
            .checked_sub(count)
            .ok_or(UpdateError::LimitExceeded)?;
        Ok(())
    }

    /// Charge canonical encoded bytes before allocating their output buffer.
    pub fn bytes(&mut self, count: usize) -> Result<(), UpdateError> {
        self.check()?;
        self.remaining.bytes = self
            .remaining
            .bytes
            .checked_sub(count)
            .ok_or(UpdateError::LimitExceeded)?;
        Ok(())
    }

    /// Charge one extraction or fixed-point work visit and observe cancellation.
    pub fn visit(&mut self) -> Result<(), UpdateError> {
        self.check()?;
        self.remaining.visits = self
            .remaining
            .visits
            .checked_sub(1)
            .ok_or(UpdateError::LimitExceeded)?;
        Ok(())
    }

    /// Admit a distinct body application before enqueueing it. Repeated applications
    /// share the application allowance, but every invocation consumes a work visit.
    pub fn application(&mut self, application: &BodyApplication) -> Result<(), UpdateError> {
        self.visit()?;
        if application.arguments.len() > MAX_NESTED
            || application.body.arguments.len() > MAX_NESTED
            || !application
                .arguments
                .iter()
                .chain(&application.body.arguments)
                .all(|ty| ty.within_wire_limits())
        {
            return Err(UpdateError::LimitExceeded);
        }
        if !self.applications.contains(application) {
            self.remaining.applications = self
                .remaining
                .applications
                .checked_sub(1)
                .ok_or(UpdateError::LimitExceeded)?;
            self.applications.insert(application.clone());
        }
        Ok(())
    }
}
