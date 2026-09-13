//! What one broadcast costs a full room, before and after serializing once.
use std::time::{Duration, Instant};

use palmcast::session::Registry;
use palmcast::wire::Frame;

const VIEWERS: usize = 400;

fn main() {
    let slide = "# Heading\n\nsome body text that makes the slide realistic\n\n---\n\n";
    let markdown = slide.repeat(400);
    let reg = Registry::new(Duration::from_secs(3600));
    let (id, _tok) = reg.create(&markdown).unwrap();
    let snapshot = reg.snapshot(&id).unwrap();
    let bytes = serde_json::to_string(&snapshot).unwrap().len();
    println!("deck payload: {} KB, room of {VIEWERS}\n", bytes / 1024);

    // Old path: every socket cloned the message, redacted its own copy, and
    // serialized it.
    let start = Instant::now();
    let mut sunk = 0usize;
    for _ in 0..VIEWERS {
        let payload = snapshot.redacted().unwrap();
        sunk += serde_json::to_string(&payload).unwrap().len();
    }
    let old = start.elapsed();

    // New path: serialize once, then each socket copies the string it needs.
    let start = Instant::now();
    let frame = Frame::new(&snapshot);
    let mut sunk2 = 0usize;
    for _ in 0..VIEWERS {
        sunk2 += frame.for_socket(false).unwrap().to_string().len();
    }
    let new = start.elapsed();

    println!("per socket clone + redact + serialize : {old:?}");
    println!("serialize once + per socket copy      : {new:?}");
    println!(
        "ratio                                 : {:.1}x",
        old.as_secs_f64() / new.as_secs_f64()
    );
    assert_eq!(sunk, sunk2, "the two paths must produce the same bytes");
    println!("\nboth paths produced identical bytes ({sunk} total)");
}
