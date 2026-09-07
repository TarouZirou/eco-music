//! mpvの起動とJSON IPC（Unixソケット）による制御。`linux.py` の `Player` 相当。
//! 改善点: mpvを独立プロセスグループで起動し、終了時に yt-dlp 子プロセスごと確実に停止する。

use crate::EcoEvent;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

const OBSERVE: [&str; 5] = [
	"media-title",
	"pause",
	"time-pos",
	"duration",
	"paused-for-cache",
];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub enum PlayerCommand {
	/// mpvへ渡す `{"command": [...]}` の中身
	Run(Value),
	Quit,
}

pub struct PlayerHandle {
	cmd_tx: Option<Sender<PlayerCommand>>,
	closed: Arc<AtomicBool>,
	dir: PathBuf,
	lifecycle: Option<std::thread::JoinHandle<()>>,
}

impl PlayerHandle {
	pub fn command(&self, args: &[Value]) {
		if let Some(tx) = &self.cmd_tx {
			let _ = tx.send(PlayerCommand::Run(Value::Array(args.to_vec())));
		}
	}

	/// 再生キュー(m3u)を置くための一時パス。
	pub fn queue_path(&self) -> PathBuf {
		self.dir.join("queue.m3u")
	}

	pub fn close(&mut self) {
		self.closed.store(true, Ordering::SeqCst);
		if let Some(tx) = self.cmd_tx.take() {
			let _ = tx.send(PlayerCommand::Quit);
		}
		if let Some(handle) = self.lifecycle.take() {
			let _ = handle.join();
		}
	}
}

impl Drop for PlayerHandle {
	fn drop(&mut self) {
		self.close();
	}
}

/// mpvへ渡す引数列（`core.py` の `mpv_args` 相当）。
pub fn mpv_args(socket: &Path) -> Vec<String> {
	let s = socket.to_string_lossy().into_owned();
	[
		"--no-config",
		"--idle=yes",
		"--no-video",
		"--no-terminal",
		&format!("--input-ipc-server={s}"),
		"--ytdl=yes",
		"--ytdl-format=bestaudio[abr<=128]/bestaudio",
		"--cache=yes",
		"--cache-secs=90",
		"--demuxer-max-bytes=8MiB",
		"--demuxer-max-back-bytes=1MiB",
		"--cache-pause=yes",
		"--cache-pause-wait=3",
		"--prefetch-playlist=yes",
		"--audio-display=no",
		"--gapless-audio=yes",
		"--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5",
		"--volume=70",
	]
	.into_iter()
	.map(str::to_owned)
	.collect()
}

/// PATH上の実行ファイルを探す（shutil.which 相当）。
pub fn find_executable(name: &str) -> Option<PathBuf> {
	let path = std::env::var_os("PATH")?;
	std::env::split_paths(&path)
		.map(|dir| dir.join(name))
		.find(|p| {
			p.is_file()
				&& std::fs::metadata(p)
					.map(|m| m.permissions().mode() & 0o111 != 0)
					.unwrap_or(false)
		})
}

fn send_event(events: &Sender<EcoEvent>, repaint: &dyn Fn(), event: EcoEvent) {
	if events.send(event).is_ok() {
		repaint();
	}
}

/// mpvとyt-dlpの存在を確認してから起動する。起動に失敗した場合はエラーイベントを送る。
pub fn start(
	events: Sender<EcoEvent>,
	repaint: Arc<dyn Fn() + Send + Sync>,
) -> Option<PlayerHandle> {
	if find_executable("mpv").is_none() || find_executable("yt-dlp").is_none() {
		send_event(
			&events,
			repaint.as_ref(),
			EcoEvent::PlayerError(
				"mpv と yt-dlp が必要です。READMEのインストール手順を参照してください。".into(),
			),
		);
		return None;
	}
	start_with(
		move |socket| {
			Command::new("mpv")
				.args(mpv_args(socket))
				.stdin(Stdio::null())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.process_group(0)
				.spawn()
		},
		events,
		repaint,
	)
}

/// 子プロセスの作り方を注入できる起動関数（統合試験でスタブmpvを使う）。
pub fn start_with<F>(
	spawn: F,
	events: Sender<EcoEvent>,
	repaint: Arc<dyn Fn() + Send + Sync>,
) -> Option<PlayerHandle>
where
	F: FnOnce(&Path) -> std::io::Result<Child> + Send + 'static,
{
	let dir = std::env::temp_dir().join(format!(
		"eco-music-player-{}-{}",
		std::process::id(),
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map(|d| d.as_nanos())
			.unwrap_or(0)
	));
	if std::fs::create_dir(&dir).is_err() {
		send_event(
			&events,
			repaint.as_ref(),
			EcoEvent::PlayerError("一時ディレクトリを作成できません".into()),
		);
		return None;
	}
	let socket = dir.join("mpv.sock");
	let dir_handle = dir.clone();
	let closed = Arc::new(AtomicBool::new(false));
	let (tx, rx) = mpsc::channel::<PlayerCommand>();
	let closed2 = closed.clone();
	let lifecycle = std::thread::spawn(move || {
		run_lifecycle(spawn, &socket, &dir_handle, events, repaint, closed2, rx);
	});
	Some(PlayerHandle {
		cmd_tx: Some(tx),
		closed,
		dir,
		lifecycle: Some(lifecycle),
	})
}

