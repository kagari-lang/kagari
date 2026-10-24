fn main() {
    // Additions and removals must re-expand the ignored rstest inventory.
    println!("cargo:rerun-if-changed=../../examples/syntax");
}
