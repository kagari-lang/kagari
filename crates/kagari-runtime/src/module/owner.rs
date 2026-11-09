//! Optional preparation ownership retained by the exact executable descriptor.
use std::{any::Any, fmt::Debug};

/// Retains a preparation product for the lifetime of its installed code. This is
/// ownership only, never verification evidence or Runtime execution authority.
/// Implementations must not retain an owning cycle back to the installed wrapper.
pub trait ProgramCodeOwner: Any + Debug + Send + Sync {}
