use std::cell::RefCell;

use crate::parser::{ParseOptions, parse};

use {kagari_common::cancellation::CancellationToken, kagari_source::source::SourceFile};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Checkpoint {
    Lex,
    Grammar,
    Lookahead,
    Publish,
}

struct Request {
    point: Checkpoint,
    remaining: usize,
    cancel: CancellationToken,
    observed_at: Option<usize>,
}

thread_local! {
    static REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
}

// Scan/grammar hooks run just after a check: a request must reach a later check.
// The publication hook instead exercises the final handoff check. Both schedule
// cancellation after work begins, without sleeps or a worker-start race.
pub(crate) fn checkpoint(point: Checkpoint, progress: usize) {
    REQUEST.with(|request| {
        let mut request = request.borrow_mut();
        if let Some(request) = request.as_mut()
            && request.point == point
            && request.observed_at.is_none()
        {
            request.remaining -= 1;
            if request.remaining == 0 {
                request.observed_at = Some(progress);
                request.cancel.cancel();
            }
        }
    });
}

#[test]
fn cancelled_parses_never_return_a_partial_success() {
    let cancel = CancellationToken::default();
    cancel.cancel();
    for text in ["", "fn f() {}"] {
        assert!(
            parse(
                &SourceFile::new("cancelled", text),
                ParseOptions::default(),
                &cancel
            )
            .is_err()
        );
    }
    let old = parse(
        &SourceFile::new("old", "mod child { fn saved() {} }"),
        ParseOptions::default(),
        &Default::default(),
    )
    .expect("fresh cancellation token");
    let retained = old.module();
    drop(old);
    let long_text = "值a".repeat(10_000);
    for text in [
        format!("/*{long_text}*/ fn f() {{}}"),
        format!("fn f() {{ \"{long_text}\" }}"),
        format!("fn f() {{ r#\"{long_text}\"# }}"),
        format!("fn f() {{ \"\\u{{1{} }}\" }}", "_".repeat(40_000)),
        format!("fn f() {{ {} }}", " ".repeat(40_000)),
    ] {
        cancelled_at(Checkpoint::Lex, 1, &text);
    }
    for text in [
        format!(
            "mod outer {{ fn f() {{ val xs = [{}]; }} }}",
            "1,".repeat(4096)
        ),
        format!("mod outer {{ fn f() {{ {} }} }}", "@ ".repeat(4096)),
    ] {
        cancelled_at(Checkpoint::Grammar, 16, &text);
    }
    cancelled_at(
        Checkpoint::Lookahead,
        2,
        &format!("#[meta({})] fn f() {{}}", "1,".repeat(4096)),
    );
    cancelled_at(Checkpoint::Publish, 1, "fn f() { 1 }");
    assert_eq!(retained.text(), "mod child { fn saved() {} }");
}

fn cancelled_at(point: Checkpoint, after: usize, text: &str) {
    let source = SourceFile::new("cancelled", text);
    let cancel = CancellationToken::default();
    REQUEST.with(|request| {
        assert!(
            request
                .replace(Some(Request {
                    point,
                    remaining: after,
                    cancel: cancel.clone(),
                    observed_at: None,
                }))
                .is_none()
        );
    });
    let result = parse(&source, ParseOptions::default(), &cancel);
    let request = REQUEST.with(|request| request.take().unwrap());
    assert!(
        request.observed_at.is_some_and(|at| at > 0),
        "{point:?}: operation reached the scheduled boundary"
    );
    assert!(
        result.is_err(),
        "{point:?}: cancelled construction is never published"
    );
}

#[test]
fn multibyte_whitespace_is_lossless_and_does_not_split_utf8() {
    let source = SourceFile::new("unicode", "fn\u{3000}main()\u{a0}{\u{2003}1\u{3000}}\r\n");
    let parsed = parse(&source, ParseOptions::default(), &Default::default())
        .expect("fresh cancellation token");
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.module().text().to_owned(), source.text());
    for token in crate::lexer::lex(source.text()) {
        assert!(source.text().is_char_boundary(token.span.start));
        assert!(source.text().is_char_boundary(token.span.end));
    }
}
