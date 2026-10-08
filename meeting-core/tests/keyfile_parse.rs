//! F4-FR1 / F4-AC1: parity with sn-copilot's parser (keyFiles.ts, __tests__/keyFiles.test.ts).
use meeting::keyfile::{FileMatch, KeyError, Provider, match_key_filename, parse};

const A: Provider = Provider::Anthropic;
const O: Provider = Provider::OpenAi;
const G: Provider = Provider::Gemini;

/// (input, provider, Ok((key, model)) | Err(reason)) — the reason text is sn-copilot's.
#[allow(clippy::type_complexity)]
#[rustfmt::skip]
const PARITY: &[(&str, Provider, Result<(&str, &str), &str>)] = &[
    // valid, all three providers
    ("provider=anthropic\nkey=sk-ant-x\nmodel=claude-haiku-4-5\n", A, Ok(("sk-ant-x", "claude-haiku-4-5"))),
    ("provider=openai\nkey=sk-1\nmodel=gpt-4o", O, Ok(("sk-1", "gpt-4o"))),
    ("provider=gemini\nkey=AIza-1\nmodel=gemini-z\n", G, Ok(("AIza-1", "gemini-z"))),
    // whitespace around values and around `=`; tabs
    ("provider = anthropic\nkey =  sk-ant-x  \nmodel=\thaiku\t\n", A, Ok(("sk-ant-x", "haiku"))),
    // CRLF, mixed LF/CRLF, lone CR
    ("provider=openai\r\nkey=k\r\nmodel=m\r\n", O, Ok(("k", "m"))),
    ("provider=openai\nkey=k\r\nmodel=m\n", O, Ok(("k", "m"))),
    ("provider=openai\rkey=k\rmodel=m", O, Ok(("k", "m"))),
    // a UTF-8 BOM is trimmed like JS trim()
    ("\u{feff}provider=openai\nkey=k\nmodel=m", O, Ok(("k", "m"))),
    // comment lines (also indented) and blank lines
    ("# a comment\n\n   # indented\nprovider=openai\n\n\nkey=k\nmodel=m", O, Ok(("k", "m"))),
    // a trailing `#` and quotes are part of the value
    ("provider=openai\nkey=sk # not stripped\nmodel=m", O, Ok(("sk # not stripped", "m"))),
    ("provider=openai\nkey=\"sk\"\nmodel=m", O, Ok(("\"sk\"", "m"))),
    // split on the first `=` only
    ("provider=openai\nkey=ab=cd==\nmodel=m", O, Ok(("ab=cd==", "m"))),
    // duplicates: last wins
    ("provider=openai\nkey=first\nmodel=m\nkey=second", O, Ok(("second", "m"))),
    // malformed lines, `=x` (empty key) and unknown keys are ignored
    ("provider=openai\njunk line\n=x\ntemperature=1\nmode=image\nkey=k\nmodel=m", O, Ok(("k", "m"))),
    // missing fields, in sn-copilot's check order
    ("key=k\nmodel=m", O, Err("missing required field: provider")),
    ("provider=\nkey=k\nmodel=m", O, Err("missing required field: provider")),
    ("provider=openai\nmodel=m", O, Err("missing required field: key")),
    ("provider=openai\nkey=\nmodel=m", O, Err("missing required field: key")),
    ("provider=openai\nkey=k", O, Err("missing required field: model")),
    ("provider=openai\nkey=k\nmodel=  ", O, Err("missing required field: model")),
    ("", O, Err("missing required field: provider")),
    // `provider=` must be the canonical id: no aliases, case-sensitive; checked before key
    ("provider=claude\nkey=k\nmodel=m", A, Err("provider=\"claude\" mismatches filename suffix (expected canonical \"anthropic\")")),
    // Only a plausible id ([a-z-]{1,32}) is echoed; anything else could be the key itself.
    ("provider=OpenAI\nkey=k\nmodel=m", O, Err("provider=\"<invalid>\" mismatches filename suffix (expected canonical \"openai\")")),
    ("provider=openai key=sk-1 model=m", O, Err("provider=\"<invalid>\" mismatches filename suffix (expected canonical \"openai\")")),
    ("provider=sk-proj-SECRET9\nkey=k\nmodel=m", O, Err("provider=\"<invalid>\" mismatches filename suffix (expected canonical \"openai\")")),
    ("provider=openai\nmodel=m", A, Err("provider=\"openai\" mismatches filename suffix (expected canonical \"anthropic\")")),
];

