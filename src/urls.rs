//! 再生リストURLの正規化と検証。`core.py` の `playlist_url` 相当。

use std::fmt;

const HOSTS: [&str; 4] = [
	"music.youtube.com",
	"www.youtube.com",
	"youtube.com",
	"m.youtube.com",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlError(pub String);

impl fmt::Display for UrlError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

impl std::error::Error for UrlError {}

fn err_untrusted() -> UrlError {
	UrlError("https://music.youtube.com/playlist?list=… を入力してください".into())
}

fn err_no_id() -> UrlError {
	UrlError("再生リストID（list=…）がありません".into())
}

fn err_radio() -> UrlError {
	UrlError("自動生成ラジオではなく、固定の再生リストを指定してください".into())
}

/// `%XX` 形式のパーセントエンコードのみをデコードする（不正なUTF-8は置換文字になる）。
fn percent_decode(value: &str) -> String {
	let bytes = value.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%' && i + 2 < bytes.len() {
			let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
			match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
				(Some(h), Some(l)) => {
					out.push(h * 16 + l);
					i += 3;
				}
				_ => {
					out.push(bytes[i]);
					i += 1;
				}
			}
		} else {
			out.push(bytes[i]);
			i += 1;
		}
	}
	String::from_utf8_lossy(&out).into_owned()
}

fn is_playlist_id(id: &str) -> bool {
	(2..=200).contains(&id.chars().count())
		&& id
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 入力を検証し、正規形 `https://www.youtube.com/playlist?list=<ID>` を返す。
pub fn playlist_url(value: &str) -> Result<String, UrlError> {
	let trimmed = value.trim();
	let parsed = url::Url::parse(trimmed).map_err(|_| err_untrusted())?;
	if parsed.scheme() != "https"
		|| !parsed.host_str().is_some_and(|h| HOSTS.contains(&h))
		|| !parsed.username().is_empty()
		|| parsed.password().is_some()
		|| parsed.port().is_some_and(|p| p != 443)
	{
		return Err(err_untrusted());
	}
	let mut ids = Vec::new();
	for pair in parsed.query().unwrap_or_default().split('&') {
		let (key, val) = pair.split_once('=').unwrap_or((pair, ""));
		if percent_decode(key) == "list" {
			ids.push(percent_decode(val));
		}
	}
	if ids.len() != 1 {
		return Err(err_no_id());
	}
	let id = &ids[0];
	if !is_playlist_id(id) {
		return Err(err_no_id());
	}
	if id.starts_with("RD") || id.starts_with("UL") {
		return Err(err_radio());
	}
	Ok(format!("https://www.youtube.com/playlist?list={id}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn canonical_url() {
		assert_eq!(
			playlist_url("https://music.youtube.com/watch?v=123&list=PLabc-12_3").unwrap(),
			"https://www.youtube.com/playlist?list=PLabc-12_3"
		);
		assert_eq!(
			playlist_url("  https://m.youtube.com/playlist?list=PLx  ").unwrap(),
			"https://www.youtube.com/playlist?list=PLx"
		);
		// 明示的な443ポートとパーセントエンコードされたIDは許可する
		assert_eq!(
			playlist_url("https://www.youtube.com:443/playlist?list=PLa%2Db").unwrap(),
			"https://www.youtube.com/playlist?list=PLa-b"
		);
	}

	#[test]
	fn reject_untrusted_inputs() {
		for url in [
			"http://youtube.com/playlist?list=PL12",
			"https://youtube.com.evil.test/?list=PL12",
			"https://evil@youtube.com/?list=PL12",
			"https://youtube.com:8443/?list=PL12",
			"file:///tmp/foo",
			"--exec=oops",
			"https://youtube.com/?list=PL1&list=PL2",
			"https://youtube.com/?list=RD123",
			"https://youtube.com/?list=ULxyz",
			"https://youtube.com/?list=A",
			"https://youtube.com/?list=",
			"https://youtube.com/",
			"",
		] {
			assert!(playlist_url(url).is_err(), "should reject: {url}");
		}
	}
}
