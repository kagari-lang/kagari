//! Protocol handlers grouped by request responsibility. Shared admission and scheduling stay in session.
pub(crate) mod completion;
pub(crate) mod control;
pub(crate) mod document_sync;
pub(crate) mod lifecycle;
pub(crate) mod navigation;

use crate::{
    protocol::typed::encode_result,
    session::handlers::navigation::{definition, hover, implementation},
};
use lsp_types::{
    InitializeResult, PositionEncodingKind, ServerCapabilities, ServerInfo, request::Initialize,
};
use serde_json::{Error, Value};

/// Explicit composition; feature owners supply their own capability configuration.
pub(crate) fn initialize_response() -> Result<Value, Error> {
    let mut capabilities = ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        ..Default::default()
    };
    hover::capabilities(&mut capabilities);
    definition::capabilities(&mut capabilities);
    implementation::capabilities(&mut capabilities);
    completion::capabilities(&mut capabilities);
    document_sync::capabilities(&mut capabilities);
    let mut response = encode_result::<Initialize>(InitializeResult {
        capabilities,
        server_info: Some(ServerInfo {
            name: "kagari-analyzer".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    })?;
    implementation::patch_capabilities(&mut response);
    Ok(response)
}
