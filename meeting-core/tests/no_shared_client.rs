//! F1-FR8 / R9: every HTTP client is built in `providers::http_client`, with Mozilla roots. Any
//! default client goes through rustls-platform-verifier, which panics on Android. clippy.toml
//! bans the constructors it can name; trait impls (`Default`), aliased imports and registry
//! shortcuts can't all be named there, so scan every source file under meeting-core/src.
//! Ordinary struct defaults (`..Default::default()`) are not client construction and pass.
use std::fs;
use std::path::{Path, PathBuf};

/// Client-construction routes, and where each is allowed (file, enclosing fn). Explicit.
const RULES: &[(&str, &[(&str, &str)])] = &[
    ("ReqwestClient::default", &[]),
    ("Client::default", &[]),
    ("ClientBuilder::default", &[]),
    ("ClientBuilder::new", &[]),
    ("Client as Default>", &[]), // <reqwest::Client as Default>, <ReqwestClient as Default>
    ("reqwest::Client as", &[]), // use …::reqwest::Client as Http;
    ("ReqwestClient as", &[]),   // use rig_reqwest::ReqwestClient as Transport;
    ("providers::registry", &[]), // registry::connect / ProviderRef: the shared client
    ("registry::connect", &[]),
    (".completion_model()", &[]), // ProviderRef::completion_model, credentials from env
    (
        "Client::builder()",
        &[("providers.rs", "pub fn http_client(")],
    ),
];

/// Every `.rs` under `dir`, as (path relative to src, contents).
fn sources(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
    for e in fs::read_dir(dir).unwrap() {
        let p: PathBuf = e.unwrap().path();
        if p.is_dir() {
            sources(&p, base, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            out.push((rel, fs::read_to_string(&p).unwrap()));
        }
    }
}

/// The text of the function starting at `signature`, up to the next line that is just `}`.
fn function<'a>(src: &'a str, signature: &str) -> Option<&'a str> {
    let start = src.find(signature)?;
    let len = src[start..]
        .find("\n}\n")
        .map_or(src.len() - start, |n| n + 3);
    Some(&src[start..start + len])
}

/// Code only: comment lines are skipped so docs may name the routes they warn about.
fn code(src: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every rule `src` (file `name`) breaks.
fn violations(name: &str, src: &str) -> Vec<String> {
    let src = code(src);
    let mut out = Vec::new();
    for (pattern, allowed) in RULES {
        let allowed_hits: usize = allowed
            .iter()
            .filter(|(f, _)| *f == name)
            .filter_map(|(_, sig)| function(&src, sig))
            .map(|body| body.matches(pattern).count())
            .sum();
        if src.matches(pattern).count() > allowed_hits {
            out.push(format!("{pattern} in src/{name}"));
        }
    }
    out
}

#[test]
fn the_sources_build_clients_only_in_http_client() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&base, &base, &mut files);
    for nested in ["library/mod.rs", "settings/mod.rs", "providers.rs"] {
        assert!(
            files.iter().any(|(n, _)| n == nested),
            "scan missed {nested}"
        );
    }
    let providers = &files.iter().find(|(n, _)| n == "providers.rs").unwrap().1;
    assert!(
        function(providers, "pub fn http_client(")
            .unwrap()
            .contains("Client::builder()")
    );
    let bad: Vec<String> = files.iter().flat_map(|(n, s)| violations(n, s)).collect();
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn every_route_is_flagged() {
    let fixtures = [
        "let c = rig_reqwest::ReqwestClient::default();",
        "let c = reqwest::Client::default();",
        "let b = reqwest::ClientBuilder::default();",
        "let b = rig_reqwest::reqwest::ClientBuilder::new();",
        "let c = <reqwest::Client as Default>::default();",
        "let c = <ReqwestClient as Default>::default();",
        "use rig_reqwest::reqwest::Client as Http;",
        "use rig_reqwest::ReqwestClient as Transport;",
        "let m = rig_core::providers::registry::connect(\"openai/x\", k)?;",
        "use rig_core::providers::registry; let m = registry::connect(s, k)?;",
        "let m = r.completion_model()?;",
        "fn other() { let c = reqwest::Client::builder().build(); }",
    ];
    for f in fixtures {
        assert!(!violations("app.rs", f).is_empty(), "not flagged: {f}");
    }
    // Client::builder() is fine inside http_client in providers.rs, and only there.
    let ok = "pub fn http_client(t: u8) -> X {\n    reqwest::Client::builder().build()\n}\n";
    assert_eq!(violations("providers.rs", ok), Vec::<String>::new());
    assert!(!violations("library/mod.rs", ok).is_empty());
    let twice = format!("{ok}fn sneaky() {{ reqwest::Client::builder(); }}\n");
    assert!(!violations("providers.rs", &twice).is_empty());
}

#[test]
fn struct_defaults_and_comments_are_not_flagged() {
    for f in [
        "let sc = Sidecar { id, ..Default::default() };",
        "let s: Settings = Default::default();",
        "extra: Default::default(),",
        "#[derive(Default)] struct Liveness { jobs: HashSet<String> }",
        "/// Never call `reqwest::Client::default()`: it panics on Android.",
        "    // ReqwestClient::default would use the shared client",
        "let m = r.completion_model_with(key, rig_http(http));",
    ] {
        assert_eq!(violations("app.rs", f), Vec::<String>::new(), "{f}");
    }
}
