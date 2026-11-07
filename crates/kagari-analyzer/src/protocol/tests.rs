use crate::protocol::{
    ErrorCode, MAX_REQUEST_ID_BYTES, Message, Notification, RequestId, Response, decode, encode,
};
use serde_json::{Value, from_slice, json};

fn error_code(body: &[u8]) -> (Option<RequestId>, i32) {
    let response = decode(body)
        .unwrap_err()
        .response
        .expect("request rejection");
    (response.id, response.response_result.unwrap_err().code)
}

#[test]
fn literal_envelopes_preserve_ids_notification_identity_and_null_success() {
    let Message::Request(request) =
        decode(br#"{"jsonrpc":"2.0","id":-7,"method":"initialize","params":{"capabilities":{}}}"#)
            .unwrap()
    else {
        panic!("request expected");
    };
    assert_eq!(request.id, RequestId::Integer(-7));
    assert_eq!(request.method, "initialize");
    assert_eq!(request.params, json!({"capabilities": {}}));
    let Message::Request(request) =
        decode("{\"jsonrpc\":\"2.0\",\"id\":\"中🦀\",\"method\":\"shutdown\"}".as_bytes()).unwrap()
    else {
        panic!("request expected");
    };
    assert_eq!(request.id, RequestId::String("中🦀".into()));
    assert_eq!(request.params, Value::Null);
    assert_eq!(
        decode(br#"{"jsonrpc":"2.0","method":"exit"}"#).unwrap(),
        Message::Notification(Notification::new("exit".into(), Value::Null))
    );
    assert_eq!(
        decode(br#"{"jsonrpc":"2.0","id":4,"result":null}"#).unwrap(),
        Message::Response(Response {
            id: Some(4.into()),
            response_result: Ok(Value::Null)
        })
    );
    let Message::Response(response) = decode(
        br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"bad","data":null}}"#,
    )
    .unwrap() else {
        panic!("response expected");
    };
    assert_eq!(response.id, None);
    let error = response.response_result.unwrap_err();
    assert_eq!(error.code, -32600);
    assert_eq!(error.message, "bad");
    assert_eq!(error.data, Some(Value::Null));
}

#[test]
fn invalid_requests_recover_only_valid_ids_and_distinguish_parse_errors() {
    for body in [
        br#"{"jsonrpc":"1.0","id":3,"method":"initialize"}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":3,"method":false}"#,
        br#"{"jsonrpc":"2.0","id":3,"method":"x","result":null}"#,
    ] {
        assert_eq!(
            error_code(body),
            (Some(3.into()), ErrorCode::InvalidRequest as i32)
        );
    }
    for body in [
        br#"{"jsonrpc":"2.0","id":null,"method":"x"}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":2147483648,"method":"x"}"#,
        br#"{"jsonrpc":"2.0","id":1.0,"method":"x"}"#,
        br#"{"jsonrpc":"2.0","id":true,"method":"x"}"#,
        br#"{"jsonrpc":"2.0","method":"x","method":"y"}"#,
        br#"[{"jsonrpc":"2.0","id":1,"method":"x"}]"#,
        br#"null"#,
        br#"{}"#,
    ] {
        assert_eq!(error_code(body), (None, ErrorCode::InvalidRequest as i32));
    }
    for body in [b"{".as_slice(), b"\xff", b"{bad}"] {
        assert_eq!(error_code(body), (None, ErrorCode::ParseError as i32));
    }
}

#[test]
fn malformed_responses_never_trigger_an_error_response_loop() {
    for body in [
        br#"{"jsonrpc":"2.0","id":1}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":1,"result":null,"error":{"code":1,"message":"bad"}}"#,
        br#"{"jsonrpc":"2.0","id":null,"result":null}"#,
        br#"{"jsonrpc":"2.0","result":null}"#,
        br#"{"jsonrpc":"2.0","id":{},"result":null}"#,
        br#"{"id":1,"result":null}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":null}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":{"code":false,"message":"bad"}}"#,
        br#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":false}}"#,
        br#"{"jsonrpc":"2.0","id":1,"params":{},"result":null}"#,
        br#"{"jsonrpc":"2.0","id":1,"id":2,"result":null}"#,
    ] {
        assert!(decode(body).unwrap_err().response.is_none(), "{body:?}");
    }
}

#[test]
fn string_id_limits_count_utf8_bytes_in_decoder_and_encoder() {
    let limit = "é".repeat(MAX_REQUEST_ID_BYTES / 2);
    let accepted = json!({"jsonrpc": "2.0", "id": limit, "method": "x"});
    assert!(decode(accepted.to_string().as_bytes()).is_ok());
    let rejected = json!({"jsonrpc": "2.0", "id": format!("{limit}a"), "method": "x"});
    let failure = decode(rejected.to_string().as_bytes()).unwrap_err();
    assert!(failure.fatal);
    assert!(failure.response.is_none());
    let rejected_response = json!({"jsonrpc": "2.0", "id": format!("{limit}a"), "result": null});
    let failure = decode(rejected_response.to_string().as_bytes()).unwrap_err();
    assert!(failure.fatal);
    assert!(failure.response.is_none());
    let duplicate =
        format!("{{\"jsonrpc\":\"2.0\",\"id\":\"{limit}a\",\"id\":1,\"method\":\"x\"}}");
    let failure = decode(duplicate.as_bytes()).unwrap_err();
    assert!(failure.fatal);
    assert!(failure.response.is_none());
    let response = Response::new_ok(RequestId::String(format!("{limit}a")), Value::Null);
    assert!(encode(&response.into(), 4096).is_err());
}

#[test]
fn encoding_counts_bytes_caps_before_delivery_and_preserves_result_presence() {
    let frame = encode(&Response::new_ok(7.into(), "中🦀").into(), 1024).unwrap();
    let split = frame
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    let body = &frame[split..];
    assert_eq!(
        &frame[..split],
        format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()
    );
    assert_eq!(
        from_slice::<Value>(body).unwrap(),
        json!({"jsonrpc":"2.0","id":7,"result":"中🦀"})
    );
    let message = Response::new_ok(7.into(), "中🦀").into();
    assert_eq!(encode(&message, body.len()).unwrap(), frame);
    assert!(encode(&message, body.len() - 1).is_err());
    let null = encode(&Response::new_ok(8.into(), ()).into(), 1024).unwrap();
    assert!(null.ends_with(br#""result":null}"#));
    let error = encode(
        &Response::new_err(None, -32700, "bad JSON".into()).into(),
        1024,
    )
    .unwrap();
    assert!(
        error.ends_with(
            br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"bad JSON"}}"#
        )
    );
    assert!(
        encode(
            &Message::Response(Response {
                id: None,
                response_result: Ok(Value::Null)
            }),
            1024
        )
        .is_err()
    );
}
