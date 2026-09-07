//! `tests/test_process.py` の移植: キャンセルでyt-dlp子プロセスが停止すること、
//! IPCでコマンドとイベントが往復しソケット・プロセスが清掃されることを検証する。

use eco_music::extract::{ExtractError, run_extractor};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn cancel_stops_extractor_child() {
	let cancel = AtomicBool::new(true);
	let before = Instant::now();
	let mut cmd = Command::new("python3");
	cmd
		.args(["-c", "import time; time.sleep(60)"])
		.stdin(Stdio::null());
	let result = run_extractor(&mut cmd, &cancel, Duration::from_secs(30));
	assert!(
		matches!(result, Err(ExtractError::Cancelled)),
		"expected Cancelled, got {result:?}"
	);
	assert!(before.elapsed() < Duration::from_secs(3));
}

#[test]
fn timeout_kills_extractor_child() {
	let cancel = AtomicBool::new(false);
	let mut cmd = Command::new("python3");
	cmd
		.args(["-c", "import time; time.sleep(60)"])
		.stdin(Stdio::null());
	let result = run_extractor(&mut cmd, &cancel, Duration::from_millis(300));
	assert!(
		matches!(result, Err(ExtractError::Timeout)),
		"expected Timeout, got {result:?}"
	);
}

#[test]
fn missing_binary_reports_notfound() {
	let cancel = AtomicBool::new(false);
	let mut cmd = Command::new("eco-music-nonexistent-binary");
	let result = run_extractor(&mut cmd, &cancel, Duration::from_secs(5));
	assert!(matches!(result, Err(ExtractError::NotFound)));
}

const FAKE_MPV: &str = r#"
import socket, sys, json
s = socket.socket(socket.AF_UNIX)
s.bind(sys.argv[1])
s.listen(1)
c, _ = s.accept()
with c.makefile('rb') as f:
    for line in f:
        cmd = json.loads(line)['command']
        if cmd[0] == 'quit':
            break
        if cmd[0] == 'cycle':
            c.sendall((json.dumps({'event': 'property-change', 'name': 'pause', 'data': True}) + '\n').encode())
c.close(); s.close()
"#;

#[test]
fn ipc_commands_events_and_clean_exit() {
	let (tx, rx) = mpsc::channel();
	let repaint: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
	let mut player = eco_music::player::start_with(
		move |socket| {
			Command::new("python3")
				.args(["-u", "-c", FAKE_MPV])
				.arg(socket)
				.stdin(Stdio::null())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.spawn()
		},
		tx,
		repaint,
	)
	.expect("player handle");

	// ready
	let event = rx
		.recv_timeout(Duration::from_secs(5))
		.expect("ready event");
	assert!(matches!(event, eco_music::EcoEvent::Ready), "got {event:?}");

	// コマンド→イベント往復
	player.command(&[serde_json::json!("cycle"), serde_json::json!("pause")]);
	let event = rx
		.recv_timeout(Duration::from_secs(5))
		.expect("property event");
	match event {
		eco_music::EcoEvent::Mpv(value) => {
			assert_eq!(value["event"], "property-change");
			assert_eq!(value["name"], "pause");
			assert_eq!(value["data"], true);
		}
		other => panic!("expected Mpv event, got {other:?}"),
	}

	// 終了: quit後、ソケットごと一時ディレクトリが清掃される
	let dir = player.queue_path().parent().unwrap().to_path_buf();
	player.close();
	assert!(!dir.join("mpv.sock").exists(), "socket cleaned");
	assert!(!dir.exists(), "temp dir cleaned");
}
