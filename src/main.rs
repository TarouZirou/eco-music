//! Eco Music / Linux: egui によるUI、単一の音声専用mpvプロセスを制御する。
//! `linux.py` の `App` 相当。

use eco_music::EcoEvent;
use eco_music::bookmarks::{self, Bookmark};
use eco_music::extract::{self, Track};
use eco_music::player::{self, PlayerHandle};
use eco_music::shuffle;
use eco_music::urls;
use rand::SeedableRng;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

fn main() -> eframe::Result<()> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_title("Eco Music · 音声プレーヤー")
			.with_inner_size([640.0, 610.0])
			.with_min_inner_size([440.0, 470.0]),
		..Default::default()
	};
	eframe::run_native(
		"Eco Music",
		options,
		Box::new(|cc| {
			install_fonts(&cc.egui_ctx);
			Ok(Box::new(EcoMusic::new(cc.egui_ctx.clone())))
		}),
	)
}

/// CJKフォント（日本語表示用）をフォールバックとして登録する。
fn install_fonts(ctx: &egui::Context) {
	let Some(path) = find_cjk_font() else {
		return;
	};
	let Ok(bytes) = std::fs::read(&path) else {
		return;
	};
	let mut fonts = egui::FontDefinitions::default();
	fonts
		.font_data
		.insert("eco-cjk".into(), egui::FontData::from_owned(bytes).into());
	for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
		if let Some(list) = fonts.families.get_mut(&family) {
			list.push("eco-cjk".into());
		}
	}
	ctx.set_fonts(fonts);
}

/// よくある場所から日本語対応フォントを探す。なければフォント領域を走査する。
fn find_cjk_font() -> Option<PathBuf> {
	const CANDIDATES: [&str; 5] = [
		"/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
		"/usr/share/fonts/noto-cjk/NotoSansCJKjp-Regular.otf",
		"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
		"/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
		"/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
	];
	for path in CANDIDATES {
		if PathBuf::from(path).is_file() {
			return Some(PathBuf::from(path));
		}
	}
	let mut found = None;
	for root in ["/usr/share/fonts", "/usr/local/share/fonts"] {
		let root = PathBuf::from(root);
		if !root.is_dir() {
			continue;
		}
		let mut stack = vec![root];
		while let (Some(dir), true) = (stack.pop(), found.is_none()) {
			if let Ok(entries) = std::fs::read_dir(&dir) {
				for entry in entries.flatten() {
					let p = entry.path();
					if p.is_dir() {
						stack.push(p);
					} else if p
						.extension()
						.is_some_and(|e| e == "ttc" || e == "otf" || e == "ttf")
						&& p.to_string_lossy().contains("CJK")
					{
						found = Some(p);
						break;
					}
				}
			}
		}
		if found.is_some() {
			break;
		}
	}
	found
}

fn playlists_path() -> PathBuf {
	let config = std::env::var_os("XDG_CONFIG_HOME")
		.map(PathBuf::from)
		.filter(|p| p.is_absolute())
		.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
	config.join("eco-music/playlists.json")
}

fn fmt_clock(seconds: f64) -> String {
	let total = seconds.max(0.0) as u64;
	format!("{}:{:02}", total / 60, total % 60)
}

struct EcoMusic {
	ctx: egui::Context,
	event_tx: mpsc::Sender<EcoEvent>,
	events: Receiver<EcoEvent>,
	player: Option<PlayerHandle>,
	bookmarks: Vec<Bookmark>,
	bookmark_sel: usize,
	url: String,
	tracks: Vec<Track>,
	loaded_url: String,
	loaded_title: String,
	shuffle: bool,
	repeat: bool,
	busy: bool,
	ready: bool,
	generation: u64,
	cancel: Arc<AtomicBool>,
	extraction_thread: Option<std::thread::JoinHandle<()>>,
	selected: Option<usize>,
	now: String,
	status: String,
	position: f64,
	duration: f64,
	error_popup: Option<String>,
	exiting: bool,
}

impl EcoMusic {
	fn new(ctx: egui::Context) -> Self {
		let (tx, events) = mpsc::channel();
		let repaint: Arc<dyn Fn() + Send + Sync> = {
			let ctx = ctx.clone();
			Arc::new(move || ctx.request_repaint())
		};
		let player = player::start(tx.clone(), repaint.clone());
		let path = playlists_path();
		Self {
			ctx,
			event_tx: tx,
			events,
			player,
			bookmarks: bookmarks::read(&path),
			bookmark_sel: usize::MAX,
			url: String::new(),
			tracks: Vec::new(),
			loaded_url: String::new(),
			loaded_title: String::new(),
			shuffle: true,
			repeat: false,
			busy: false,
			ready: false,
			generation: 0,
			cancel: Arc::new(AtomicBool::new(false)),
			extraction_thread: None,
			selected: None,
			now: "再生リストURLを入力してください".into(),
			status: "起動中…".into(),
			position: 0.0,
			duration: 0.0,
			error_popup: None,
			exiting: false,
		}
	}

