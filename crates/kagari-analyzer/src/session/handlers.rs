//! Protocol handlers grouped by request responsibility. Shared admission and scheduling stay in session.
pub(crate) mod completion;
pub(crate) mod control;
pub(crate) mod document_sync;
pub(crate) mod lifecycle;
pub(crate) mod navigation;
