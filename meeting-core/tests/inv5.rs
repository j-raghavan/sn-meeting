//! INV5 / F1-AC1: without `jni-bridge`, `jni` is not in meeting-core's resolved graph.
use std::process::Command;

fn tree(features: &[&str]) -> String {
    let out = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "meeting-core",
            "-e",
            "normal",
            "-i",
            "jni",
            "--offline",
        ])
        .args(features)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo tree");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn jni_is_absent_from_the_host_graph() {
    // An absent package makes `cargo tree -i` print nothing (it errors "did not match").
    assert_eq!(tree(&[]), "");
    // Control: the Android feature does pull it in, so the empty result above is meaningful.
    assert!(tree(&["--features", "jni-bridge"]).contains("jni v0.22.4"));
}
