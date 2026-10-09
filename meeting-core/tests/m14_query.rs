//! F19-FR9: a handwritten query strip goes to the summarise provider in one small call (fixed
//! instruction, max_tokens 64, the image on the wire), is recorded as `kind: "query"` with no
//! meeting, and shows in nativeSpend as Search. Without a key, network or timely reply it says
//! so, and the shell falls back to the keyboard.
mod libcommon;

use libcommon::*;
use meeting::api::Core;
use meeting::app::App;
use meeting::providers::Endpoints;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A PNG header claiming `w` × `h`.
fn strip(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    b.extend(b"IHDR");
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 0, 0, 0, 0, 1, 2, 3, 4]);
    b
}

struct Fx {
    _e: Env,
    rt: Runtime,
    s: MockServer,
    app: App,
}

fn fixture(with_key: bool, timeout: Duration) -> Fx {
    let rt = Runtime::new().unwrap();
    let s = rt.block_on(MockServer::start());
    let e = Env::new();
    let key_dir = e.data.path().join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    if with_key {
        std::fs::write(
            key_dir.join("copilot-key-openai.txt"),
            "provider=openai\nkey=sk-test-SECRET-q\nmodel=gpt-4o-mini\n",
        )
        .unwrap();
    }
    let uri = s.uri();
    let core = Core {
        app_data_dir: e.data.path().into(),
        key_dir,
        endpoints: Endpoints {
            openai: format!("{uri}/v1"),
            gemini: uri.clone(),
            anthropic: uri,
        },
        timeout,
        https_only: false,
    };
    let app = App::with_tz(core, e.root.path().into(), Arc::new(tz(PDT)));
    Fx { _e: e, rt, s, app }
}

fn reply(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "resp_1", "object": "response", "created_at": 1, "status": "completed",
        "model": "gpt-4o-mini",
        "output": [{"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
            "content": [{"type": "output_text", "text": text, "annotations": []}]}],
        "usage": {"input_tokens": 300, "output_tokens": 3, "total_tokens": 303}
    }))
}

impl Fx {
    fn mount(&self, t: ResponseTemplate) {
        let m = Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(t);
        self.rt.block_on(m.mount(&self.s));
    }
    fn bodies(&self) -> Vec<Value> {
        let r = self.rt.block_on(self.s.received_requests()).unwrap();
        r.iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
}

#[test]
fn a_written_query_is_read_in_one_small_call_and_counted_as_search() {
    let fx = fixture(true, Duration::from_secs(5));
    fx.mount(reply("  \"budget\"\nextra line"));
    let png = strip(1568, 200);
    let r = parse(&fx.app.recognise_query_json(&png));
    assert_eq!(r, json!({"text": "budget"}));
    let b = &fx.bodies()[0];
    assert_eq!(
        b["instructions"],
        "Return only the handwritten text, verbatim."
    );
    assert_eq!(b["max_output_tokens"], 64);
    assert!(
        b.get("text").is_none_or(|t| t.get("format").is_none()),
        "plain text: {b}"
    );
    use base64::Engine;
    let data = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    assert!(b.to_string().contains(&data), "the strip is on the wire");
    // F16: kind query, no meeting; Settings → SPEND shows it as Search.
    let usage: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.app.query_usage_path()).unwrap()).unwrap();
    assert_eq!(usage.as_array().unwrap().len(), 1);
    assert_eq!(usage[0]["kind"], "query");
    let month = &fx.app.query_usage()[0];
    let at = serde_json::to_value(month).unwrap()["at"].as_str().unwrap()[..7].to_owned();
    let s = parse(&fx.app.spend_json(&at, &at));
    let m = &s["months"][0];
    assert_eq!(m["searches"], 1);
    assert!(m["search"].as_f64().unwrap() > 0.0, "{m}");
    assert_eq!(m["usd"], m["search"]);
    assert_eq!(m["meetings"], 0);
    assert!(m["label"].as_str().unwrap().contains("spent"), "{m}");
}

#[test]
fn bad_strips_are_refused_before_any_call() {
    let fx = fixture(true, Duration::from_secs(5));
    for png in [
        b"not a png".to_vec(),
        strip(1569, 10),
        strip(10, 201),
        strip(0, 10),
    ] {
        let r = parse(&fx.app.recognise_query_json(&png));
        assert_eq!(r["offline"], false, "{r}");
        assert!(r["error"].is_string());
    }
    let mut big = strip(10, 10);
    big.resize(3 * 1024 * 1024, 0);
    assert_eq!(
        parse(&fx.app.recognise_query_json(&big))["error"],
        "the query image is too large"
    );
    assert!(fx.bodies().is_empty());
}

/// EC31: no key, no network or no reply in time: the box falls back to the keyboard.
#[test]
fn offline_no_key_and_timeouts_say_offline() {
    let fx = fixture(false, Duration::from_secs(5));
    let r = parse(&fx.app.recognise_query_json(&strip(100, 50)));
    assert_eq!(r["offline"], true, "{r}");

    let fx = fixture(true, Duration::from_millis(300));
    fx.mount(reply("budget").set_delay(Duration::from_secs(2)));
    let r = parse(&fx.app.recognise_query_json(&strip(100, 50)));
    assert_eq!(r, json!({"error": "Timed out", "offline": true}));

    let fx = fixture(true, Duration::from_secs(5));
    fx.mount(ResponseTemplate::new(400).set_body_string("bad request"));
    let r = parse(&fx.app.recognise_query_json(&strip(100, 50)));
    assert_eq!(r["offline"], false, "{r}");
    assert!(!r.to_string().contains("SECRET"));
    assert!(
        !fx.app.query_usage_path().exists(),
        "no reply, nothing counted (OQ16)"
    );

    // A cut-off reply was still billed: counted.
    let fx = fixture(true, Duration::from_secs(5));
    fx.mount(ResponseTemplate::new(200).set_body_json(json!({
        "id": "r", "object": "response", "created_at": 1, "status": "incomplete",
        "incomplete_details": {"reason": "max_output_tokens"}, "model": "gpt-4o-mini",
        "output": [], "usage": {"input_tokens": 300, "output_tokens": 64, "total_tokens": 364}})));
    let r = parse(&fx.app.recognise_query_json(&strip(100, 50)));
    assert!(r["error"].is_string(), "{r}");
    assert_eq!(fx.app.query_usage().len(), 1);
}

#[test]
fn the_record_list_is_capped_and_junk_reads_as_none() {
    let fx = fixture(true, Duration::from_secs(5));
    std::fs::write(fx.app.query_usage_path(), "junk").unwrap();
    assert!(fx.app.query_usage().is_empty());
    let one = json!({"id": "u", "at": "2026-10-09T10:00:00-07:00", "kind": "query",
        "provider": "openai", "model": "gpt-4o-mini", "billing": "tokens", "usd": 0.0001,
        "source": "provider", "estimated": false});
    let many: Vec<Value> = (0..meeting::app::MAX_QUERY_RECORDS)
        .map(|_| one.clone())
        .collect();
    std::fs::write(fx.app.query_usage_path(), json!(many).to_string()).unwrap();
    fx.mount(reply("budget"));
    fx.app.recognise_query_json(&strip(100, 50));
    assert_eq!(fx.app.query_usage().len(), meeting::app::MAX_QUERY_RECORDS);
}
