# Maintainer: TarouZirou <zinnnnnnnnnnnn8@gmail.com>
# Contributor: TarouZirou <zinnnnnnnnnnnn8@gmail.com>

pkgname=eco-music
pkgver=0.2.0
pkgrel=1
pkgdesc='YouTube Musicの公開・限定公開再生リストを聴く、音声専用の軽量プレーヤー（Rust / egui UI + mpv IPC）'
arch=('x86_64' 'aarch64')
url='https://github.com/TarouZirou/eco-music'
license=('GPL-3.0-or-later')
depends=(
  'mpv'
  'yt-dlp'
  'noto-fonts-cjk'   # 日本語UI表示用のフォールバックフォント
  'hicolor-icon-theme'
)
makedepends=('cargo')
optdepends=(
  'deno: yt-dlpのEJSによる署名解決が必要な場合'
)
source=("$pkgname-$pkgver.tar.gz::https://github.com/TarouZirou/eco-music/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')  # タグ作成後に実効値へ置き換える

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/^host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

check() {
  cd "$pkgname-$pkgver"
  export CARGO_TARGET_DIR=target
  cargo test --frozen --release
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm0755 "target/release/$pkgname" -t "$pkgdir/usr/bin/"
  install -Dm0644 dev.sapi.eco-music.desktop -t "$pkgdir/usr/share/applications/"
  install -Dm0644 LICENSE -t "$pkgdir/usr/share/licenses/$pkgname/"
  install -Dm0644 README.md -t "$pkgdir/usr/share/doc/$pkgname/"
}
