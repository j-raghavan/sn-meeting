//! F4-FR2 discovery and the F4-FR5 vault signal. Every test uses its own temp dir.
use meeting::keyfile::{KeyError, Provider, load};
use std::fs;
use std::path::Path;

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn not_found(vault_present: bool, provider: &'static str) -> Result<(), KeyError> {
    Err(KeyError::NotFound {
        provider,
        vault_present,
    })
}

fn load_unit(dir: &Path, p: Provider) -> Result<(), KeyError> {
    load(dir, p).map(|_| ())
}

#[test]
fn a_missing_or_empty_dir_is_not_found() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        load_unit(&d.path().join("nope"), Provider::OpenAi),
        not_found(false, "openai")
    );
    assert_eq!(
        load_unit(d.path(), Provider::Gemini),
        not_found(false, "gemini")
    );
    assert_eq!(
        KeyError::NotFound {
            provider: "gemini",
            vault_present: false
        }
        .to_string(),
        "no key file for gemini"
    );
}

#[test]
fn either_vault_file_is_reported() {
    for vault in ["copilot-key.enc", ".copilot-key.enc"] {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), vault, "ciphertext");
        assert_eq!(
            load_unit(d.path(), Provider::OpenAi),
            not_found(true, "openai"),
            "{vault}"
        );
    }
    // A directory with the vault's name is not a vault.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("copilot-key.enc")).unwrap();
    assert_eq!(
        load_unit(d.path(), Provider::OpenAi),
        not_found(false, "openai")
    );
}

#[test]
fn the_canonical_name_wins_over_aliases() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "copilot-key-claude.txt",
        "provider=anthropic\nkey=a1\nmodel=m",
    );
    write(
        d.path(),
        "copilot-key-anthropic.txt",
        "provider=anthropic\nkey=a2\nmodel=m",
    );
    let k = load(d.path(), Provider::Anthropic).unwrap();
    assert_eq!(
        (k.expose(), k.file_name().as_str()),
        ("a2", "copilot-key-anthropic.txt")
    );
    assert_eq!(k.source, d.path().join("copilot-key-anthropic.txt"));
}

#[test]
fn the_canonical_name_is_case_insensitive() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "copilot-key-chatgpt.txt",
        "provider=openai\nkey=o1\nmodel=m",
    );
    write(
        d.path(),
        "Copilot-Key-OpenAI.txt",
        "provider=openai\nkey=o2\nmodel=m",
    );
    assert_eq!(load(d.path(), Provider::OpenAi).unwrap().expose(), "o2");
}

#[test]
fn among_aliases_the_lexically_first_wins() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "copilot-key-gpt.txt",
        "provider=openai\nkey=gpt\nmodel=m",
    );
    write(
        d.path(),
        "copilot-key-chatgpt.txt",
        "provider=openai\nkey=chatgpt\nmodel=m",
    );
    assert_eq!(
        load(d.path(), Provider::OpenAi).unwrap().expose(),
        "chatgpt"
    );
}

#[test]
fn other_providers_deepseek_and_directories_are_skipped() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "copilot-key-deepseek.txt",
        "provider=deepseek\nkey=d\nmodel=m",
    );
    write(
        d.path(),
        "copilot-key-gemini.txt",
        "provider=gemini\nkey=g\nmodel=m",
    );
    write(d.path(), "notes.txt", "provider=openai\nkey=n\nmodel=m");
    fs::create_dir(d.path().join("copilot-key-openai.txt")).unwrap();
    assert_eq!(
        load_unit(d.path(), Provider::OpenAi),
        not_found(false, "openai")
    );
    assert_eq!(load(d.path(), Provider::Gemini).unwrap().expose(), "g");
}

#[test]
fn a_bad_file_reports_its_parse_error() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        "copilot-key-google.txt",
        "provider=gemini\nkey=g\n",
    );
    assert_eq!(
        load_unit(d.path(), Provider::Gemini),
        Err(KeyError::MissingField("model"))
    );
}

#[test]
fn the_size_cap_is_64_kib() {
    let d = tempfile::tempdir().unwrap();
    let body = "provider=openai\nkey=k\nmodel=m\n";
    let at_cap = format!("{body}{}", "#".repeat(64 * 1024 - body.len()));
    write(d.path(), "copilot-key-openai.txt", &at_cap);
    assert_eq!(load(d.path(), Provider::OpenAi).unwrap().expose(), "k");
    write(d.path(), "copilot-key-openai.txt", &format!("{at_cap}#"));
    assert_eq!(
        load_unit(d.path(), Provider::OpenAi),
        Err(KeyError::Unreadable("file too large".into()))
    );
}

#[test]
fn invalid_utf8_is_decoded_lossily() {
    let d = tempfile::tempdir().unwrap();
    let mut bytes = b"provider=openai\nkey=k\nmodel=m\nnote=".to_vec();
    bytes.extend([0xff, 0xfe, b'\n']);
    fs::write(d.path().join("copilot-key-openai.txt"), bytes).unwrap();
    assert_eq!(load(d.path(), Provider::OpenAi).unwrap().expose(), "k");
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_names_the_io_kind_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("copilot-key-openai.txt");
    fs::write(&path, "provider=openai\nkey=sk-SECRET\nmodel=m").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let got = load(d.path(), Provider::OpenAi);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let err = got.unwrap_err();
    assert_eq!(err, KeyError::Unreadable("permission denied".into()));
    assert_eq!(err.to_string(), "key file unreadable: permission denied");
}

#[cfg(unix)]
#[test]
fn an_unlistable_dir_is_unreadable_not_missing() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("SnCopilot");
    fs::create_dir(&dir).unwrap();
    write(
        &dir,
        "copilot-key-openai.txt",
        "provider=openai\nkey=k\nmodel=m",
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
    let got = load_unit(&dir, Provider::OpenAi);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(got, Err(KeyError::Unreadable("permission denied".into())));
}
