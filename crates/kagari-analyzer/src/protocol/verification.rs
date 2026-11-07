//! Bounded production-code wire oracle shared by seed replay and libFuzzer.
use crate::protocol::{
    Message,
    codec::{CodecLimits, FrameDecoder, FrameError, FrameEvent},
    decode, encode,
};
use serde_json::{Value, from_slice};
use std::iter::once;

const MAX_INPUT: usize = 64 * 1024;
const MAX_FRAMES: usize = 256;

#[derive(Debug, PartialEq, Eq)]
struct Observed {
    frames: Vec<Vec<u8>>,
    failure: Option<FrameError>,
}

fn collect(input: &[u8], fragmented: bool, schedule: &[u8]) -> Observed {
    let mut decoder = FrameDecoder::new(CodecLimits {
        max_header_bytes: 256,
        max_body_bytes: MAX_INPUT,
    });
    let mut observed = Observed {
        frames: Vec::new(),
        failure: None,
    };
    let mut admitted_length = None;
    let mut cursor = 0;
    let mut chunks = 0;
    loop {
        let length = if fragmented && chunks < 255 {
            1 + usize::from(
                schedule
                    .get(chunks % schedule.len().max(1))
                    .copied()
                    .unwrap_or(0),
            )
        } else {
            input.len() - cursor
        };
        let end = cursor.saturating_add(length).min(input.len());
        loop {
            let progress = match decoder.consume(&input[cursor..end]) {
                Ok(progress) => progress,
                Err(error) => {
                    observed.failure = Some(error);
                    assert_eq!(
                        decoder.consume(b"Content-Length: 0\r\n\r\n").unwrap_err(),
                        FrameError::Failed
                    );
                    return observed;
                }
            };
            assert!(progress.consumed <= end - cursor);
            cursor += progress.consumed;
            match progress.event {
                FrameEvent::BodyLength(length) => {
                    assert!(length <= MAX_INPUT);
                    assert_eq!(admitted_length.replace(length), None);
                    decoder.grant_body().expect("bounded body allocation");
                }
                FrameEvent::Frame(body) => {
                    assert_eq!(admitted_length.take(), Some(body.len()));
                    observed.frames.push(body);
                    if observed.frames.len() == MAX_FRAMES {
                        return observed;
                    }
                }
                FrameEvent::NeedMore => {
                    assert_eq!(cursor, end);
                    break;
                }
            }
        }
        chunks += 1;
        if cursor == input.len() {
            observed.failure = decoder.finish().err();
            return observed;
        }
    }
}

fn check_encoding(message: &Message, schedule: &[u8]) {
    let Ok(frame) = encode(message, MAX_INPUT) else {
        // Decoded source can expand during JSON escaping. The production encoder
        // must reject it before emitting a header, not promise a truncated frame.
        return;
    };
    let split = frame
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("header delimiter")
        + 4;
    let body = &frame[split..];
    assert_eq!(
        &frame[..split],
        format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()
    );
    let value: Value = from_slice(body).expect("encoder emits valid JSON");
    assert_eq!(value.get("jsonrpc"), Some(&Value::String("2.0".into())));
    let object = value.as_object().expect("single envelope object");
    match message {
        Message::Request(request) => {
            assert_eq!(
                object.get("method").and_then(Value::as_str),
                Some(request.method.as_str())
            );
            assert!(object.contains_key("id"));
            assert!(!object.contains_key("result") && !object.contains_key("error"));
        }
        Message::Notification(notification) => {
            assert_eq!(
                object.get("method").and_then(Value::as_str),
                Some(notification.method.as_str())
            );
            assert!(!object.contains_key("id"));
        }
        Message::Response(response) => {
            assert!(object.contains_key("id"));
            match &response.response_result {
                Ok(result) => {
                    assert_eq!(object.get("result"), Some(result));
                    assert!(!object.contains_key("error"));
                }
                Err(error) => {
                    assert!(!object.contains_key("result"));
                    assert_eq!(
                        object["error"]["code"].as_i64(),
                        Some(i64::from(error.code))
                    );
                    assert_eq!(
                        object["error"]["message"].as_str(),
                        Some(error.message.as_str())
                    );
                }
            }
        }
    }
    let cap = schedule
        .first()
        .map_or(body.len(), |value| usize::from(*value) % (body.len() + 1));
    assert_eq!(encode(message, cap).is_ok(), cap >= body.len());
    let observed = collect(&frame, true, schedule);
    assert_eq!(observed.frames, [body.to_vec()]);
    assert_eq!(observed.failure, None);
}

/// Check raw framing plus independently constructed valid frames and EOF cuts.
pub(crate) fn check(input: &[u8]) {
    if input.len() > MAX_INPUT {
        return;
    }
    let contiguous = collect(input, false, input);
    let fragmented = collect(input, true, input);
    assert_eq!(
        contiguous, fragmented,
        "fragmentation must preserve accepted prefixes and failures"
    );
    for body in once(input).chain(contiguous.frames.iter().map(Vec::as_slice)) {
        match decode(body) {
            Ok(message) => check_encoding(&message, input),
            Err(error) => {
                assert!(!error.fatal || error.response.is_none());
                if let Some(response) = error.response {
                    assert!(matches!(
                        response.response_result.as_ref().unwrap_err().code,
                        -32700 | -32600
                    ));
                    check_encoding(&response.into(), input);
                }
            }
        }
    }

    // Literal construction is independent of the production encoder/validator:
    // every arbitrary body (including non-UTF8 bytes) must frame losslessly.
    let mut canonical = format!("Content-Length: {}\r\n\r\n", input.len()).into_bytes();
    canonical.extend_from_slice(input);
    let first_end = canonical.len();
    canonical.extend_from_slice(b"Content-Length: 4\r\n\r\nnull");
    let known = collect(&canonical, true, input);
    assert_eq!(known.frames, [input.to_vec(), b"null".to_vec()]);
    assert_eq!(known.failure, None);
    let cut = input.first().map_or(0, |byte| usize::from(*byte)) % (canonical.len() + 1);
    let truncated = collect(&canonical[..cut], true, input);
    let expected_clean = cut == 0 || cut == first_end || cut == canonical.len();
    assert_eq!(
        truncated.failure,
        (!expected_clean).then_some(FrameError::Truncated)
    );
    let expected_frames = usize::from(cut >= first_end) + usize::from(cut == canonical.len());
    assert_eq!(truncated.frames.len(), expected_frames);
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn wire_oracle_reaches_valid_envelopes_invalid_bytes_and_limit_boundaries() {
        for input in [
            b"".as_slice(),
            br#"{"jsonrpc":"2.0","id":"abc","method":"shutdown"}"#,
            br#"{"jsonrpc":"2.0","id":2,"result":null}"#,
            b"Content-Length: 1\r\n\r\n\xffContent-Length: 4\r\n\r\nnull",
            b"Content-Length: 65537\r\n\r\n",
        ] {
            check(input);
        }
        check(&vec![b'x'; 64 * 1024]);
    }
}
