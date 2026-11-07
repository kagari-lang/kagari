//! Begin/end are reliable; each active token retains at most one optional report.
use crate::protocol::Notification;
use lsp_types::ProgressToken;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{self, Error},
};

struct Stream {
    token: ProgressToken,
    report: Option<Notification>,
    reports_enabled: bool,
    chunks: usize,
}

#[derive(Default)]
pub(crate) struct ProgressStreams {
    pub supported: bool,
    streams: BTreeMap<u64, Stream>,
}

impl ProgressStreams {
    pub fn begin(&mut self, serial: u64, token: ProgressToken) -> io::Result<Notification> {
        if self.streams.values().any(|stream| stream.token == token) {
            return Err(Error::other("work-done progress token is already active"));
        }
        let notification = Notification::new(
            "$/progress".into(),
            json!({"token":token,"value":{
                "kind":"begin","title":"Find implementations","cancellable":true
            }}),
        );
        self.streams.insert(
            serial,
            Stream {
                token,
                report: None,
                reports_enabled: true,
                chunks: 0,
            },
        );
        Ok(notification)
    }

    pub fn serial(&self, token: &ProgressToken) -> Option<u64> {
        self.streams
            .iter()
            .find_map(|(serial, stream)| (&stream.token == token).then_some(*serial))
    }

    pub fn report(&mut self, serial: u64) {
        let Some(stream) = self
            .streams
            .get_mut(&serial)
            .filter(|stream| stream.reports_enabled)
        else {
            return;
        };
        stream.chunks += 1;
        stream.report = Some(Notification::new(
            "$/progress".into(),
            json!({"token":stream.token,"value":{
                "kind":"report","message":format!("Indexed {} project contexts",stream.chunks),"cancellable":true
            }}),
        ));
    }

    pub fn take_report(&mut self) -> Option<(u64, Notification)> {
        self.streams
            .iter_mut()
            .find_map(|(serial, stream)| stream.report.take().map(|report| (*serial, report)))
    }

    pub fn disable_reports(&mut self, serial: u64) {
        if let Some(stream) = self.streams.get_mut(&serial) {
            stream.reports_enabled = false;
            stream.report = None;
        }
    }

    pub fn end(&mut self, serial: u64) -> Option<Notification> {
        let stream = self.streams.remove(&serial)?;
        Some(Notification::new(
            "$/progress".into(),
            json!({"token":stream.token,"value":{"kind":"end"}}),
        ))
    }
}
