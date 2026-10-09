//! F1-FR9: building the provider client reads no system proxy settings. On macOS (the host the
//! gates run on) that lookup can stall for seconds inside the first build of a process, eating
//! a job's budget; on Android reqwest would only read proxy environment variables, which an app
//! never has. This binary's first build must be quick.
use meeting::providers::http_client;
use std::time::{Duration, Instant};

#[test]
fn the_first_client_builds_quickly() {
    let t = Instant::now();
    http_client(Duration::from_secs(5), false).unwrap();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}
