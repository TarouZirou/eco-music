//! Eco Music / Linux: UI非依存の中核部品（yt-dlp抽出・ブックマーク・mpv IPC）。
//! 認証情報や音声データは永続化しない。

pub mod bookmarks;
pub mod extract;
pub mod player;
pub mod shuffle;
pub mod urls;

use serde_json::Value;

/// バックグラウンドスレッドからUIへ渡すイベント。
#[derive(Debug)]
pub enum EcoEvent {
	Ready,
	Mpv(Value),
	PlayerError(String),
	Loaded {
		token: u64,
		url: String,
		title: String,
		tracks: Vec<extract::Track>,
	},
	LoadError(String),
}
