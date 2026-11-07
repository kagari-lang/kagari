#![no_main]

use kagari_analyzer::verify_documents;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    verify_documents(data);
});
