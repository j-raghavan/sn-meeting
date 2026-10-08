# Third-party components

## Bundled fonts

| Font | Source | License | Styles |
|------|--------|---------|--------|
| Pinyon Script | github.com/SorkinType/Pinyon (Google Fonts) | SIL Open Font License 1.1 | Regular |

Pinyon Script is bundled at `app/src/main/assets/fonts/pinyon_script.ttf` for the "Meetings"
wordmark and footer tagline. © The Pinyon Script Project Authors; SIL OFL 1.1
(http://scripts.sil.org/OFL) — free to use, bundle, and redistribute with attribution.

## Icons

| Icon | Source | License |
|------|--------|---------|
| `res/drawable/ic_settings.xml` (gear) | Material Design Icons (Google) | Apache License 2.0 |

## Rust crates linked into `libmeeting.so`

The main crates and their `license` fields as published on crates.io (checked with
`cargo metadata` against the Android target with `jni-bridge` enabled). Their transitive
dependencies are permissively licensed (MIT / Apache-2.0 / ISC / BSD style).

| Crate | Version | License |
|-------|---------|---------|
| rig-core, rig-reqwest, rig-http | 0.44.0 | MIT |
| reqwest | 0.13 | MIT OR Apache-2.0 |
| hyper | 1 | MIT |
| tokio | 1 | MIT |
| rustls | 0.23 | Apache-2.0 OR ISC OR MIT |
| rustls-webpki | 0.103 | ISC |
| aws-lc-rs | 1 | ISC AND (Apache-2.0 OR ISC) |
| aws-lc-sys | 0.45 | ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0) |
| webpki-root-certs (Mozilla's root store, as DER) | 1 | CDLA-Permissive-2.0 |
| serde, serde_json | 1 | MIT OR Apache-2.0 |
| thiserror | 2 | MIT OR Apache-2.0 |
| jni | 0.22.4 | MIT OR Apache-2.0 |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| iana-time-zone, android_system_properties | 0.1 | MIT OR Apache-2.0 |
| num-traits | 0.2 | MIT OR Apache-2.0 |
| minijinja (template layouts, F11) | 2.24.0 | Apache-2.0 |
| memo-map (minijinja dependency) | 0.3 | Apache-2.0 |
| uuid (to-do ids) | 1 | Apache-2.0 OR MIT |
