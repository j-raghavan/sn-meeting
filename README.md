<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/meetings-icon-dark.png">
    <img src=".github/assets/meetings-icon-light.png" alt="Meetings" width="120" height="120">
  </picture>
</p>

<h1 align="center">Meetings</h1>

<p align="center">
  <strong>Record meetings on your Supernote, then turn them into a transcript, a summary and a to-do list.</strong>
</p>

<p align="center">
  <a href="https://github.com/j-raghavan/sn-meeting/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/j-raghavan/sn-meeting/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="core: Rust" src="https://img.shields.io/badge/core-Rust-orange?logo=rust&logoColor=white">
  <img alt="platform: Supernote" src="https://img.shields.io/badge/platform-Supernote-blue">
  <a href="./LICENSE"><img alt="License: AGPL-3.0" src="https://img.shields.io/badge/license-AGPL--3.0-blue.svg"></a>
</p>

---

Meetings is an Android app for the Supernote (Android 11). Plug in a USB microphone, or connect a Bluetooth headset, and tap **＋ New**. Meetings records in the background while you take notes in another app. Afterwards it transcribes the recording, summarises it with a template you choose, and keeps every open to-do from every meeting on its home screen.

It is built the way [inkread](https://github.com/j-raghavan/inkread) is: a small Kotlin shell for the Android parts, around a Rust core that does everything else and is tested on a laptop with no device.

## Why Meetings?

- **A meeting recorder that suits e-ink.** It is black on white with no animations, and shows explicit cues ("SCROLL FOR MORE ↓") instead of motion. The screens follow inkread's look.
- **Your audio stays a file you own.** Recordings, transcripts and to-dos are plain files in `EXPORT/sn-meeting`, which Supernote Partner syncs to your computer. There is no database and no lock-in.
- **Bring your own AI provider.** Transcribe with OpenAI or Google Gemini, and summarise with OpenAI, Gemini or Anthropic. Meetings reuses the API keys you already keep on the device for SN-Copilot.
- **Built to survive real meetings.** Long recordings are split into 15-minute segments, so a crash loses at most one segment. Unplugging the mic ends the recording cleanly and keeps the audio. A crashed recording is recovered the next time the app opens.

## Status

Meetings is at **v0.1**. Everything below works and is checked by CI. On the device, recording, transcription and summaries, crash recovery and mic loss have been verified on a Supernote A5 X2 Manta.

Not yet verified on a device: recordings over 15 minutes (segment rollover), long screen-off recordings, and Bluetooth headsets.

### Roadmap

- **v1.1:** a follow-along transcript. Words highlight in step with playback, and you tap a line to jump there.
- **v2:** handwritten notes synced to the audio. Each meeting gets an infinite canvas, with your ink lighting up as the recording plays. The tools (pen, highlighter, eraser, lasso, undo, redo) sit in a docked bar, as in inkread.

## Quick start

1. **Install.** Download `meetings-<version>.apk` from [Releases](https://github.com/j-raghavan/sn-meeting/releases) and sideload it (`adb install -r meetings-<version>.apk`). Releases are signed with the Meetings release key, so after that Meetings updates itself: it checks for a newer release when it opens (at most every 6 hours) and offers to install it. A copy you build yourself is signed with your own debug key and can't update itself.
2. **Add a key.** Meetings reads keys from SN-Copilot's key files in `MyStyle/SnCopilot/`, for example `copilot-key-openai.txt`:
   ```
   provider=openai
   key=sk-...
   model=gpt-4o-mini
   ```
   The same format works for `copilot-key-gemini.txt` and `copilot-key-anthropic.txt`. Meetings never writes to these files and never shows the key.
3. **Allow access.** On first launch, grant **All files access**, so Meetings can save to `EXPORT/sn-meeting`. It asks for the microphone the first time you record.
4. **Record.** Plug in a USB microphone and tap **＋ New**. Tap **■ Stop** when the meeting ends.
5. **Transcribe.** Open the meeting and tap **Transcribe**. Meetings transcribes it, then summarises it with the meeting's template.

The Supernote has no usable built-in microphone, so Meetings records only from a USB microphone or a Bluetooth headset. The USB-C port can't be connected to a computer and a USB microphone at the same time; use adb over Wi-Fi for development (see Building).

## Using Meetings

### Home
Your five most recent meetings and every open to-do sit side by side. Tick a to-do to close it. The gear opens Settings.

### Recording
- **USB first, then Bluetooth,** or the microphone you choose in Settings. USB records at 44.1 kHz; Bluetooth headsets record at 16 kHz.
- Recording continues with the screen off or another app open. A notification shows the elapsed time and a Stop button.
- If the microphone is unplugged or the headset drops out, the recording stops, keeps what it captured, and notes "mic disconnected".

### Transcripts, summaries and to-dos
- **Transcribe** picks a transcription model your account can use, and stores each segment's text as it arrives. A cancelled or interrupted job resumes where it stopped.
- **Summarise again** re-runs the summary with the current template. To-dos you have ticked stay ticked, even if the model rewords them. **Transcribe again** redoes the transcript.
- Each meeting gets a title, a summary, its decisions, the template's sections and its to-dos, written to `<meeting>.transcript.md` beside the audio.

### Templates
Six built-in templates shape what the summary asks for and how the transcript file reads: General meeting, Stand-up, 1:1, Client call, Interview, and Lecture / talk notes. Edit one in the app, or as a file in `MyStyle/Meetings/templates/` on your computer. **Reset** brings back the original.

### Settings
Settings covers the microphone, the save folder, the transcription and summary providers (each with **Test connection**), the default template, and updates.

### Updates
**Settings → About → Check now** looks for a newer release on GitHub. When there is one, you see its notes and choose **Install**, **Skip this version** or **Later**. Install downloads the APK, checks it against the published SHA-256 and checks that it is signed with the same key as the installed app, then hands it to Android, which asks you to confirm. Android may first ask you to allow Meetings to install apps.

Two switches sit below: **Check for updates automatically** (on by default) and **Install updates automatically** (off by default; Android still asks you to confirm). Meetings never checks, downloads or installs while it is recording or transcribing, because installing restarts the app.

### Your files
For each meeting, in `EXPORT/sn-meeting` (or the folder you choose):

| File | What it is |
|---|---|
| `meeting-YYYYMMDD-HHMMSS-01.m4a` | the audio, one file per 15-minute segment |
| `meeting-YYYYMMDD-HHMMSS.json` | the meeting's details, transcript text and to-dos |
| `meeting-YYYYMMDD-HHMMSS.transcript.md` | the readable transcript and summary |

### Privacy
Audio is sent only to the transcription provider you choose, and transcripts only to the summary provider. This happens only when you tap Transcribe or Summarise. The first time, Meetings names the providers before anything is sent. Keys never leave the Rust core and never appear in logs or files. The update check asks GitHub for the latest release and sends nothing about your meetings; you can turn it off in Settings.

## Architecture

```
Kotlin shell (app/)          recording service, microphones, screens, in-app updates (GitHub Releases, HTTPS)
        │ JNI (JSON strings; never a key)
Rust core (meeting-core/)    keys, settings, meeting library, providers (rig-core), templates (minijinja)
        │ HTTPS (rustls, bundled roots, no redirects)
OpenAI · Google Gemini · Anthropic
```

The core builds and tests on a laptop. Provider calls are tested against local mock servers. The Android-only parts are kept thin and checked on the device.

## Building

You need Rust 1.95.0 (pinned in `rust-toolchain.toml`), `cargo-ndk`, the Android NDK, and JDK 21.

```bash
./scripts/gates.sh                    # fmt, clippy, tests, coverage ≥ 97 %, lint, JVM tests, script checks
./buildApk.sh                         # gates, then the Rust core for arm64, then the APK
./buildApk.sh --install --device 10.0.0.22:5555
```

`buildApk.sh --help` explains adb over Wi-Fi. The install step refuses to run while a recording is in progress.

A local build is signed with the debug key unless `MEETINGS_KEYSTORE_FILE`, `MEETINGS_KEYSTORE_PASSWORD`, `MEETINGS_KEY_ALIAS` and `MEETINGS_KEY_PASSWORD` are set. Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds the APK signed with the release key and publishes `meetings-<tag>.apk` and its `.sha256` as a GitHub Release.

## Contributing

Every pull request runs the full gates in CI, along with a commit check: subjects reference a requirement (for example `F5-FR2: …`) and carry no AI attribution. See the pull request template.

## About & disclaimer

Meetings is an independent project. It is not affiliated with or endorsed by Ratta (Supernote), OpenAI, Google or Anthropic. You use your own API keys and are bound by each provider's terms; providers may retain the audio and text you send them under their own policies.

## License

[AGPL-3.0-only](./LICENSE). Third-party notices are in [LICENSES-3RDPARTY.md](./LICENSES-3RDPARTY.md).
