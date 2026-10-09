//! The JNI surface can only be exercised on the device, so check here that every Kotlin
//! `external fun` in NativeBridge.kt has a matching export in jni.rs, with the same parameter
//! types, and the other way round. A mismatch is an UnsatisfiedLinkError (or worse) at runtime.
use std::collections::BTreeMap;
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// Kotlin parameter type → the jni-rs type its export must take.
fn jni_type(kotlin: &str) -> &'static str {
    match kotlin {
        "String" => "JString",
        "Int" => "jint",
        "Long" => "jlong",
        "Boolean" => "jboolean",
        "ByteArray" => "JByteArray",
        other => panic!("unmapped Kotlin type {other}"),
    }
}

/// name → parameter types, from `external fun name(a: T, b: U): String`.
fn kotlin_externals() -> BTreeMap<String, Vec<String>> {
    let src = read("../app/src/main/java/dev/jraghavan/snmeeting/NativeBridge.kt");
    src.lines()
        .filter_map(|l| l.trim().strip_prefix("external fun "))
        .map(|rest| {
            let (name, args) = rest.split_once('(').unwrap();
            let args = args.split_once(')').unwrap().0;
            let types = args
                .split(',')
                .filter(|a| !a.trim().is_empty())
                .map(|a| jni_type(a.split_once(':').unwrap().1.trim()).to_owned())
                .collect();
            (name.to_owned(), types)
        })
        .collect()
}

/// name → parameter types after `env` and `_class`, from jni.rs.
fn rust_exports() -> BTreeMap<String, Vec<String>> {
    let src = read("src/jni.rs");
    let mut out = BTreeMap::new();
    for chunk in src
        .split("pub extern \"system\" fn Java_dev_jraghavan_snmeeting_NativeBridge_")
        .skip(1)
    {
        let name = chunk.split('<').next().unwrap().to_owned();
        let params = chunk
            .split_once('(')
            .unwrap()
            .1
            .split_once(") ->")
            .unwrap()
            .0;
        let types: Vec<String> = params
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .skip(2) // env, _class
            .map(|p| {
                let ty = p.split_once(':').unwrap().1.trim();
                ty.split('<').next().unwrap().to_owned()
            })
            .collect();
        out.insert(name, types);
    }
    out
}

#[test]
fn kotlin_and_rust_agree() {
    let kotlin = kotlin_externals();
    let rust = rust_exports();
    assert!(kotlin.contains_key("nativeVersion"), "parsed no externals");
    assert_eq!(kotlin, rust);
}
