//! LSP lifecycle, capability publication and ordered document notification dispatch.
use crate::documents::Documents;
use kagari_source::source_database::SourceDatabase;
use lsp_server::{ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    InitializeParams, InitializeResult, InitializedParams, PositionEncodingKind,
    ServerCapabilities, ServerInfo, TextDocumentSyncKind, TextDocumentSyncOptions,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
        Notification as NotificationMethod,
    },
    request::{Initialize, Request as RequestMethod, Shutdown},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, from_value};
use std::process::ExitCode;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Uninitialized,
    AwaitingInitialized,
    Running,
    Shutdown,
}

pub(crate) enum Action {
    None,
    Reply(Response),
    Exit(ExitCode),
}

pub(crate) struct Session {
    state: State,
    documents: Documents,
}

impl Session {
    pub(crate) fn new(source: SourceDatabase) -> Self {
        Self {
            state: State::Uninitialized,
            documents: Documents::new(source),
        }
    }

    pub(crate) fn handle(&mut self, message: Message) -> Action {
        match message {
            Message::Request(request) => Action::Reply(self.request(request)),
            Message::Notification(notification) => self.notification(notification),
            Message::Response(response) => {
                eprintln!(
                    "kagari-analyzer: unexpected client response {}",
                    response.id
                );
                Action::None
            }
        }
    }

    fn request(&mut self, request: Request) -> Response {
        let reject = |code: ErrorCode, message: &str| {
            Response::new_err(request.id.clone(), code as i32, message.into())
        };
        if self.state == State::Shutdown {
            return reject(ErrorCode::InvalidRequest, "server has shut down");
        }
        if request.method == Initialize::METHOD {
            if self.state != State::Uninitialized {
                return reject(ErrorCode::InvalidRequest, "server is already initialized");
            }
            if let Err(error) = decode::<InitializeParams>(request.params.clone()) {
                return reject(ErrorCode::InvalidParams, &error);
            }
            self.state = State::AwaitingInitialized;
            return Response::new_ok(request.id, capabilities());
        }
        if matches!(
            self.state,
            State::Uninitialized | State::AwaitingInitialized
        ) {
            return reject(ErrorCode::ServerNotInitialized, "server is not initialized");
        }
        if request.method == Shutdown::METHOD {
            if let Err(error) = decode::<()>(request.params.clone()) {
                return reject(ErrorCode::InvalidParams, &error);
            }
            self.state = State::Shutdown;
            return Response::new_ok(request.id, ());
        }
        reject(ErrorCode::MethodNotFound, "unsupported request")
    }

    fn notification(&mut self, notification: Notification) -> Action {
        if notification.method == Exit::METHOD {
            let valid = decode::<()>(notification.params).is_ok();
            let orderly = valid && self.state == State::Shutdown;
            if !orderly {
                eprintln!("kagari-analyzer: exit requires a completed shutdown and no parameters");
            }
            return Action::Exit(if orderly {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        let result = if notification.method == Initialized::METHOD {
            self.initialized(notification.params)
        } else if self.state != State::Running {
            Err("notification outside the running session".into())
        } else {
            self.document_notification(&notification.method, notification.params)
        };
        if let Err(error) = result {
            eprintln!("kagari-analyzer: ignored {}: {error}", notification.method);
        }
        Action::None
    }

    fn initialized(&mut self, params: Value) -> Result<(), String> {
        if self.state != State::AwaitingInitialized {
            return Err("unexpected initialized notification".into());
        }
        decode::<InitializedParams>(params)?;
        self.state = State::Running;
        Ok(())
    }

    fn document_notification(&mut self, method: &str, params: Value) -> Result<(), String> {
        match method {
            DidOpenTextDocument::METHOD => self
                .documents
                .open(decode::<DidOpenTextDocumentParams>(params)?),
            DidChangeTextDocument::METHOD => self
                .documents
                .change(decode::<DidChangeTextDocumentParams>(params)?),
            DidCloseTextDocument::METHOD => self
                .documents
                .close(decode::<DidCloseTextDocumentParams>(params)?),
            _ if method.starts_with("$/") => Ok(()),
            _ => Err("unsupported notification".into()),
        }
    }
}

fn decode<T: DeserializeOwned>(params: Value) -> Result<T, String> {
    from_value(params).map_err(|error| format!("invalid parameters: {error}"))
}

fn capabilities() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: Some(PositionEncodingKind::UTF16),
            text_document_sync: Some(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                }
                .into(),
            ),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "kagari-analyzer".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(session: &mut Session, method: &str, params: Value) -> Response {
        let Action::Reply(response) =
            session.handle(Request::new(1.into(), method.into(), params).into())
        else {
            panic!("request must receive a response");
        };
        response
    }

    fn notify(session: &mut Session, method: &str, params: Value) -> Action {
        session.handle(Notification::new(method.into(), params).into())
    }

    fn code(response: Response) -> i32 {
        response.response_result.unwrap_err().code
    }

    #[test]
    fn invalid_initialized_notifications_do_not_advance_the_session() {
        let mut session = Session::new(SourceDatabase::new("c:/project").unwrap());
        assert!(matches!(
            notify(&mut session, "initialized", json!({})),
            Action::None
        ));

        assert!(
            request(&mut session, "initialize", json!({"capabilities": {}}))
                .response_result
                .is_ok()
        );

        assert!(matches!(
            notify(&mut session, "initialized", Value::Null),
            Action::None
        ));
        assert_eq!(
            code(request(&mut session, "custom/request", Value::Null)),
            -32002
        );
        notify(&mut session, "initialized", json!({}));
        assert_eq!(
            code(request(&mut session, "custom/request", Value::Null)),
            -32601
        );
    }
}
