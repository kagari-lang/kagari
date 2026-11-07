//! Memory transport exercises the actual actor, codec, outbox and deadlines.
use crate::server::{
    options::{Endpoint, SessionLimits},
    run_endpoint,
};
use crate::tests::peer::{read, send};
use serde_json::{Value, json};
use std::{process::ExitCode, thread, time::Duration};
use tokio::{
    io::{AsyncReadExt, duplex},
    runtime::Builder,
    time::timeout,
};

#[test]
fn real_actor_drains_pipelined_shutdown_and_exit_without_client_eof() {
    let (mut peer, server) = duplex(256);
    let task = thread::spawn(move || {
        run_endpoint(Endpoint::Memory(server), SessionLimits::default()).unwrap()
    });
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async { timeout(Duration::from_secs(5), async {
        send(&mut peer, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}})).await;
        assert_eq!(read(&mut peer).await["id"], 1);
        send(&mut peer, json!({"jsonrpc":"2.0","method":"initialized","params":{}})).await;
        // Reuse the same wire ID while an older writer acknowledgement may still be queued.
        send(&mut peer, json!({"jsonrpc":"2.0","id":1,"method":"missing"})).await;
        assert_eq!(read(&mut peer).await["error"]["code"], -32601);
        send(&mut peer, json!({"jsonrpc":"2.0","id":2,"method":"shutdown"})).await;
        send(&mut peer, json!({"jsonrpc":"2.0","method":"exit"})).await;
        let shutdown = read(&mut peer).await;
        assert_eq!(shutdown["id"], 2);
        assert_eq!(shutdown.get("result"), Some(&Value::Null));
        assert_eq!(peer.read_u8().await.unwrap_err().kind(), std::io::ErrorKind::UnexpectedEof);
    }).await }).unwrap();
    assert_eq!(task.join().unwrap(), ExitCode::SUCCESS);
}

#[test]
fn stalled_writer_expires_without_waiting_for_another_input_frame() {
    let (mut peer, server) = duplex(1);
    let limits = SessionLimits {
        write: Duration::from_millis(30),
        ..Default::default()
    };
    let task = thread::spawn(move || run_endpoint(Endpoint::Memory(server), limits).unwrap());
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async { timeout(Duration::from_secs(5), async {
        send(&mut peer, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}})).await;
        // Keep the read side open but never consume its one-byte output capacity.
    }).await }).unwrap();
    assert_eq!(task.join().unwrap(), ExitCode::FAILURE);
}
