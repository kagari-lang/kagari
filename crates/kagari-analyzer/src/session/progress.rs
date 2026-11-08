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
}

#[derive(Default)]
pub(crate) struct ProgressStreams {
    pub supported: bool,
    streams: BTreeMap<u64, Stream>,
}

impl ProgressStreams {
    pub fn begin(
        &mut self,
        serial: u64,
        token: ProgressToken,
        title: &str,
    ) -> io::Result<Notification> {
        if title.len() > 512 {
            return Err(Error::other("progress title exceeds its byte limit"));
        }
        if self.streams.values().any(|stream| stream.token == token) {
            return Err(Error::other("work-done progress token is already active"));
        }
        let notification = Notification::new(
            "$/progress".into(),
            json!({"token":token,"value":{
                "kind":"begin","title":title,"cancellable":true
            }}),
        );
        self.streams.insert(
            serial,
            Stream {
                token,
                report: None,
                reports_enabled: true,
            },
        );
        Ok(notification)
    }

    pub fn serial(&self, token: &ProgressToken) -> Option<u64> {
        self.streams
            .iter()
            .find_map(|(serial, stream)| (&stream.token == token).then_some(*serial))
    }

    pub fn report(&mut self, serial: u64, message: String) {
        if message.len() > 1024 {
            return;
        }
        let Some(stream) = self
            .streams
            .get_mut(&serial)
            .filter(|stream| stream.reports_enabled)
        else {
            return;
        };
        stream.report = Some(Notification::new(
            "$/progress".into(),
            json!({"token":stream.token,"value":{
                "kind":"report","message":message,"cancellable":true
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

#[cfg(test)]
mod tests {
    use super::ProgressStreams;
    use lsp_types::ProgressToken;

    #[test]
    fn callers_own_content_while_streams_coalesce_reports_and_preserve_end() {
        let mut streams = ProgressStreams::default();
        let first = ProgressToken::String("search".into());
        let second = ProgressToken::Number(2);
        let begin = streams
            .begin(1, first.clone(), "Search references")
            .unwrap();
        assert_eq!(begin.params["value"]["title"], "Search references");
        let begin = streams.begin(2, second.clone(), "Check workspace").unwrap();
        assert_eq!(begin.params["value"]["title"], "Check workspace");
        assert!(streams.begin(3, first.clone(), "Other task").is_err());
        streams.report(1, "Visited 1 file".into());
        streams.report(1, "Visited 2 files".into());
        let (serial, report) = streams.take_report().unwrap();
        assert_eq!(serial, 1);
        assert_eq!(report.params["token"], "search");
        assert_eq!(report.params["value"]["message"], "Visited 2 files");
        assert!(streams.take_report().is_none());
        streams.report(2, "Checking dependencies".into());
        streams.disable_reports(2);
        streams.report(2, "Ready".into());
        assert!(streams.take_report().is_none());
        for serial in [1, 2] {
            let end = streams.end(serial).unwrap();
            assert_eq!(end.params["value"]["kind"], "end");
            assert!(streams.end(serial).is_none());
        }
        assert!(streams.begin(3, first, "Search references").is_ok());
        assert!(streams.begin(4, second, "Check workspace").is_ok());
    }
}
