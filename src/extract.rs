//! yt-dlpサブプロセスによる再生リスト取得。`core.py` の `extract_process` /
//! `load_playlist` 相当。曲数の上限は設けない（旧実装の2000曲制限を廃止）。

use crate::urls;
use serde_json::Value;
use std::fmt;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// 無制限に取得するためのタイムアウト。旧実装の150秒は2000曲基準だった。
pub const EXTRACT_TIMEOUT: Duration = Duration::from_secs(600);
pub const YT_DLP: &str = "yt-dlp";

#[derive(Debug)]
pub enum ExtractError {
	/// yt-dlpバイナリが存在しない
	NotFound,
	/// キャンセルされた
	Cancelled,
	/// タイムアウト
	Timeout,
	/// URLが不正
	Invalid(urls::UrlError),
	/// 取得に失敗した（終了コード・通信・内容のいずれか）
	Failed(&'static str),
}

impl fmt::Display for ExtractError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			ExtractError::NotFound => {
				f.write_str("yt-dlp がありません。READMEの依存パッケージを導入してください")
			}
			ExtractError::Cancelled => f.write_str("取得を中止しました"),
			ExtractError::Timeout => f.write_str("取得がタイムアウトしました"),
			ExtractError::Invalid(e) => f.write_str(&e.0),
			ExtractError::Failed(msg) => f.write_str(msg),
		}
	}
}

impl std::error::Error for ExtractError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
	pub title: String,
	pub url: String,
}

fn is_track_id(id: &str) -> bool {
	id.len() == 11
		&& id
			.bytes()
			.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 外部yt-dlpを呼び出して再生リストを全件取得する。
/// `cancel` が立つと子プロセスグループごと停止する。
pub fn load_playlist(
	value: &str,
	cancel: &AtomicBool,
) -> Result<(String, Vec<Track>), ExtractError> {
	let url = urls::playlist_url(value).map_err(ExtractError::Invalid)?;
	let mut cmd = Command::new(YT_DLP);
	cmd
		.args([
			"--ignore-config",
			"--flat-playlist",
			"--dump-single-json",
			"--skip-download",
			"--ignore-errors",
			"--socket-timeout",
			"15",
			"--retries",
			"2",
			"--",
		])
		.arg(&url);
	let stdout = run_extractor(&mut cmd, cancel, EXTRACT_TIMEOUT)?;
	parse_playlist(&stdout)
}

/// コマンドを独立プロセスグループで実行し、キャンセル・タイムアウト時に
/// グループごと確実に停止して標準出力を返す。
pub fn run_extractor(
	cmd: &mut Command,
	cancel: &AtomicBool,
	timeout: Duration,
) -> Result<String, ExtractError> {
	use std::os::unix::process::CommandExt;
	cmd
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.process_group(0);
	let mut child = match cmd.spawn() {
		Ok(c) => c,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ExtractError::NotFound),
		Err(_) => {
			return Err(ExtractError::Failed(
				"yt-dlp を起動できません。インストールと実行権限を確認してください",
			));
		}
	};
	let mut stdout_pipe = child.stdout.take().expect("piped stdout");
	let mut stderr_pipe = child.stderr.take().expect("piped stderr");
	let out_thread = std::thread::spawn(move || {
		let mut buf = Vec::new();
		let _ = stdout_pipe.read_to_end(&mut buf);
		buf
	});
	// stderrは破棄するが、パイプが詰まらないよう読み切りスレッドを起こす
	let err_thread = std::thread::spawn(move || {
		let mut buf = Vec::new();
		let _ = stderr_pipe.read_to_end(&mut buf);
	});

	let deadline = Instant::now() + timeout;
	let status = loop {
		if cancel.load(Ordering::Relaxed) {
			kill_group(&mut child, true);
			let _ = out_thread.join();
			let _ = err_thread.join();
			return Err(ExtractError::Cancelled);
		}
		match child.try_wait() {
			Ok(Some(status)) => break status,
			Ok(None) => {}
			Err(_) => {
				kill_group(&mut child, true);
				let _ = out_thread.join();
				let _ = err_thread.join();
				return Err(ExtractError::Failed("yt-dlp の実行に失敗しました"));
			}
		}
		if Instant::now() >= deadline {
			kill_group(&mut child, false);
			let _ = out_thread.join();
			let _ = err_thread.join();
			return Err(ExtractError::Timeout);
		}
		std::thread::sleep(Duration::from_millis(100));
	};
	let stdout = out_thread.join().unwrap_or_default();
	let _ = err_thread.join();
	if status.success() {
		Ok(String::from_utf8_lossy(&stdout).into_owned())
	} else {
		Err(ExtractError::Failed(
			"再生リストを取得できません。公開設定・通信・yt-dlpの更新を確認してください。",
		))
	}
}

