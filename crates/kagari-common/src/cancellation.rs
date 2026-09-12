use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Shared cooperative cancellation for analysis and execution scopes.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl PartialEq for CancellationToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for CancellationToken {}

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.0.load(Ordering::Relaxed) {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;
