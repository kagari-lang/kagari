use std::{fs, path::Path};

fn replay(directory: &str, check: fn(&[u8])) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(directory);
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "LSP corpus must not be empty");
    check(b"");
    for path in paths {
        let bytes = fs::read(&path).unwrap();
        eprintln!("replaying {}", path.display());
        check(&bytes);
    }
}

#[test]
fn replay_lsp_wire_seeds() {
    replay("lsp_wire", kagari_analyzer::verify_wire);
}

#[test]
fn replay_lsp_session_seeds() {
    replay("lsp_session", kagari_analyzer::verify_session);
}

#[test]
fn replay_lsp_documents_seeds() {
    replay("lsp_documents", kagari_analyzer::verify_documents);
}

#[test]
fn replay_lsp_analysis_seeds() {
    replay("lsp_analysis", kagari_analyzer::verify_analysis);
}