	fn shutdown(&mut self) {
		self.cancel.store(true, Ordering::SeqCst);
		if let Some(player) = self.player.as_mut() {
			player.close();
		}
		if let Some(thread) = self.extraction_thread.take() {
			let _ = thread.join();
		}
	}

	fn cmd(&self, args: &[Value]) {
		if let Some(player) = &self.player {
			player.command(args);
		}
	}

	fn load(&mut self) {
		if self.busy {
			return;
		}
		let url = match urls::playlist_url(&self.url) {
			Ok(u) => u,
			Err(e) => {
				self.error_popup = Some(e.0);
				return;
			}
		};
		self.busy = true;
		self.generation += 1;
		let token = self.generation;
		self.status = "再生リストを取得中…（全件取得）".into();
		// The completion event can arrive just before the previous worker exits.
		if let Some(thread) = self.extraction_thread.take() {
			let _ = thread.join();
		}
		self.cancel.store(false, Ordering::SeqCst);
		let (tx, cancel, ctx) = (self.event_tx.clone(), self.cancel.clone(), self.ctx.clone());
		self.extraction_thread = Some(std::thread::spawn(move || {
			let result = extract::load_playlist(&url, &cancel);
			let event = match result {
				Ok((title, tracks)) => EcoEvent::Loaded {
					token,
					url,
					title,
					tracks,
				},
				Err(e) => EcoEvent::LoadError(e.to_string()),
			};
			if tx.send(event).is_ok() {
				ctx.request_repaint();
			}
		}));
	}

	fn play(&mut self, selected: bool) {
		let Some(player) = self.player.as_ref() else {
			return;
		};
		if !self.ready || self.tracks.is_empty() {
			return;
		}
		let mut rng = rand::rngs::StdRng::seed_from_u64(
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map(|d| d.as_nanos() as u64)
				.unwrap_or(0),
		);
		let mut order = shuffle::ordered(&self.tracks, self.shuffle, &mut rng);
		let selected_index = match (selected, self.selected) {
			(true, Some(i)) if i < self.tracks.len() => Some(i),
			_ => None,
		};
		if let Some(index) = selected_index {
			let chosen = self.tracks[index].clone();
			shuffle::promote_to_front(&mut order, &chosen);
		}
		let queue = order
			.iter()
			.map(|t| t.url.as_str())
			.collect::<Vec<_>>()
			.join("\n")
			+ "\n";
		let queue_path = player.queue_path();
		if let Err(e) = std::fs::write(&queue_path, queue) {
			self.error_popup = Some(format!("再生キューを書き込めません: {e}"));
			return;
		}
		let queue_path = queue_path.to_string_lossy().into_owned();
		self.cmd(&[json!("loadlist"), Value::from(queue_path), json!("replace")]);
		self.cmd(&[json!("set_property"), json!("pause"), json!(false)]);
		self.set_repeat();
		self.status = "音声を取得中…".into();
	}

	fn set_repeat(&self) {
		self.cmd(&[
			json!("set_property"),
			json!("loop-playlist"),
			Value::from(if self.repeat { "inf" } else { "no" }),
		]);
	}

	fn save(&mut self) {
		let url = match urls::playlist_url(&self.url) {
			Ok(u) => u,
			Err(e) => {
				self.error_popup = Some(e.0);
				return;
			}
		};
		if url != self.loaded_url {
			self.error_popup = Some("先にこのリストを取得してください".into());
			return;
		}
		self.bookmarks.retain(|b| b.url != url);
		self.bookmarks.push(Bookmark {
			title: self.loaded_title.clone(),
			url,
		});
		let excess = self
			.bookmarks
			.len()
			.saturating_sub(bookmarks::MAX_BOOKMARKS);
		self.bookmarks.drain(0..excess);
		self.bookmark_sel = self.bookmarks.len() - 1;
		if let Err(e) = bookmarks::save(&playlists_path(), &self.bookmarks) {
			self.error_popup = Some(format!("保存エラー: {e}"));
		}
	}

