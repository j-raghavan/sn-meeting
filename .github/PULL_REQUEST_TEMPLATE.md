## What & why

<!-- What does this change do, and why? -->

## Requirements

<!-- The FR / AC / INV / EC / NFR / ADR ids this implements or touches, e.g. F8-FR1, INV5. -->

-

## Checklist

- [ ] `./scripts/gates.sh` passes locally (CI runs the same gates)
- [ ] Every commit subject starts with its requirement id, e.g. `F8-FR1: …`
- [ ] A test fails without this change (bugs and new behaviour), or why not:
- [ ] `jni` stays out of the host graph (INV5); new linked crates are in `LICENSES-3RDPARTY.md`
- [ ] No secrets: no API keys, key files, keystores or `local.properties`
- [ ] No AI or co-author attribution in commits, the title or this description

## Device verification

<!-- What was checked on a Supernote, and what is still device-unverified. Host-only: say so. -->

- Verified on device:
- Not yet verified on device:
