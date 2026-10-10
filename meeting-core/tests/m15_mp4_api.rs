//! F15-FR1 / FR8 (D14 revised 2026-10-09): the MP4 writer's exports need no nativeInit.
use meeting::api;
use serde_json::Value;

fn v(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn the_writer_exports_work_before_init() {
    let d = tempfile::tempdir().unwrap();
    let p = |n: &str| d.path().join(n).to_string_lossy().into_owned();
    let h = v(&api::mp4_open(
        &p("s.m4a"),
        16_000,
        1,
        &[0x14, 0x08],
        &p("s.idx"),
    ))["handle"]
        .as_i64()
        .unwrap();
    for i in 0..20u8 {
        assert_eq!(api::mp4_write(h, &[i; 30]), "");
    }
    assert_eq!(v(&api::mp4_close(h))["frames"], 20);
    assert_eq!(
        v(&api::mp4_cut(&p("s.m4a"), 0, 640, &p("c.m4a")))["frames"],
        10
    );
    assert_eq!(v(&api::mp4_abandon(-1))["ok"], false);
    let pieces = format!(r#"[{{"path": "{}", "skipMs": 0}}]"#, p("c.m4a"));
    assert_eq!(
        v(&api::mp4_rebuild(&p("r.m4a"), "", &pieces))["from"],
        "chunks"
    );
}