	fn remove(&mut self) {
		if self.bookmark_sel >= self.bookmarks.len() {
			return;
		}
		self.bookmarks.remove(self.bookmark_sel);
		self.bookmark_sel = usize::MAX;
		if let Err(e) = bookmarks::save(&playlists_path(), &self.bookmarks) {
			self.error_popup = Some(format!("保存エラー: {e}"));
		}
	}

	fn drain_events(&mut self) {
		for _ in 0..200 {
			match self.events.try_recv() {
				Ok(event) => self.handle_event(event),
				Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
			}
		}
	}

	fn handle_event(&mut self, event: EcoEvent) {
		match event {
			EcoEvent::Ready => {
				self.ready = true;
				self.status = "準備完了".into();
			}
			EcoEvent::PlayerError(msg) => {
				self.status = msg;
			}
			EcoEvent::LoadError(msg) => {
				self.busy = false;
				self.status = msg;
			}
			EcoEvent::Loaded {
				token,
				url,
				title,
				tracks,
			} => {
				self.busy = false;
				if token != self.generation {
					return;
				}
				self.loaded_url = url;
				self.loaded_title = title.clone();
				self.tracks = tracks;
				self.selected = None;
				self.status = format!("{title} · {}曲", self.tracks.len());
			}
			EcoEvent::Mpv(value) => self.handle_mpv(value),
		}
	}

	fn handle_mpv(&mut self, value: Value) {
		let obj = match value.as_object() {
			Some(o) => o,
			None => return,
		};
		match obj.get("event").and_then(Value::as_str) {
			Some("property-change") => {
				let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
				let data = obj.get("data");
				match name {
					"media-title" => {
						if let Some(title) = data.and_then(Value::as_str) {
							self.now = title.to_owned();
						}
					}
					"time-pos" => {
						self.position = data.and_then(Value::as_f64).unwrap_or(0.0);
					}
					"duration" => {
						self.duration = data.and_then(Value::as_f64).unwrap_or(0.0);
					}
					"paused-for-cache" => {
						// 旧実装では補充中の文言が残り続けたので false 時に戻す
						if data == Some(&Value::Bool(true)) {
							self.status = "バッファ補充中…".into();
						} else {
							self.status = PLAYING_STATUS.into();
						}
					}
					"pause" => {
						if data == Some(&Value::Bool(true)) {
							self.status = "一時停止".into();
						} else {
							self.status = PLAYING_STATUS.into();
						}
					}
					_ => {}
				}
			}
			Some("file-loaded") => {
				self.status = PLAYING_STATUS.into();
			}
			Some("end-file") if obj.get("reason") == Some(&json!("error")) => {
				self.status =
					"この曲を再生できません。次の曲へ進みます。yt-dlpの更新も確認してください。".into();
			}
			Some("idle") => {
				self.status = "停止中".into();
			}
			_ => {}
		}
	}
}

const PLAYING_STATUS: &str = "再生中 · 先読み最大90秒 / 圧縮データ8MiB";

impl eframe::App for EcoMusic {
	/// フレーム毎の状態更新（非表示中も呼ばれる）。UIの表示は行わない。
	fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// ウィンドウ閉鎖を横取りしてプレーヤーを停止してから終了する
		if ctx.input(|i| i.viewport().close_requested()) {
			if !self.exiting {
				self.exiting = true;
				self.shutdown();
			}
			return;
		}
		self.drain_events();
		ctx.request_repaint_after(Duration::from_millis(300));
	}

	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.show_ui(ui);
	}
}

