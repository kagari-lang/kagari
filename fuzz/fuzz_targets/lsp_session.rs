#![no_main]

use kagari_analyzer::verify_session;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    verify_session(data);
});
