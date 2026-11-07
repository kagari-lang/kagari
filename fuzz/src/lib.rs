//! Syntax fuzz entrypoint shared by coverage-guided fuzzing and corpus replay.

mod syntax;
mod traversal;

#[cfg(test)]
mod tests;

/// Checks bounded UTF-8 source through parsing, recovery, traversal and destruction.
///
/// Inputs over 128 KiB or containing invalid UTF-8 are skipped: SourceFile accepts
/// text, and parser limits do not bound source size. Panics are intentionally not
/// caught, so libFuzzer can report and minimize failing source verbatim.
pub fn check_input(data: &[u8]) {
    if data.len() > 128 * 1024 {
        return;
    }
    if let Ok(text) = str::from_utf8(data) {
        syntax::check(text);
    }
}