impl EcoMusic {
	fn show_ui(&mut self, ui: &mut egui::Ui) {
		if let Some(message) = self.error_popup.clone() {
			egui::Window::new("エラー")
				.collapsible(false)
				.resizable(false)
				.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
				.show(ui.ctx(), |ui| {
					ui.set_min_width(280.0);
					ui.label(&message);
					ui.vertical_centered(|ui| {
						if ui.button("OK").clicked() {
							self.error_popup = None;
						}
					});
				});
		}

		let mut play_clicked = false;
		let mut play_selected = false;

		egui::Panel::bottom("playback").show(ui, |ui| {
			ui.horizontal_wrapped(|ui| {
				if ui
					.add_enabled(
						self.ready && !self.tracks.is_empty(),
						egui::Button::new("再生"),
					)
					.clicked()
				{
					play_clicked = true;
				}
				ui.add_enabled_ui(self.ready, |ui| {
					if ui.button("前へ").clicked() {
						self.cmd(&[json!("playlist-prev"), json!("weak")]);
					}
					if ui.button("再生 / 一時停止").clicked() {
						self.cmd(&[json!("cycle"), json!("pause")]);
					}
					if ui.button("次へ").clicked() {
						self.cmd(&[json!("playlist-next"), json!("weak")]);
					}
					if ui.button("停止").clicked() {
						self.cmd(&[json!("stop")]);
					}
				});
			});
			ui.add(egui::Label::new(&self.now).truncate());
			ui.label(format!(
				"{} / {}",
				fmt_clock(self.position),
				fmt_clock(self.duration)
			));
			ui.add(egui::Label::new(&self.status).truncate());
		});

		egui::CentralPanel::default().show(ui, |ui| {
			ui.heading("Eco Music");
			ui.label("音声のみ · 公開 / 限定公開の再生リスト");
			ui.add_space(10.0);

			let selected_text = self
				.bookmarks
				.get(self.bookmark_sel)
				.map(|b| b.title.clone())
				.unwrap_or_else(|| "登録済みリストを選択".into());
			let mut pick = self.bookmark_sel;
			ui.add_enabled_ui(!self.busy, |ui| {
				egui::ComboBox::from_id_salt("bookmarks")
					.selected_text(selected_text)
					.width(320.0)
					.show_ui(ui, |ui| {
						for (i, b) in self.bookmarks.iter().enumerate() {
							ui.selectable_value(&mut pick, i, &b.title);
						}
					});
			});
			if pick != self.bookmark_sel && pick < self.bookmarks.len() {
				self.bookmark_sel = pick;
				self.url = self.bookmarks[pick].url.clone();
				self.load();
			}

			ui.add(
				egui::TextEdit::singleline(&mut self.url)
					.hint_text("YouTube Musicの再生リストURL")
					.desired_width(f32::INFINITY),
			);
			ui.add_space(4.0);
			ui.horizontal(|ui| {
				if ui
					.add_enabled(!self.busy, egui::Button::new("リストを取得"))
					.clicked()
				{
					self.load();
				}
				if ui.button("登録").clicked() {
					self.save();
				}
				if ui.button("登録を削除").clicked() {
					self.remove();
				}
			});
			ui.add_space(4.0);
			ui.checkbox(
				&mut self.shuffle,
				"シャッフル（重複なし・次の再生開始時に適用）",
			);
			if ui.checkbox(&mut self.repeat, "全曲リピート").changed() {
				self.set_repeat();
			}
			ui.add_space(6.0);

			let row_height = ui.text_style_height(&egui::TextStyle::Body);
			let selected = self.selected;
			egui::ScrollArea::vertical()
				.auto_shrink([false, false])
				.show_rows(ui, row_height, self.tracks.len(), |ui, range| {
					for i in range {
						let is_selected = selected == Some(i);
						let response =
							ui.add(egui::Button::selectable(is_selected, &self.tracks[i].title).truncate());
						if response.clicked() {
							self.selected = Some(i);
						}
						if response.double_clicked() {
							self.selected = Some(i);
							play_selected = true;
						}
					}
				});
		});

		if play_clicked {
			self.play(false);
		}
		if play_selected {
			self.play(true);
		}
	}
}