fn run_lifecycle<F>(
	spawn: F,
	socket: &Path,
	dir: &Path,
	events: Sender<EcoEvent>,
	repaint: Arc<dyn Fn() + Send + Sync>,
	closed: Arc<AtomicBool>,
	rx: mpsc::Receiver<PlayerCommand>,
) where
	F: FnOnce(&Path) -> std::io::Result<Child>,
{
	let mut child = match spawn(socket) {
		Ok(c) => c,
		Err(_) => {
			send_event(
				&events,
				repaint.as_ref(),
				EcoEvent::PlayerError("mpv が起動できません。mpvを更新してください。".into()),
			);
			let _ = std::fs::remove_dir_all(dir);
			return;
		}
	};
	let stream = match connect_with_retry(socket, &mut child, &closed) {
		Ok(Some(s)) => s,
		Ok(None) => {
			kill_child(&mut child);
			let _ = std::fs::remove_dir_all(dir);
			return;
		}
		Err(msg) => {
			send_event(&events, repaint.as_ref(), EcoEvent::PlayerError(msg));
			kill_child(&mut child);
			let _ = std::fs::remove_dir_all(dir);
			return;
		}
	};
	let mut writer = match stream.try_clone() {
		Ok(w) => w,
		Err(_) => {
			send_event(
				&events,
				repaint.as_ref(),
				EcoEvent::PlayerError("mpv との接続に失敗しました".into()),
			);
			kill_child(&mut child);
			let _ = std::fs::remove_dir_all(dir);
			return;
		}
	};
	// observe_property の登録（ready通知の前に完了させる）
	let mut boot = String::new();
	for (id, prop) in OBSERVE.iter().enumerate() {
		let payload = serde_json::json!(["observe_property", id, prop]);
		let line =
			serde_json::to_string(&serde_json::json!({ "command": payload })).unwrap_or_default();
		boot.push_str(&line);
		boot.push('\n');
	}
	if writer.write_all(boot.as_bytes()).is_err() {
		send_event(
			&events,
			repaint.as_ref(),
			EcoEvent::PlayerError("mpv の初期化に失敗しました".into()),
		);
		kill_child(&mut child);
		let _ = std::fs::remove_dir_all(dir);
		return;
	}
	spawn_writer(writer, rx, events.clone(), repaint.clone());
	send_event(&events, repaint.as_ref(), EcoEvent::Ready);

	// イベント読み取りループ。終了時（quit・mpv死亡）に抜ける。
	let mut reader = BufReader::new(stream);
	let mut line = String::new();
	loop {
		line.clear();
		match reader.read_line(&mut line) {
			Ok(0) | Err(_) => break,
			Ok(_) => {
				if closed.load(Ordering::SeqCst) {
					break;
				}
				if let Ok(value) = serde_json::from_str::<Value>(&line) {
					send_event(&events, repaint.as_ref(), EcoEvent::Mpv(value));
				}
			}
		}
	}
	if !closed.load(Ordering::SeqCst) {
		send_event(
			&events,
			repaint.as_ref(),
			EcoEvent::PlayerError("mpv が終了しました。アプリを再起動してください。".into()),
		);
	}
	kill_child(&mut child);
	let _ = std::fs::remove_dir_all(dir);
}

fn spawn_writer(
	mut writer: UnixStream,
	rx: mpsc::Receiver<PlayerCommand>,
	events: Sender<EcoEvent>,
	repaint: Arc<dyn Fn() + Send + Sync>,
) {
	std::thread::spawn(move || {
		loop {
			match rx.recv() {
				Ok(PlayerCommand::Run(value)) => {
					let mut line =
						serde_json::to_string(&serde_json::json!({ "command": value })).unwrap_or_default();
					line.push('\n');
					if writer.write_all(line.as_bytes()).is_err() {
						send_event(
							&events,
							repaint.as_ref(),
							EcoEvent::PlayerError("プレーヤーとの接続が切れました".into()),
						);
						let _ = writer.shutdown(std::net::Shutdown::Both);
						break;
					}
				}
				Ok(PlayerCommand::Quit) => {
					let _ = writer.write_all(b"{\"command\":[\"quit\"]}\n");
					let _ = writer.shutdown(std::net::Shutdown::Both);
					break;
				}
				Err(_) => {
					let _ = writer.shutdown(std::net::Shutdown::Both);
					break;
				}
			}
		}
	});
}

/// mpvがIPCソケットを開くまで再試行する。戻り値: Ok(Some) 接続成功 / Ok(None) アプリ終了 / Err エラー文言
fn connect_with_retry(
	socket: &Path,
	child: &mut Child,
	closed: &AtomicBool,
) -> Result<Option<UnixStream>, String> {
	let deadline = Instant::now() + CONNECT_TIMEOUT;
	loop {
		if closed.load(Ordering::SeqCst) {
			return Ok(None);
		}
		if let Ok(Some(_)) = child.try_wait() {
			return Err("mpv が起動できません。mpvを更新してください。".into());
		}
		if let Ok(s) = UnixStream::connect(socket) {
			return Ok(Some(s));
		}
		if Instant::now() >= deadline {
			return Err("mpv の起動がタイムアウトしました".into());
		}
		std::thread::sleep(Duration::from_millis(50));
	}
}

/// SIGTERM→最大1秒待機→SIGKILL で子プロセスグループを停止する。
fn kill_child(child: &mut Child) {
	let pid = child.id() as libc::pid_t;
	unsafe {
		libc::kill(-pid, libc::SIGTERM);
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
