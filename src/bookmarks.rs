//! 登録済み再生リスト（ブックマーク）の原子的な保存と検証付き読み込み。
//! URLとリスト名のみを永続化し、1件の不正行があっても他の行を保持する。

use crate::urls;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const MAX_BOOKMARKS: usize = 200;
const MAX_TITLE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
	pub title: String,
	pub url: String,
}

/// 一時ファイルへ書いた後、パーミッション600で置換する原子的保存。
pub fn save(path: &Path, rows: &[Bookmark]) -> io::Result<()> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let json = serde_json::to_string_pretty(rows)
		.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
	let (tmp, mut file) = loop {
		let tmp = path.with_file_name(format!(
			".eco-bookmarks-{:032x}.tmp",
			rand::random::<u128>()
		));
		match OpenOptions::new()
			.write(true)
			.create_new(true)
			.mode(0o600)
			.open(&tmp)
		{
			Ok(file) => break (tmp, file),
			Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
			Err(e) => return Err(e),
		}
	};
	let result = (|| {
		file.write_all(json.as_bytes())?;
		fs::rename(&tmp, path)
	})();
	if result.is_err() {
		let _ = fs::remove_file(&tmp);
	}
	result
}

/// 読み込みに失敗した場合は空のリストを返す。
/// 不正な行（URL検証不通・必須項目なし）はその行だけ読み飛ばす。
pub fn read(path: &Path) -> Vec<Bookmark> {
	let data = fs::read_to_string(path).unwrap_or_default();
	let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) else {
		return Vec::new();
	};
	let Some(rows) = parsed.as_array() else {
		return Vec::new();
	};
	rows
		.iter()
		.take(MAX_BOOKMARKS)
		.filter_map(|row| {
			let title = row.get("title")?.as_str()?;
			let url = urls::playlist_url(row.get("url")?.as_str()?).ok()?;
			Some(Bookmark {
				title: title.chars().take(MAX_TITLE_CHARS).collect(),
				url,
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::os::unix::fs::{PermissionsExt, symlink};

	#[test]
	fn concurrent_saves_are_complete_and_leave_no_tempfiles() {
		let dir = std::env::temp_dir().join(format!("eco-concurrent-{:032x}", rand::random::<u128>()));
		fs::create_dir(&dir).unwrap();
		let path = dir.join("bookmarks.json");
		let barrier = std::sync::Barrier::new(8);
		std::thread::scope(|scope| {
			for i in 0..8 {
				let (path, barrier) = (&path, &barrier);
				scope.spawn(move || {
					let rows = vec![
						Bookmark {
							title: format!("writer {i}"),
							url: "https://www.youtube.com/playlist?list=PL123".into(),
						};
						MAX_BOOKMARKS
					];
					barrier.wait();
					for _ in 0..20 {
						save(path, &rows).unwrap();
						let saved = read(path);
						assert_eq!(saved.len(), MAX_BOOKMARKS);
						assert!(saved.iter().all(|row| row == &saved[0]));
						assert_eq!(
							fs::metadata(path).unwrap().permissions().mode() & 0o777,
							0o600
						);
					}
				});
			}
		});
		assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
		fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn save_ignores_old_temp_symlink_and_cleans_up_failed_rename() {
		let dir = std::env::temp_dir().join(format!("eco-temp-{:032x}", rand::random::<u128>()));
		fs::create_dir(&dir).unwrap();
		let path = dir.join("bookmarks.json");
		let victim = dir.join("victim");
		fs::write(&victim, "untouched").unwrap();
		symlink(&victim, path.with_extension("tmp")).unwrap();
		save(&path, &[]).unwrap();
		assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
		fs::remove_file(&path).unwrap();
		fs::create_dir(&path).unwrap();
		assert!(save(&path, &[]).is_err());
		assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);
		fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn atomic_bookmarks_roundtrip() {
		let dir = std::env::temp_dir().join(format!("eco-test-{}", std::process::id()));
		let path = dir.join("a/b.json");
		let rows = vec![Bookmark {
			title: "日本語タイトル".into(),
			url: "https://www.youtube.com/playlist?list=PL123".into(),
		}];
		save(&path, &rows).unwrap();
		assert_eq!(read(&path), rows);
		let perms = fs::metadata(&path).unwrap().permissions();
		assert_eq!(perms.mode() & 0o777, 0o600);
		fs::remove_dir_all(&dir).ok();
	}

	#[test]
	fn corrupt_file_returns_empty() {
		let dir = std::env::temp_dir().join(format!("eco-test-corrupt-{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("broken.json");
		fs::write(&path, "{").unwrap();
		assert!(read(&path).is_empty());
		fs::remove_dir_all(&dir).ok();
	}

	#[test]
	fn invalid_rows_are_skipped_but_valid_kept() {
		// 1行の不正データで全ブックマークが失われる従来実装の欠陥を修正した挙動
		let dir = std::env::temp_dir().join(format!("eco-test-skip-{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("mixed.json");
		let json = format!(
			r#"[{{"title": "ok", "url": "https://www.youtube.com/playlist?list=PLok"}},
                {{"title": "no url"}},
                {{"title": "bad scheme", "url": "http://youtube.com/playlist?list=PLx"}},
                {{"title": "{}", "url": "https://www.youtube.com/playlist?list=PLlong"}},
                {{"title": "not object"}}]"#,
			"あ".repeat(300)
		);
		fs::write(&path, json).unwrap();
		let rows = read(&path);
		assert_eq!(rows.len(), 2);
		assert_eq!(rows[0].title, "ok");
		assert_eq!(rows[1].title.chars().count(), 200); // タイトルは200文字で切る
		fs::remove_dir_all(&dir).ok();
	}
}
