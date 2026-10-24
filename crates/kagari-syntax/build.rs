fn main() {
    // rstest's file list must be expanded again on additions as well as edits.
    println!("cargo:rerun-if-changed=tests/fixtures/ui/parser");
}
