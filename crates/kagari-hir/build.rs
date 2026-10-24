fn main() {
    // File additions/removals must re-expand rstest's independent UI cases.
    println!("cargo:rerun-if-changed=tests/fixtures/ui");
}
