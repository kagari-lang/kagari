//! A bounded, nonblocking producer; only the sink thread writes standard error.
use std::{
    io::{self, Write},
    sync::mpsc::{SyncSender, sync_channel},
    thread::{Builder, JoinHandle},
};

const LOG_PREFIX: &str = "kagari-analyzer: ";
const PAYLOAD_LIMIT: usize = 1024 - LOG_PREFIX.len() - 1;

#[derive(Clone)]
pub(crate) struct LogSink(SyncSender<Box<str>>);

impl LogSink {
    pub fn start() -> io::Result<(Self, JoinHandle<()>)> {
        let (sender, receiver) = sync_channel::<Box<str>>(32);
        let thread = Builder::new()
            .name("kagari-lsp-log".into())
            .spawn(move || {
                let mut stderr = io::stderr().lock();
                while let Ok(line) = receiver.recv() {
                    if writeln!(stderr, "{LOG_PREFIX}{line}").is_err() {
                        break;
                    }
                }
            })?;
        Ok((Self(sender), thread))
    }

    pub fn write(&self, line: String) {
        let _ = self.0.try_send(bounded_line(line));
    }
}

fn bounded_line(mut line: String) -> Box<str> {
    if line.len() > PAYLOAD_LIMIT {
        let mut end = PAYLOAD_LIMIT;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    // Even a short string may retain a large capacity from its producer.
    // Queue only the bounded slice, not that unused backing allocation.
    line.into_boxed_str()
}

#[cfg(test)]
mod tests {
    use super::{LOG_PREFIX, bounded_line};

    #[test]
    fn log_payload_releases_spare_capacity_and_includes_prefix_in_byte_limit() {
        let mut short = String::with_capacity(64 * 1024);
        short.push_str("small");
        let bounded: String = bounded_line(short).into();
        assert_eq!(bounded, "small");
        assert_eq!(bounded.capacity(), bounded.len());

        let bounded = bounded_line("🦀".repeat(400));
        let formatted = format!("{LOG_PREFIX}{bounded}\n");
        assert!(formatted.len() <= 1024);
        assert!(formatted.len() + "🦀".len() > 1024);
        assert!(bounded.chars().all(|character| character == '🦀'));
    }
}
