//! Frozen method registration; typed Lattice messages retain admission permits.
use crate::{
    protocol::{Message as WireMessage, Notification, Request},
    session::{
        SessionActor,
        handlers::{
            completion::CompletionRequested,
            control::{
                CancelRequested, ClientResponse, ProgressCancelled, UnknownNotification,
                UnknownRequest,
            },
            document_sync::{Changed, Closed, Opened, ReloadRequested, Saved, WatchedFilesChanged},
            lifecycle::{ExitRequested, InitializeRequested, Initialized, ShutdownRequested},
            navigation::{
                definition::DefinitionRequested, hover::HoverRequested,
                implementation::ImplementationRequested,
            },
        },
    },
    transport::IngressPermit,
};
use lattice_actor::{
    context::HandlerContext,
    handle::ActorHandle,
    traits::{Handler, Message},
};
use std::io::Error;
use std::{collections::HashMap, future::Future, io, pin::Pin};

type Dispatch = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;
type RequestRoute =
    Box<dyn Fn(ActorHandle<SessionActor>, Request, IngressPermit) -> Dispatch + Send + Sync>;
type NotificationRoute =
    Box<dyn Fn(ActorHandle<SessionActor>, Notification, IngressPermit) -> Dispatch + Send + Sync>;

pub(crate) struct Ingress<M> {
    message: M,
    permit: IngressPermit,
}

impl<M: Message> Message for Ingress<M> {}

impl<M: Message> Handler<Ingress<M>> for SessionActor
where
    Self: Handler<M>,
{
    async fn handle(
        &mut self,
        ctx: &mut HandlerContext<'_, Self>,
        msg: Ingress<M>,
    ) -> io::Result<()> {
        let result = <Self as Handler<M>>::handle(self, ctx, msg.message).await;
        drop(msg.permit);
        result
    }
}

pub(crate) async fn send<M: Message>(
    handle: ActorHandle<SessionActor>,
    message: M,
    permit: IngressPermit,
) -> io::Result<()>
where
    SessionActor: Handler<M>,
{
    handle
        .tell::<Ingress<M>>(Ingress { message, permit })
        .await
        .map_err(|_| Error::other("session admission closed"))
}

pub(crate) struct Router {
    requests: HashMap<&'static str, RequestRoute>,
    notifications: HashMap<&'static str, NotificationRoute>,
}

impl Router {
    pub fn new() -> Self {
        let mut router = Self {
            requests: HashMap::new(),
            notifications: HashMap::new(),
        };
        router.request("initialize", InitializeRequested);
        router.request("shutdown", ShutdownRequested);
        router.request("textDocument/hover", HoverRequested);
        router.request("textDocument/definition", DefinitionRequested);
        router.request("textDocument/implementation", ImplementationRequested);
        router.request("textDocument/completion", CompletionRequested);
        router.request("workspace/executeCommand", ReloadRequested);
        router.notification("initialized", Initialized);
        router.notification("exit", ExitRequested);
        router.notification("$/cancelRequest", CancelRequested);
        router.notification("window/workDoneProgress/cancel", ProgressCancelled);
        router.notification("textDocument/didOpen", Opened);
        router.notification("textDocument/didChange", Changed);
        router.notification("textDocument/didClose", Closed);
        router.notification("textDocument/didSave", Saved);
        router.notification("workspace/didChangeWatchedFiles", WatchedFilesChanged);
        router
    }

    pub fn request<M: Message>(&mut self, method: &'static str, make: fn(Request) -> M)
    where
        SessionActor: Handler<M>,
    {
        assert!(
            self.requests
                .insert(
                    method,
                    Box::new(move |handle, request, permit| Box::pin(send(
                        handle,
                        make(request),
                        permit
                    )))
                )
                .is_none(),
            "duplicate request registration"
        );
    }

    pub fn notification<M: Message>(&mut self, method: &'static str, make: fn(Notification) -> M)
    where
        SessionActor: Handler<M>,
    {
        assert!(
            self.notifications
                .insert(
                    method,
                    Box::new(move |handle, notification, permit| Box::pin(send(
                        handle,
                        make(notification),
                        permit
                    )))
                )
                .is_none(),
            "duplicate notification registration"
        );
    }

    pub async fn dispatch(
        &self,
        handle: ActorHandle<SessionActor>,
        message: WireMessage,
        permit: IngressPermit,
    ) -> io::Result<bool> {
        match message {
            WireMessage::Request(request) => {
                if let Some(route) = self.requests.get(request.method.as_str()) {
                    route(handle, request, permit).await?;
                } else {
                    send(handle, UnknownRequest(request), permit).await?;
                }
                Ok(false)
            }
            WireMessage::Notification(notification) => {
                let exiting = notification.method == "exit";
                if let Some(route) = self.notifications.get(notification.method.as_str()) {
                    route(handle, notification, permit).await?;
                } else {
                    send(handle, UnknownNotification(notification), permit).await?;
                }
                Ok(exiting)
            }
            WireMessage::Response(response) => {
                send(handle, ClientResponse(response), permit).await?;
                Ok(false)
            }
        }
    }
}
