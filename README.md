# Eco Music 0.2.0

YouTube Musicの公開・限定公開再生リストを登録して聴く、Linux / Android用の実装です。
ブラウザエンジン・動画表示・サムネイルを使わず、音声再生を主眼に設計しています。
LinuxアプリはRust（egui UI + mpv IPC）で実装されています。
Googleの公式クライアントではありません。

## 機能と範囲

| 項目 | Linux | Android |
|---|---|---|
| 実装 | Rust（egui / mpv IPC） | Java / Android標準View |
| 再生エンジン | mpv | Media3 ExoPlayer |
| YouTube抽出 | 外部yt-dlp | NewPipeExtractor |
| 再生リスト | URL取得・登録・選択・削除、最大200件 | 同左、共有メニューからURL受付 |
| 曲数 | 無制限（リスト全件を取得） | 無制限（全ページを取得） |
| シャッフル | 開始時に全エントリーを置換順列へ | 同左 |
| 全曲リピート | 任意 | 任意 |
| バックグラウンド | ウィンドウ最小化中も再生 | MediaSessionService / メディア通知 |
| 外部操作 | アプリ内ボタン | 通知・対応イヤホンのメディアボタン |
| 音声の先読み | 最大90秒・圧縮データ上限8MiB、後方1MiB | 最大90秒・サンプルバッファ目標8MiB、後方0秒 |
| 通信エラー | mpv/FFmpeg再接続、再生不可の曲は次へ | Media3内部リトライ＋URL再抽出を2/4/8秒後、以後停止 |
| 次の曲 | mpvのprefetch-playlist | 次曲の音声URL先行解決、Media3の連続キュー |

「重複なし」は各リストエントリーを一巡につき一度再生する意味です。
元リストに同じ曲が複数登録されている場合、その重複は保持します。
リピート時は同じ並びを反復し、再生ボタンを押すと再シャッフルします。
無作為に復元抽出し続ける独立したランダムモードはありません。

Googleログイン、アカウントライブラリの自動同期、非公開リスト、購入済み・会員限定曲、
ライブ配信、自動生成ラジオ、オフライン保存は対象外です。
既存リストを勝手に公開する処理はありません。

## Linuxで起動（Arch Linux）

依存パッケージを導入します。

```sh
sudo pacman -Syu --needed mpv yt-dlp noto-fonts-cjk
```

ソースから実行する場合（Rust 1.85以上）:

```sh
cargo run --release
# または
cargo build --release
./target/release/eco-music
```

### PKGBUILDでインストール

```sh
makepkg -si
```

アプリケーションメニューの「Eco Music」から起動できます。バイナリは`/usr/bin/eco-music`、
デスクトップエントリは`/usr/share/applications/dev.sapi.eco-music.desktop`に配置されます。

### ビルド済みバイナリで導入（コンパイル不要）