impl Drop for EcoMusic {
	fn drop(&mut self) {
		self.shutdown();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fixture(ctx: egui::Context, track_count: usize) -> EcoMusic {
		let (event_tx, events) = mpsc::channel();
		let title = "A very long playlist and track title ".repeat(100);
		EcoMusic {
			ctx,
			event_tx,
			events,
			player: None,
			bookmarks: vec![Bookmark {
				title: title.clone(),
				url: "https://www.youtube.com/playlist?list=PLfixture".into(),
			}],
			bookmark_sel: 0,
			url: String::new(),
			tracks: (0..track_count)
				.map(|i| Track {
					title: format!("Track {i}: {title}"),
					url: format!("https://www.youtube.com/watch?v={i:011}"),
				})
				.collect(),
			loaded_url: String::new(),
			loaded_title: title.clone(),
			shuffle: true,
			repeat: false,
			busy: false,
			ready: true,
			generation: 0,
			cancel: Arc::new(AtomicBool::new(false)),
			extraction_thread: None,
			selected: None,
			now: title.clone(),
			status: title,
			position: 12.0,
			duration: 345.0,
			error_popup: None,
			exiting: false,
		}
	}

	fn render(app: &mut EcoMusic, size: egui::Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
		let mut output = app.ctx.clone().run_ui(
			egui::RawInput {
				screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
				events,
				..Default::default()
			},
			|ui| app.show_ui(ui),
		);
		output.textures_delta.clear();
		output
	}

	#[test]
	fn transport_stays_visible_with_empty_and_large_playlists() {
		for size in [egui::vec2(440.0, 470.0), egui::vec2(640.0, 610.0)] {
			for count in [0, 5000] {
				for ready in [false, true] {
					for font_size in [13.0, 24.0] {
						let ctx = egui::Context::default();
						ctx.global_style_mut(|style| {
							style.override_font_id = Some(egui::FontId::proportional(font_size));
						});
						let mut app = fixture(ctx, count);
						app.ready = ready;
						// Bottom panels use the previous frame's measured height.
						render(&mut app, size, vec![]);
						render(&mut app, size, vec![]);
						for _ in 0..3 {
							let output = render(&mut app, size, vec![]);
							let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
							let mut rows = Vec::new();
							for label in ["再生", "前へ", "再生 / 一時停止", "次へ", "停止"] {
								let matches: Vec<_> = output
									.shapes
									.iter()
									.filter_map(|shape| {
										if let egui::epaint::Shape::Text(text) = &shape.shape {
											(text.galley.text() == label).then_some((shape.clip_rect, text))
										} else {
											None
										}
									})
									.collect();
								assert_eq!(
									matches.len(),
									1,
									"missing/duplicate {label}: {size:?}, {count}, {ready}, {font_size}"
								);
								let (clip, text) = matches[0];
								let bounds = text.galley.rect.translate(text.pos.to_vec2());
								assert!(
									viewport.contains_rect(bounds) && clip.contains_rect(bounds),
									"clipped {label}: {bounds:?}, clip {clip:?}, viewport {viewport:?}, {count}, {ready}, {font_size}"
								);
								rows.push(bounds.top());
							}
							if size.x == 440.0 && font_size == 24.0 {
								assert!(
									rows.iter().any(|y| *y > rows[0] + font_size),
									"buttons did not wrap: {rows:?}"
								);
							}
							let track_labels = output.shapes.iter().filter(|shape| {
								matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.text().starts_with("Track "))
							}).count();
							if count > 0 {
								assert!(
									(1..50).contains(&track_labels),
									"rows not virtualized: {track_labels}"
								);
							}
						}
					}
				}
			}
		}
	}

	#[test]
	fn clicking_a_virtual_row_selects_it_and_loading_clears_selection() {
		let mut app = fixture(egui::Context::default(), 5000);
		let size = egui::vec2(440.0, 470.0);
		render(&mut app, size, vec![]);
		render(&mut app, size, vec![]);
		let output = render(&mut app, size, vec![]);
		let pos = output
			.shapes
			.iter()
			.find_map(|shape| {
				if let egui::epaint::Shape::Text(text) = &shape.shape
					&& text.galley.text().starts_with("Track 1:")
				{
					return Some(text.pos + egui::vec2(10.0, 5.0));
				}
				None
			})
			.expect("second virtual row is visible");
		for pressed in [true, false] {
			render(
				&mut app,
				size,
				vec![
					egui::Event::PointerMoved(pos),
					egui::Event::PointerButton {
						pos,
						button: egui::PointerButton::Primary,
						pressed,
						modifiers: egui::Modifiers::NONE,
					},
				],
			);
		}
		assert_eq!(app.selected, Some(1));
		app.handle_event(EcoEvent::Loaded {
			token: app.generation,
			url: app.bookmarks[0].url.clone(),
			title: "Replacement".into(),
			tracks: vec![],
		});
		assert_eq!(app.selected, None);
	}

	#[test]
	fn save_rejects_unloaded_url_without_changing_bookmarks() {
		let mut app = fixture(egui::Context::default(), 0);
		app.url = app.bookmarks[0].url.clone();
		app.save();
		assert!(app.error_popup.is_some());
		assert_eq!(app.bookmarks.len(), 1);
		assert_eq!(app.bookmark_sel, 0);
	}

	#[test]
	fn shutdown_cancels_and_joins_extraction() {
		let mut app = fixture(egui::Context::default(), 0);
		let cancel = app.cancel.clone();
		let completed = Arc::new(AtomicBool::new(false));
		let worker_completed = completed.clone();
		app.extraction_thread = Some(std::thread::spawn(move || {
			while !cancel.load(Ordering::SeqCst) {
				std::thread::sleep(Duration::from_millis(1));
			}
			worker_completed.store(true, Ordering::SeqCst);
		}));
		app.shutdown();
		assert!(completed.load(Ordering::SeqCst));
		assert!(app.extraction_thread.is_none());
		app.shutdown();
	}
}
