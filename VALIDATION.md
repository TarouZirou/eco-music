# 検証記録 — Eco Music 0.2.1

この成果物は試作版です。実機再生確認済みの製品版ではありません。

## 0.2.1の変更と検証

- Linuxの操作ボタンを画面下部に固定し、長い曲名で一覧の行高が崩れる問題を修正。
- 終了時のIPC待ちを中断し、応答しないmpvと子プロセスを停止。取得処理の終了も待機。
- 登録の選択ずれ、取得中のリスト切り替え、取得タイムアウト、同時保存を修正。
- `cargo test`: 30件通過。最小画面・5000曲・拡大文字でのヘッドレスUI検証を含む。
- `cargo clippy --all-targets -- -D warnings`: 通過。
- `cargo build --release --locked` と `cargo test --release --locked`: 通過（30件）。
- 実際の音声再生とネイティブウィンドウを閉じる操作の組み合わせは未検証。
- Androidは変更なし。以下のLinuxビルド・インストール記録は0.2.0時点。

## Linux（Rust実装）

| 検証 | 結果 |
|---|---|
| `cargo build --release`（thin LTO） | 通過 |
| `cargo clippy --all-targets -- -D warnings` | 通過 |
| `cargo test`（URL正規化・拒否ケース・シャッフル置換・原子ブックマーク・不正行スキップ・パース無制限5000件・NotFound/タイムアウト/キャンセル・スタブmpv IPC往復と清掃） | 16件通過 |
| GUI起動スモークテスト（mpv未導入環境でエラー文言表示まで） | 通過 |
| mpv・yt-dlp実環境でのEnd-to-End再生 | 未検証（mpv導入後に実施） |
| `makepkg` によるパッケージビルド・インストール | 通過 |
| 実機RSS/PSS・消費電力 | 未測定 |

## Android（Java実装）

| 検証 | 結果 |
|---|---|
| Android Javaコンパイル・DEX・APK生成（0.1.0時点） | 通過 |
| Android単体テスト（0.1.0時点、JUnit 5件） | 通過 |
| APK署名検証（0.1.0時点、apksigner verify v2署名） | 通過 |
| Android Lint（0.1.0時点、エラー0・警告8） | 通過 |
| 0.2.0のJava変更（曲数上限撤廃）の再ビルド | 未検証（Android SDK未整備環境のため、機械的な上限除去のみ） |
| Android端末でのインストール・音出し | 未検証 |
| YouTube実サービスとのEnd-to-End再生 | 未検証 |
| 曲間ギャップ・長時間連続再生・通信切替 | 未検証 |

## 0.2.0の主な変更と修正

- Linux実装をPython/TkからRust（egui UI + mpv IPC）に全面移植した。
- 曲数の上限（旧: リスト先頭2000曲）を撤廃し、再生リスト全件を取得するようにした。
  Linuxはyt-dlp `--flat-playlist` の全件ダンプ、Androidはページ取得ループの上限（100ページ）を除去した。
- 修正した既知の不具合:
  1. バッファ補充終了後もステータス表示が「バッファ補充中…」のまま残る（Linux）→ `paused-for-cache` のfalse時と一時停止解除時にステータスを復帰させるようにした。
  2. ブックマーク1行の不正データで登録済みリスト全体が失われる（Linux）→ 不正行のみ読み飛ばし、有効な行を保持するようにした。
  3. `install-linux.py` のdesktopエントリ書き込み時のバックスラッシュ二重エスケープ（Linux）→ Python実装の廃止に伴い解消。
  4. 終了時にmpvの子プロセス（yt-dlp）が残る可能性（Linux）→ mpvを独立プロセスグループで起動し、終了時にグループごと停止するようにした。
- AndroidはJavaのまま維持し、曲数上限の撤廃と文言更新のみ適用した。

## 提供物

- `PKGBUILD`: Arch Linux用パッケージ定義（`makepkg -si` で導入）。
- 本リポジトリ：Linuxアプリ（Rust）、Androidプロジェクト（Java）、テスト、ライセンス。

Androidビルド環境（0.1.0検証時）: JDK 17、Gradle 8.11.1、Android Gradle Plugin 8.9.2、SDK 35、Build Tools 35.0.0。

APKはデバッグ署名であり、ストア向けリリース署名ではありません。
署名鍵・認証情報はソースアーカイブに含めません。

実機上ではREADMEの検証項目を確認してください。
問題が発生する場合、OSのバージョン・再生リストの公開範囲・停止タイミング・表示されたエラーを記録すると切り分けできます。
