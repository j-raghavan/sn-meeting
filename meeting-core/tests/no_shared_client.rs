//! F1-FR8 / R9: every HTTP client is built in `providers::http_client`, with Mozilla roots. Any
//! default client goes through rustls-platform-verifier, which panics on Android. clippy.toml
//! bans the named constructors; trait impls (`Default`) can't be named there, so scan the source.
use std::fs;
use std::path::Path;

/// (pattern, files and functions where it is allowed). The allowlists are deliberately explicit.
const RULES: &[(&str, &[(&str, &str)])] = &[
    ("ReqwestClient::default", &[]),
    ("Client::default", &[]),
    ("Default::default()", &[]),
    (
        "Client::builder()",
        &[("providers.rs", "pub fn http_client(")],
    ),
];

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&p).unwrap())
        })
        .collect();
    out.sort();
    assert!(
        out.iter().any(|(n, _)| n == "providers.rs"),
        "scan found no sources"
    );
    out
}

/// The text of the function starting at `signature`, up to the next line that is just `}`.
fn function<'a>(src: &'a str, signature: &str) -> &'a str {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} not found"));
    let len = src[start..]
        .find("\n}\n")
        .map_or(src.len() - start, |n| n + 3);
    &src[start..start + len]
}

#[test]
fn clients_are_built_only_in_http_client() {
    let files = sources();
    for (pattern, allowed) in RULES {
        for (name, src) in &files {
            let mut allowed_hits = 0;
            for (file, signature) in allowed.iter().filter(|(f, _)| f == name) {
                allowed_hits += function(src, signature).matches(pattern).count();
                assert!(allowed_hits > 0, "{pattern} expected in {file} {signature}");
            }
            let hits = src.matches(pattern).count();
            assert_eq!(
                hits, allowed_hits,
                "{pattern} used outside its allowlist in src/{name}"
            );
        }
    }
}
