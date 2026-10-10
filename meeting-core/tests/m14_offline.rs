//! F19-FR10: Search never calls a provider except to read a handwritten query (F19-FR9).
//! Indexing, rebuilding and searching run on the local index only, with keys present and every
//! endpoint pointed at a server that records what reaches it.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::App;
use meeting::providers::Endpoints;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use wiremock::MockServer;

#[test]
fn indexing_and_searching_reach_no_provider() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let keys = e.data.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    for (p, k) in [
        ("openai", "sk-a"),
        ("gemini", "AIza-b"),
        ("anthropic", "sk-ant-c"),
    ] {
        let body = format!("provider={p}\nkey={k}\nmodel=m\n");
        std::fs::write(keys.join(format!("copilot-key-{p}.txt")), body).unwrap();
    }
    let uri = s.uri();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir: keys,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.clone(),
            anthropic: uri,
        },
        timeout: Duration::from_secs(2),
        https_only: false,
    };
    let app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    let b = begin(&app, T0);
    let id = b["id"].as_str().unwrap().to_owned();
    app.finish_json(&id, 1_000, "");
    let dir = e.export();
    let mut sc = read_sidecar(&dir, &id);
    sc.summary = Some("Budget".into());
    meeting::sidecar::write(&dir, &sc).unwrap();
    app.index_update_json();
    app.rebuild_index_json();
    app.index_status_json();
    let r = parse(&app.search_json("budget", "all", 0));
    assert_eq!(r["totalGroups"], 1);
    for f in ["notes", "transcript", "minutes", "todos", "events"] {
        app.search_json("budg", f, 0);
    }
    let reqs = rt.block_on(s.received_requests()).unwrap();
    assert!(
        reqs.is_empty(),
        "{} requests reached a provider",
        reqs.len()
    );
}

/// The search module calls no provider code (it only reads the no-speech placeholder text).
#[test]
fn the_search_module_has_no_provider_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/search");
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let src = std::fs::read_to_string(&p).unwrap();
        for banned in [
            "complete_json",
            "complete_text",
            "providers::transcribe",
            "http_client",
            "Endpoints",
            "rig_core",
            "reqwest",
        ] {
            assert!(!src.contains(banned), "{} uses {banned}", p.display());
        }
    }
}
