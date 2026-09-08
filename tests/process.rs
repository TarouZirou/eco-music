//! `tests/test_process.py` の移植: キャンセルでyt-dlp子プロセスが停止すること、
//! IPCでコマンドとイベントが往復しソケット・プロセスが清掃されることを検証する。

use eco_music::extract::{ExtractError, run_extractor};
use std::os::unix::process::CommandExt;
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
	normal_shutdown(false);
}

#[test]
fn drop_stops_normal_backend() {
	normal_shutdown(true);
}

fn normal_shutdown(drop_only: bool) {
	let (tx, rx) = mpsc::channel();
	let (pid_tx, pid_rx) = mpsc::channel();
	let repaint: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
	let player = eco_music::player::start_with(
		move |socket| {
			let child = Command::new("python3")
				.args(["-u", "-c", FAKE_MPV])
				.arg(socket)
				.stdin(Stdio::null())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.process_group(0)
				.spawn()?;
			pid_tx.send(child.id()).unwrap();
			Ok(child)
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

	// Shutdown must reap the backend and remove its socket directory.
	let dir = player.queue_path().parent().unwrap().to_path_buf();
	let pid = pid_rx.recv_timeout(Duration::from_secs(5)).unwrap();
	close_bounded(player, pid, drop_only);
	assert!(
		!rx
			.try_iter()
			.any(|event| matches!(event, eco_music::EcoEvent::PlayerError(_)))
	);
	assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1, "backend reaped");
	assert!(!dir.join("mpv.sock").exists(), "socket cleaned");
	assert!(!dir.exists(), "temp dir cleaned");
}

fn close_bounded(mut player: eco_music::player::PlayerHandle, pid: u32, drop_only: bool) {
	let (tx, rx) = mpsc::channel();
	let thread = std::thread::spawn(move || {
		if drop_only {
			drop(player);
		} else {
			player.close();
			player.close(); // Idempotent, including the subsequent Drop.
		}
		tx.send(()).unwrap();
	});
	if rx.recv_timeout(Duration::from_secs(3)).is_err() {
		unsafe {
			libc::kill(-(pid as i32), libc::SIGKILL);
			libc::kill(pid as i32, libc::SIGKILL);
		}
		panic!("player shutdown exceeded three seconds");
	}
	thread.join().unwrap();
}

const STUCK_MPV: &str = r#"
import os, signal, socket, sys, time
mode = sys.argv[2]
signal.signal(signal.SIGTERM, signal.SIG_IGN)
if mode == 'descendant':
    pid = os.fork()
    if pid == 0:
        while True: time.sleep(1)
    with open(sys.argv[1] + '.child', 'w') as f: f.write(str(pid))
    signal.signal(signal.SIGTERM, signal.SIG_DFL)
if mode != 'startup':
    s = socket.socket(socket.AF_UNIX)
    s.bind(sys.argv[1])
    s.listen(0)
    if mode == 'backlog':
        c = socket.socket(socket.AF_UNIX)
        c.connect(sys.argv[1])
    else:
        c, _ = s.accept()
with open(sys.argv[1] + '.started', 'w') as f: f.write('ready')
while True: time.sleep(1)
"#;

fn stuck_backend(mode: &'static str, grouped: bool, drop_only: bool) {
	let (tx, rx) = mpsc::channel();
	let (pid_tx, pid_rx) = mpsc::channel();
	let player = eco_music::player::start_with(
		move |socket| {
			let mut cmd = Command::new("python3");
			cmd.args(["-u", "-c", STUCK_MPV]).arg(socket).arg(mode);
			if grouped {
				cmd.process_group(0);
			}
			let child = cmd.spawn()?;
			if mode == "backlog" {
				let deadline = Instant::now() + Duration::from_secs(5);
				while !socket.with_extension("sock.started").exists() {
					assert!(Instant::now() < deadline, "stub did not fill backlog");
					std::thread::sleep(Duration::from_millis(10));
				}
			}
			pid_tx.send(child.id()).unwrap();
			Ok(child)
		},
		tx,
		std::sync::Arc::new(|| {}),
	)
	.unwrap();
	let pid = pid_rx.recv_timeout(Duration::from_secs(5)).unwrap();
	let dir = player.queue_path().parent().unwrap().to_path_buf();
	let deadline = Instant::now() + Duration::from_secs(5);
	while !dir.join("mpv.sock.started").exists() {
		assert!(Instant::now() < deadline, "stub did not start");
		std::thread::sleep(Duration::from_millis(10));
	}
	let descendant = if mode == "descendant" {
		Some(
			std::fs::read_to_string(dir.join("mpv.sock.child"))
				.unwrap()
				.parse::<u32>()
				.unwrap(),
		)
	} else {
		None
	};
	if mode != "startup" && mode != "backlog" {
		assert!(matches!(
			rx.recv_timeout(Duration::from_secs(5)).unwrap(),
			eco_music::EcoEvent::Ready
		));
		// The peer never reads: this exceeds the socket buffer and blocks the writer.
		player.command(&[serde_json::json!("x".repeat(4 * 1024 * 1024))]);
		std::thread::sleep(Duration::from_millis(100));
	}
	if mode == "backlog" {
		std::thread::sleep(Duration::from_millis(100));
	}
	close_bounded(player, pid, drop_only);
	assert!(!dir.exists(), "temporary directory cleaned");
	assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1, "backend reaped");
	if let Some(pid) = descendant {
		let deadline = Instant::now() + Duration::from_secs(2);
		loop {
			// Orphans can remain zombies until the host's init reaps them.
			let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
			if stat
				.as_ref()
				.is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
				|| stat
					.as_ref()
					.is_ok_and(|s| s.rsplit_once(") ").unwrap().1.starts_with('Z'))
			{
				break;
			}
			assert!(Instant::now() < deadline, "descendant still running");
			std::thread::sleep(Duration::from_millis(10));
		}
	}
}

#[test]
fn close_interrupts_nonresponsive_ipc_and_kills_descendants() {
	stuck_backend("descendant", true, false);
}

#[test]
fn drop_interrupts_nonresponsive_ungrouped_backend() {
	stuck_backend("connected", false, true);
}

#[test]
fn close_cancels_backend_startup() {
	stuck_backend("startup", true, false);
}

#[test]
fn close_interrupts_full_socket_backlog() {
	stuck_backend("backlog", true, false);
}
