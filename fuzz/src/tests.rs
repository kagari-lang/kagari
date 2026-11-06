use std::{fs, path::Path};

use crate::check_input;

#[test]
fn replay_syntax_seeds() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("seeds/syntax");
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty());
    check_input(b"");
    for path in paths {
        let bytes = fs::read(&path).unwrap();
        eprintln!("replaying {}", path.display());
        check_input(&bytes);
    }
}

#[test]
fn replay_growth_and_coordinate_boundaries() {
    // Exercise both lexical offset widths and iterative/recursive tree growth
    // without storing large mechanically repeated corpus entries in Git.
    for text in [
        format!("/*{}*/\r\nfn f(){{1}}", "中😀".repeat(10_000)),
        format!("fn f(){{x{}}}", ".field".repeat(4096)),
        format!("fn f(){{{}x{}}}", "(".repeat(2000), ")".repeat(2000)),
    ] {
        check_input(text.as_bytes());
    }
    // Place multibyte text and token boundaries beside the exact narrow/wide
    // source-length transition, rather than only testing a much larger file.
    for length in [65_535, 65_536, 65_537] {
        let suffix = "中😀*/fn f(){x+x} //尾\r\n";
        let text = format!("/*{}{suffix}", " ".repeat(length - 2 - suffix.len()));
        assert_eq!(text.len(), length);
        check_input(text.as_bytes());
    }
}
