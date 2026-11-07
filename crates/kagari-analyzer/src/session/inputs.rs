//! Ordered document changes; source storage retains the text and coordinate authority.
use crate::{
    protocol::Notification,
    session::{Lifecycle, SessionActor},
};
use lattice_actor::{
    context::HandlerContext,
    traits::{Handler, Message},
};
use serde_json::from_value;
use std::io;

pub(crate) struct Opened(pub Notification);

pub(crate) struct Changed(pub Notification);

pub(crate) struct Closed(pub Notification);

impl Message for Opened {}

impl Message for Changed {}

impl Message for Closed {}

impl Handler<Opened> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: Opened,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.open(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
        }
        Ok(())
    }
}

impl Handler<Changed> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: Changed,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.change(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
        }
        Ok(())
    }
}

impl Handler<Closed> for SessionActor {
    async fn handle(
        &mut self,
        _: &mut HandlerContext<'_, Self>,
        message: Closed,
    ) -> io::Result<()> {
        if self.state == Lifecycle::Running {
            let result = from_value(message.0.params)
                .map_err(|error| error.to_string())
                .and_then(|params| self.documents.close(params));
            if let Err(error) = result {
                self.logger.write(error);
            }
        }
        Ok(())
    }
}