/// SIGTERM→最大1秒待機→SIGKILL でプロセスグループを停止する。
fn kill_group(child: &mut std::process::Child, immediate: bool) {
	let pid = child.id() as libc::pid_t;
	unsafe {
		if immediate {
			libc::kill(-pid, libc::SIGKILL);
		} else {
			libc::kill(-pid, libc::SIGTERM);
		}
	}
	let deadline = Instant::now() + Duration::from_secs(1);
	while child.try_wait().map(|s| s.is_none()).unwrap_or(false) {
		if Instant::now() >= deadline {
			unsafe {
				libc::kill(-pid, libc::SIGKILL);
			}
			break;
		}
		std::thread::sleep(Duration::from_millis(50));
	}
	let _ = child.wait();
}

/// yt-dlpの `--dump-single-json` 出力を解析する（純関数・上限なし）。
pub fn parse_playlist(json: &str) -> Result<(String, Vec<Track>), ExtractError> {
	let failed = ExtractError::Failed("再生リストを取得できません。yt-dlpの更新を確認してください。");
	let Ok(data) = serde_json::from_str::<Value>(json) else {
		return Err(failed);
	};
	let title = data
		.get("title")
		.and_then(Value::as_str)
		.filter(|s| !s.is_empty())
		.unwrap_or("Playlist")
		.to_string();
	let mut tracks = Vec::new();
	if let Some(entries) = data.get("entries").and_then(Value::as_array) {
		for entry in entries {
			let Some(id) = entry.get("id").and_then(Value::as_str) else {
				continue;
			};
			if !is_track_id(id) {
				continue;
			}
			let title = entry
				.get("title")
				.and_then(Value::as_str)
				.filter(|s| !s.is_empty())
				.unwrap_or(id)
				.to_string();
			tracks.push(Track {
				title,
				url: format!("https://www.youtube.com/watch?v={id}"),
			});
		}
	}
	if tracks.is_empty() {
		return Err(ExtractError::Failed(
			"再生可能な曲がありません。非公開リスト・ログイン必須の曲は未対応です。",
		));
	}
	Ok((title, tracks))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_filters_invalid_entries() {
		let json = r#"{"title": "Test", "entries": [
            null,
            {"id": "abcdefghijk", "title": "Song"},
            {"id": "file:///etc/passwd"},
            {"id": 123},
            {"title": "no id"}
        ]}"#;
		let (title, tracks) = parse_playlist(json).unwrap();
		assert_eq!(title, "Test");
		assert_eq!(tracks.len(), 1);
		assert_eq!(tracks[0].url, "https://www.youtube.com/watch?v=abcdefghijk");
		assert_eq!(tracks[0].title, "Song");
	}

	#[test]
	fn parse_falls_back_to_id_for_missing_title() {
		let json = r#"{"entries": [{"id": "abcdefghijk", "title": ""}, {"id": "12345678901"}]}"#;
		let (title, tracks) = parse_playlist(json).unwrap();
		assert_eq!(title, "Playlist");
		assert_eq!(tracks[0].title, "abcdefghijk");
		assert_eq!(tracks[1].title, "12345678901");
	}

	#[test]
	fn empty_list_reports_failure() {
		assert!(parse_playlist(r#"{"entries": []}"#).is_err());
		assert!(parse_playlist("{}").is_err());
	}

	#[test]
	fn unlimited_playlist_keeps_every_entry() {
		// 2000曲超えのリストが丸ごと保持されることを検証する（旧実装は2000で打ち切り）
		let entries: Vec<String> = (0..5000)
			.map(|i| format!(r#"{{"id": "{:011}", "title": "Song {}"}}"#, i, i))
			.collect();
		let json = format!(r#"{{"title": "Big", "entries": [{}]}}"#, entries.join(","));
		let (title, tracks) = parse_playlist(&json).unwrap();
		assert_eq!(title, "Big");
		assert_eq!(tracks.len(), 5000);
		assert_eq!(
			tracks[4999].url,
			"https://www.youtube.com/watch?v=00000004999"
		);
	}
}
