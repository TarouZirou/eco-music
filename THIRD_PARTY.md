# Third-party dependencies

## Linux (Rust) アプリ

| Dependency | License | Source |
|---|---|---|
| egui / eframe 0.36 | MIT OR Apache-2.0 | https://github.com/emilk/egui/tree/0.36.1 |
| ehttp / winit ほかeguiの依存群 | MIT OR Apache-2.0 | https://github.com/emilk/egui/blob/0.36.1/crates/eframe/Cargo.toml |
| serde 1 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json 1 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| rand 0.10 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-random/rand |
| url 2 | MIT OR Apache-2.0 | https://github.com/servo/rust-url |
| libc 0.2 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| mpv (external, not bundled) | GPL-2.0-or-later or LGPL-2.1-or-later, depending on build | https://github.com/mpv-player/mpv |
| yt-dlp (external, not bundled) | Unlicense; distributions may include differently licensed dependencies | https://github.com/yt-dlp/yt-dlp |

## Android (Java) アプリ

| Dependency | License | Source |
|---|---|---|
| NewPipeExtractor v0.26.5 | GPL-3.0-or-later | https://github.com/TeamNewPipe/NewPipeExtractor/tree/v0.26.5 |
| AndroidX Media3 1.6.1 | Apache-2.0 | https://github.com/androidx/media/tree/1.6.1 |
| OkHttp 4.12.0 | Apache-2.0 | https://github.com/square/okhttp/tree/parent-4.12.0 |
| Gradle Wrapper | Apache-2.0 | https://github.com/gradle/gradle/tree/v8.11.1 |

NewPipeExtractor and Media3 pull transitive dependencies through Gradle.
Before public redistribution of a binary, include all applicable notices and corresponding source obligations.
Source code for Eco Music is provided with this deliverable. This build has no telemetry, advertising SDK or Google login.