#[test]
fn parity_table() {
    for (i, (text, provider, want)) in PARITY.iter().enumerate() {
        let got = parse(text, *provider);
        match (want, &got) {
            (Ok((key, model)), Ok(k)) => {
                assert_eq!((k.expose(), k.model.as_str()), (*key, *model), "row {i}");
                assert_eq!(k.provider, *provider, "row {i}");
            }
            (Err(reason), Err(e)) => assert_eq!(e.to_string(), *reason, "row {i}"),
            _ => panic!("row {i}: want {want:?}, got {got:?}"),
        }
    }
}

#[test]
fn mismatch_and_missing_are_typed() {
    assert_eq!(
        parse("provider=gemini\nkey=k\nmodel=m", O),
        Err(KeyError::ProviderMismatch {
            expected: "openai",
            found: "gemini".into()
        })
    );
    assert_eq!(
        parse("provider=openai", O),
        Err(KeyError::MissingField("key"))
    );
}

#[test]
fn transcribe_model_is_optional() {
    let k = parse("provider=gemini\nkey=k\nmodel=m\ntranscribe_model=g-t\n", G).unwrap();
    assert_eq!(k.transcribe_model.as_deref(), Some("g-t"));
    let k = parse("provider=gemini\nkey=k\nmodel=m\ntranscribe_model=\n", G).unwrap();
    assert_eq!(k.transcribe_model, None);
    assert_eq!(k.file_name(), "", "parsed text has no source file");
}

#[test]
fn debug_redacts_the_key() {
    let k = parse("provider=openai\nkey=sk-SECRET-123\nmodel=m", O).unwrap();
    let d = format!("{k:?}");
    assert!(!d.contains("sk-SECRET-123"), "{d}");
    assert!(
        d.contains("[redacted]") && d.contains("model: \"m\""),
        "{d}"
    );
}

#[test]
fn filename_aliases() {
    let table = [
        ("copilot-key-anthropic.txt", A),
        ("copilot-key-claude.txt", A),
        ("copilot-key-claude-ai.txt", A),
        ("copilot-key-claude-anthropic.txt", A),
        ("copilot-key-openai.txt", O),
        ("copilot-key-gpt.txt", O),
        ("copilot-key-chatgpt.txt", O),
        ("copilot-key-gemini.txt", G),
        ("copilot-key-google.txt", G),
        ("copilot-key-google-gemini.txt", G),
        // case-insensitive and trimmed
        ("Copilot-Key-OpenAI.txt", O),
        ("COPILOT-KEY-CLAUDE.TXT", A),
        ("  copilot-key-gemini.txt  ", G),
    ];
    for (name, p) in table {
        assert_eq!(
            match_key_filename(name),
            Some(FileMatch::Provider(p)),
            "{name}"
        );
    }
    assert_eq!(
        match_key_filename("copilot-key-deepseek.txt"),
        Some(FileMatch::Ignored)
    );
    for name in [
        "random.txt",
        "copilot-key-grok.txt",
        "copilot-key-.txt",
        "key-anthropic.txt",
        "copilot-key-claude.json",
        "",
    ] {
        assert_eq!(match_key_filename(name), None, "{name:?}");
    }
}

#[test]
fn provider_ids_round_trip() {
    for p in Provider::ALL {
        assert_eq!(Provider::from_id(p.id()), Some(p));
        assert_eq!(
            match_key_filename(&p.canonical_file_name()),
            Some(FileMatch::Provider(p))
        );
    }
    assert_eq!(
        Provider::OpenAi.canonical_file_name(),
        "copilot-key-openai.txt"
    );
    assert_eq!(
        Provider::from_id("claude"),
        None,
        "aliases are filenames only"
    );
    assert_eq!(Provider::from_id("deepseek"), None);
}