GitHubの[Releases](https://github.com/TarouZirou/eco-music/releases)から
`eco-music-0.2.0-x86_64` を取得し、実行権限を付けて起動します。
依存パッケージ（`mpv` `yt-dlp` `noto-fonts-cjk`）は別途導入してください。

```sh
chmod +x eco-music-0.2.0-x86_64
./eco-music-0.2.0-x86_64
```

コンパイル不要の`eco-music-bin`用PKGBUILDも `eco-music-bin/PKGBUILD` に同梱しています。

### 使い方

1. YouTube Musicで再生リストの共有URLをコピーします。
2. Eco Musicに貼り付けて「リストを取得」を押します。
3. 必要なら「登録」。次回から上部の選択欄で呼び出せます。
4. シャッフル・全曲リピートを指定し「再生」。曲のダブルクリックでその曲を先頭にできます。
5. 聴き続けるときは最小化します。ウィンドウを閉じると再生も終了します。

他のLinuxでもRust、mpv、yt-dlp、CJKフォントがあればビルド・起動できます。
Ubuntu/Debianでは`sudo apt install librust-cargo-dev mpv yt-dlp fonts-noto-cjk`相当が必要です。
古いyt-dlpはYouTubeの変更に追従できないことがあります。

yt-dlpのYouTube抽出には対応するEJSコンポーネント（Deno）が必要な場合があります。
ディストリビューションのyt-dlpパッケージで不足する場合は、専用venvを使用できます。

```sh
python3 -m venv .venv
.venv/bin/pip install -U 'yt-dlp[default]'
PATH="$PWD/.venv/bin:$PATH" cargo run --release
```

登録データは`${XDG_CONFIG_HOME:-~/.config}/eco-music/playlists.json`に保存します。
URLとリスト名のみを永続化し、認証Cookie・音声ファイルは保存しません。

## Android

Android 8.0（API 26）以上を対象にしたネイティブアプリです。Termuxは不要です。
APKの提供・検証状況は`VALIDATION.md`を確認してください。

1. APKが付属している場合は端末で開き、使用するインストーラーの「この提供元のアプリを許可」を有効にしてインストールします。
2. アプリで再生リストURLを入力→「取得」→必要なら「登録」→「再生」。
3. YouTube Musicの共有先としてEco Musicを選ぶこともできます。
4. 画面消灯時もサービスが再生を担当します。通知から再生を操作できます。
5. 「停止」はキューと再生を停止します。OSによる強制停止後の自動再開は行いません。

バックグラウンドで停止する場合、端末のアプリ別バッテリー設定でEco Musicの
バックグラウンド実行を許可してください。ColorOSなどの省電力制御による終了は、
通常のForeground Serviceだけでは完全に防止できません。
音声フォーカス管理とイヤホン切断時の一時停止を有効にしています。

### ソースからビルド

Android Studioで`android/`を開くか、JDK 17・Android SDK 35・Build Tools 35.0.0を用意します。

```sh
cd android
./gradlew :app:assembleDebug :app:testDebugUnitTest
```

Gradle Wrapperがない環境では、Gradle 8.11.1で同じタスクを実行できます。
出力：`app/build/outputs/apk/debug/app-debug.apk`。
ネットワーク上のGoogle Maven / Maven Central / JitPackから依存物を取得します。
依存バージョンは`app/build.gradle`で固定しています。
YouTube側の変更で抽出できなくなった場合、NewPipeExtractorの修正版に更新して再ビルドします。
リリース配布時は自分の署名鍵で署名し、その鍵を保持してください。
同じパッケージ名でも署名鍵が異なるAPKは上書きインストールできません。

## 「省メモリ」と「途切れない」の厳密な範囲

8MiBはアプリ全体のRSS/PSS上限ではありません。
音声デコーダ、抽出ライブラリ、ネットワーク処理、UI、VMなどのメモリは別に必要です。
Androidの8MiBはLoadControlの割り当て目標であり、厳密なプロセスメモリ上限でもありません。
再生リスト取得時は、一時的に追加メモリとCPUを使用します。
曲数上限を撤廃したため、巨大な再生リストを取得した場合はその分メモリと時間を消費します。
実機のRSS/PSS・電池消費量を測定していない状態で、具体的な削減率は主張しません。

128kbpsの圧縮音声を90秒先読みするデータ量は約1.44MBです。
ただしストリームの可用性によって128kbps超の音源にフォールバックします。
先読みは回線変動への耐性を高めますが、長時間のオフライン・OSによる終了・
YouTubeによるアクセス拒否を解消するものではありません。
曲間の完全なギャップレス再生も保証していません。

## テスト

```sh
cargo test
cd android
./gradlew :app:testDebugUnitTest
```

実機では、再生→画面消灯→10分待つ、Wi-Fiからモバイル通信へ切り替える、
一時的な通信断、イヤホン切断、通話、最終曲終了、リピート、画面回転を確認してください。

測定例：

```sh
# Linux: UIとmpvの両プロセスを対象にする
ps -C eco-music,mpv -o pid,comm,rss,pcpu
# Android: 起動直後 / 再生中 / 画面消灯後を比較する
adb shell dumpsys meminfo dev.sapi.ecomusic
```

## 参照・ライセンス

- [mpvマニュアル](https://mpv.io/manual/stable/)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- [yt-dlp EJSの設定](https://github.com/yt-dlp/yt-dlp/wiki/EJS)
- [NewPipeExtractor](https://github.com/TeamNewPipe/NewPipeExtractor)
- [Media3バックグラウンド再生](https://developer.android.com/media/media3/session/background-playback)
- [egui / eframe](https://github.com/emilk/egui)

本プロジェクトはGPL-3.0-or-laterです。依存ライブラリのライセンスは`THIRD_PARTY.md`を参照してください。
