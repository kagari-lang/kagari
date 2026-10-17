//! Synchronous transport orchestration; protocol and document policy live in the session.
use crate::session::{Action, Session};
use kagari_source::source_database::SourceDatabase;
use lsp_server::Connection;
use std::{
    env,
    io::{self, Error, ErrorKind},
    process::ExitCode,
};

pub(crate) fn run_stdio() -> io::Result<ExitCode> {
    let root = env::current_dir()?;
    let root = root
        .to_str()
        .ok_or_else(|| Error::other("working directory is not UTF-8"))?;
    let source = SourceDatabase::new(root).map_err(Error::other)?;
    let mut session = Session::new(source);
    let (connection, threads) = Connection::stdio();
    let status = drive(&connection, &mut session)?;
    drop(connection);
    // Normal completion follows an exit notification or EOF, so the library's
    // reader has stopped. An error returns without joining a still-blocked stdin.
    threads.join()?;
    Ok(status)
}

fn drive(connection: &Connection, session: &mut Session) -> io::Result<ExitCode> {
    for message in &connection.receiver {
        match session.handle(message) {
            Action::None => {}
            Action::Reply(response) => connection
                .sender
                .send(response.into())
                .map_err(|_| Error::new(ErrorKind::BrokenPipe, "LSP output disconnected"))?,
            Action::Exit(status) => return Ok(status),
        }
    }
    eprintln!("kagari-analyzer: LSP input closed without an exit notification");
    Ok(ExitCode::FAILURE)
}
