// Share the runtime-owned source-free authoring helper across embedding fixtures.
#[cfg(feature = "source")]
#[allow(dead_code)] // Embedding uses the builder, not the native-only convenience fixture.
#[path = "../../../../../kagari-runtime/tests/support/native/program.rs"]
mod native_program;

#[cfg(feature = "source")]
mod conformance;

#[cfg(feature = "source")]
mod embedding_api;

#[cfg(all(feature = "source", feature = "native"))]
mod error_traces;

#[cfg(feature = "source")]
mod generic_reload;

#[cfg(all(feature = "source", feature = "native"))]
mod host_interfaces;

#[cfg(feature = "source")]
mod native_provider_reset;

#[cfg(all(feature = "source", feature = "native"))]
mod offline_composite;

#[cfg(all(feature = "source", feature = "native"))]
mod offline_functions;

#[cfg(all(feature = "source", feature = "native"))]
mod offline_nominal;

#[cfg(feature = "source")]
mod runtime_transfer;

#[cfg(feature = "source")]
mod scoped_calls;

#[cfg(all(feature = "source", feature = "serde"))]
mod state_snapshot;

#[cfg(feature = "source")]
mod typed_calls;

#[cfg(feature = "source")]
mod async_execution;
