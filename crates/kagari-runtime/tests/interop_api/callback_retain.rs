mod prototype;
use prototype::{LoanContext, Player, bind};
use std::sync::{Arc, Mutex};

fn main() {
    let saved: Arc<Mutex<Option<&'static Player>>> = Arc::new(Mutex::new(None));
    bind(move |_: &mut LoanContext, (p, _): (&Player, i32)| {
        *saved.lock().unwrap() = Some(p);
        Ok(())
    });
}
