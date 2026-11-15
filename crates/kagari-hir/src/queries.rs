//! Read-only tooling projections of retained analysis facts.
//!
//! Calls, completion, navigation and documentation share the matching
//! analysis snapshot. Declaration/signature/body preparation, cache validity and
//! publication remain in [`crate::analysis`]. Query methods stay on the retained
//! results; this module owns their projections and presentation types.

pub mod calls;
pub mod documentation;
pub mod methods;
mod namespaces;
