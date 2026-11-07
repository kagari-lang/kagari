use super::*;

fn feed(
    decoder: &mut FrameDecoder,
    mut input: &[u8],
    frames: &mut Vec<Vec<u8>>,
) -> Result<(), FrameError> {
    loop {
        let progress = decoder.consume(input)?;
        input = &input[progress.consumed..];
        match progress.event {
            FrameEvent::BodyLength(_) => decoder.grant_body()?,
            FrameEvent::Frame(body) => frames.push(body),
            FrameEvent::NeedMore if input.is_empty() => break,
            FrameEvent::NeedMore => panic!("decoder must consume available input"),
        }
    }
    Ok(())
}

#[test]
fn every_fragment_boundary_preserves_coalesced_frames_and_empty_bodies() {
    let literal =
        b"Content-Length: 2\r\n\r\n{}Content-Length: 0\r\n\r\nContent-Length: 4\r\n\r\nnull";
    for split in 0..=literal.len() {
        let mut decoder = FrameDecoder::default();
        let mut frames = Vec::new();
        feed(&mut decoder, &literal[..split], &mut frames).unwrap();
        feed(&mut decoder, &literal[split..], &mut frames).unwrap();
        decoder.finish().unwrap();
        assert_eq!(
            frames,
            [b"{}".to_vec(), Vec::new(), b"null".to_vec()],
            "split {split}"
        );
    }
    let mut decoder = FrameDecoder::default();
    let mut frames = Vec::new();
    for byte in literal {
        feed(&mut decoder, &[*byte], &mut frames).unwrap();
    }
    assert_eq!(frames.len(), 3);
    decoder.finish().unwrap();
}

#[test]
fn allocation_wait_preserves_unconsumed_body_and_rejects_oversized_header_lengths() {
    let mut decoder = FrameDecoder::new(CodecLimits {
        max_header_bytes: 128,
        max_body_bytes: 4,
    });
    let input = b"Content-Length: 4\r\n\r\nnull";
    let progress = decoder.consume(input).unwrap();
    assert_eq!(progress.event, FrameEvent::BodyLength(4));
    assert_eq!(&input[progress.consumed..], b"null");
    assert_eq!(decoder.body.capacity(), 0);
    assert_eq!(
        decoder.consume(b"null").unwrap(),
        FrameProgress {
            consumed: 0,
            event: FrameEvent::BodyLength(4)
        }
    );
    decoder.grant_body().unwrap();
    assert_eq!(
        decoder.consume(b"nulltail").unwrap(),
        FrameProgress {
            consumed: 4,
            event: FrameEvent::Frame(b"null".to_vec())
        }
    );
    let mut too_large = FrameDecoder::new(CodecLimits {
        max_header_bytes: 128,
        max_body_bytes: 4,
    });
    assert_eq!(
        too_large.consume(b"Content-Length: 5\r\n\r\n"),
        Err(FrameError::BodyTooLarge)
    );
    assert_eq!(too_large.body.capacity(), 0);
}

#[test]
fn invalid_headers_are_terminal_and_cannot_resynchronize_at_later_json() {
    let cases: &[(&[u8], FrameError)] = &[
        (b"X-Test: x\r\n\r\n", FrameError::MissingContentLength),
        (b"Content-Length: 0\r\ncontent-length: 0\r\n\r\n", FrameError::DuplicateContentLength),
        (b"Content-Length: -1\r\n\r\n", FrameError::InvalidContentLength),
        (b"Content-Length: +1\r\n\r\n", FrameError::InvalidContentLength),
        (b"Content-Length: \r\n\r\n", FrameError::InvalidContentLength),
        (b"Content-Length: 1 0\r\n\r\n", FrameError::InvalidContentLength),
        (b"Content-Length: 999999999999999999999999999999\r\n\r\n", FrameError::InvalidContentLength),
        (b"Content-Length : 0\r\n\r\n", FrameError::InvalidHeader),
        (b"Content-Length: 0\nX: z\r\n\r\n", FrameError::InvalidHeader),
        (b"Content-Length: 0\r\nX: \xff\r\n\r\n", FrameError::InvalidHeader),
        (b"Content-Length: 0\r\nContent-Encoding: gzip\r\n\r\n", FrameError::UnsupportedEncoding),
        (b"Content-Length: 0\r\nContent-Type: application/vscode-jsonrpc; charset=utf-16\r\n\r\n", FrameError::UnsupportedEncoding),
        (b"Content-Length: 0\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8; charset=utf8\r\n\r\n", FrameError::UnsupportedEncoding),
    ];
    for (input, expected) in cases {
        let mut decoder = FrameDecoder::default();
        assert_eq!(decoder.consume(input), Err(*expected), "{input:?}");
        assert_eq!(
            decoder.consume(b"Content-Length: 2\r\n\r\n{}"),
            Err(FrameError::Failed)
        );
        assert_eq!(decoder.finish(), Err(FrameError::Failed));
    }
}

#[test]
fn header_limits_include_delimiter_and_charset_accepts_legacy_utf8_spelling() {
    let literal = b"content-length:\t2\r\nContent-Type: APPLICATION/VSCODE-JSONRPC; CHARSET=UTF8\r\nX-Trace: yes\r\n\r\n{}";
    let header_bytes = literal.len() - 2;
    let mut exact = FrameDecoder::new(CodecLimits {
        max_header_bytes: header_bytes,
        max_body_bytes: 2,
    });
    let mut frames = Vec::new();
    feed(&mut exact, literal, &mut frames).unwrap();
    assert_eq!(frames, [b"{}".to_vec()]);
    let mut short = FrameDecoder::new(CodecLimits {
        max_header_bytes: header_bytes - 1,
        max_body_bytes: 2,
    });
    assert_eq!(short.consume(literal), Err(FrameError::HeaderTooLarge));
}

#[test]
fn eof_is_clean_only_at_frame_boundaries_and_json_failure_keeps_framing() {
    let literal = b"Content-Length: 2\r\n\r\n{}";
    for end in 0..=literal.len() {
        let mut decoder = FrameDecoder::default();
        feed(&mut decoder, &literal[..end], &mut Vec::new()).unwrap();
        if end == 0 || end == literal.len() {
            assert_eq!(decoder.finish(), Ok(()));
        } else {
            assert_eq!(decoder.finish(), Err(FrameError::Truncated), "end {end}");
        }
    }
    let mut decoder = FrameDecoder::default();
    let mut frames = Vec::new();
    feed(
        &mut decoder,
        b"Content-Length: 1\r\n\r\n\xffContent-Length: 2\r\n\r\n{}",
        &mut frames,
    )
    .unwrap();
    assert_eq!(frames, [vec![0xff], b"{}".to_vec()]);
    decoder.finish().unwrap();
}
