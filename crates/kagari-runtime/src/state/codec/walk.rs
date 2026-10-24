use crate::{
    error::RuntimeError,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    state::{
        error::{
            PathComponent, SnapshotError, SnapshotErrorKind, SnapshotPath, SnapshotResult,
            SnapshotStage,
        },
        limits::{BYTE_CHUNK, Budget, SnapshotOptions},
    },
    value::Value,
};
use kagari_common::cancellation::{CancellationSubscription, CancellationToken};
use std::{
    cmp::Ordering,
    mem::size_of,
    sync::Arc,
    task::{Wake, Waker},
};

struct CancellationRelay(CancellationToken);

impl Wake for CancellationRelay {
    fn wake(self: Arc<Self>) {
        self.0.cancel();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.cancel();
    }
}

struct PathNode {
    parent: Option<usize>,
    component: PathComponent,
}

/// Linked components share prefixes; only a reported diagnostic materializes a path.
pub(super) struct Walk {
    pub(super) budget: Budget,
    paths: Vec<PathNode>,
    pub(super) current: Option<usize>,
    _option_cancellation: CancellationSubscription,
    _session_cancellation: Option<CancellationSubscription>,
}

impl Walk {
    pub(super) fn new(
        cx: &ConversionContext<'_>,
        options: &SnapshotOptions,
        stage: SnapshotStage,
    ) -> Self {
        // The shared budget has one token. Existing cancellation subscriptions
        // relay either source into an operation-local token without propagating
        // cancellation back to the host or retaining a runtime in a waker.
        let cancellation = CancellationToken::default();
        let waker = Waker::from(Arc::new(CancellationRelay(cancellation.clone())));
        let option_cancellation = options.cancellation.subscribe(waker.clone());
        let session_cancellation = cx
            .runtime()
            .resources()
            .active_session()
            .map(|session| session.options.cancellation.subscribe(waker));
        let combined = SnapshotOptions {
            limits: options.limits.clone(),
            cancellation,
        };
        Self {
            budget: Budget::new(&combined, stage),
            paths: Vec::new(),
            current: None,
            _option_cancellation: option_cancellation,
            _session_cancellation: session_cancellation,
        }
    }

    pub(super) fn finish<T>(
        &self,
        cx: &ConversionContext<'_>,
        result: SnapshotResult<T>,
    ) -> SnapshotResult<T> {
        // Session polling preserves sticky termination and the original runtime
        // error if cancellation was observed inside a portable validation pass.
        self.runtime(cx.poll())?;
        result
    }

    pub(super) fn poll(&self, cx: &ConversionContext<'_>) -> SnapshotResult<()> {
        self.runtime(cx.poll())?;
        self.budget.cancel()
    }

    /// Run a checked runtime kernel with codec accounting at its actual pass
    /// boundaries. The error slot preserves the codec kind/path instead of turning
    /// budget/cancellation failures into a generic runtime conversion error.
    pub(super) fn checked<T>(
        &mut self,
        cx: &ConversionContext<'_>,
        operation: impl FnOnce(&mut dyn FnMut() -> NativeResult<()>) -> NativeResult<T>,
    ) -> SnapshotResult<T> {
        let mut failure = None;
        let result = operation(&mut || {
            let result = self
                .poll(cx)
                .and_then(|()| self.budget.work(1 + size_of::<Value>() as u64));
            match result {
                Ok(()) => Ok(()),
                Err(error) => {
                    failure = Some(error);
                    Err(RuntimeError::resource_limit(
                        "state codec stopped checked traversal",
                    ))
                }
            }
        });
        match failure {
            Some(error) => self.locate(Err(error)),
            None => self.runtime(result),
        }
    }

    pub(super) fn runtime<T>(&self, result: Result<T, RuntimeError>) -> SnapshotResult<T> {
        result.map_err(|error| {
            SnapshotError::runtime(self.budget.stage, error).at(&self.path(self.current))
        })
    }

    pub(super) fn error(&self, kind: SnapshotErrorKind, message: &str) -> SnapshotError {
        self.budget
            .error(kind, message)
            .at(&self.path(self.current))
    }

    pub(super) fn locate<T>(&self, result: SnapshotResult<T>) -> SnapshotResult<T> {
        result.map_err(|error| {
            if error.path.0.is_empty() {
                error.at(&self.path(self.current))
            } else {
                error
            }
        })
    }

    pub(super) fn path(&self, mut index: Option<usize>) -> SnapshotPath {
        let mut components = Vec::new();
        while let Some(slot) = index {
            let node = &self.paths[slot];
            components.push(node.component.clone());
            index = node.parent;
        }
        components.reverse();
        SnapshotPath(components)
    }

    pub(super) fn push(&mut self, component: PathComponent) -> SnapshotResult<Option<usize>> {
        self.budget.work(1)?;
        // Incremental diagnostics use geometric growth; exact reservation at
        // every edge would repeatedly copy all preceding path components.
        self.paths
            .try_reserve(1)
            .map_err(|_| self.error(SnapshotErrorKind::Allocation, "snapshot path components"))?;
        let previous = self.current;
        self.current = Some(self.paths.len());
        self.paths.push(PathNode {
            parent: previous,
            component,
        });
        Ok(previous)
    }

    pub(super) fn pop(&mut self, previous: Option<usize>) {
        self.current = previous;
    }

    pub(super) fn vector<T>(&mut self, count: usize, schema: bool) -> SnapshotResult<Vec<T>> {
        self.budget.children(count, schema)?;
        let mut result = Vec::new();
        self.budget.reserve(&mut result, count)?;
        Ok(result)
    }

    pub(super) fn output<T>(&mut self, count: usize, schema: bool) -> SnapshotResult<Vec<T>> {
        self.budget.admit_children(count, schema)?;
        let mut result = Vec::new();
        self.budget.reserve(&mut result, count)?;
        Ok(result)
    }

    pub(super) fn fields(&mut self, count: usize) -> SnapshotResult<()> {
        self.budget
            .count(count, self.budget.limits.max_fields, "fields")
    }

    pub(super) fn copy_text(
        &mut self,
        cx: &ConversionContext<'_>,
        text: &str,
        identifier: bool,
    ) -> SnapshotResult<String> {
        self.poll(cx)?;
        self.budget.copy_string(text, identifier)
    }

    pub(super) fn compare_text(
        &mut self,
        cx: &ConversionContext<'_>,
        left: &str,
        right: &str,
    ) -> SnapshotResult<Ordering> {
        for (a, b) in left
            .as_bytes()
            .chunks(BYTE_CHUNK)
            .zip(right.as_bytes().chunks(BYTE_CHUNK))
        {
            self.poll(cx)?;
            self.budget.bytes(a.len().max(b.len()))?;
            let count = a.len().min(b.len());
            let order = a[..count].cmp(&b[..count]);
            if !order.is_eq() {
                return Ok(order);
            }
        }
        Ok(left.len().cmp(&right.len()))
    }
}
