# リリース手順

## 前提

- Raspberry Pi 4 上に `ffmpeg` をインストールする．
- 実行ユーザ `pi-auto-feeder` を `gpio` および `video` グループへ追加する．
- `/dev/video0` と PWM0（BCM GPIO18，物理ピン12）を利用可能にする．
- Web UI はインターネットへ直接公開せず，Tailscale Serve 経由で公開する．

## ビルドと配置

```sh
cargo build --release
topcoat asset bundle --release
sudo install -m 0755 target/release/pi-auto-feeder /usr/local/bin/pi-auto-feeder
sudo install -d /usr/local/bin/assets
sudo cp -a target/assets/. /usr/local/bin/assets/
sudo install -m 0644 deploy/pi-auto-feeder.service /etc/systemd/system/pi-auto-feeder.service
sudo install -d -o pi-auto-feeder -g pi-auto-feeder /var/lib/pi-auto-feeder
```

実機固有の設定は `/etc/pi-auto-feeder.env` に記述する．値を省略した場合，GPIOとカメラは実機モードになる．

```text
FEEDER_MOCK=false
CAMERA_MOCK=false
DATABASE_URL=sqlite:///var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3
```

サービスを有効化し，ループバックで起動したアプリをtailnet内へ公開する．

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now pi-auto-feeder
sudo tailscale serve --bg http://127.0.0.1:3000
```

## データ移行

停止中のサービスが使用するDBをバックアップしてから，新しいバイナリを初回起動する．移行処理はトランザクション内で実行されるが，物理的な破損やストレージ障害には備えられない．

```sh
sudo systemctl stop pi-auto-feeder
sudo cp --preserve=all /var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3 \
  /var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3.bak
sudo systemctl start pi-auto-feeder
```

## リリース前確認

1. `cargo fmt`，`topcoat fmt`，`cargo clippy --all-targets -- -D warnings`，`cargo test`，`cargo build --release`，`topcoat asset bundle --release` を実行する．
2. モックモードでSSR，設定保存，手動給餌，スケジュール追加・削除，SSE更新を確認する．
3. JavaScriptを無効にし，フォームから手動給餌，スケジュール操作，設定保存ができることを確認する．
4. 実機でサーボの初期位置，駆動時間，クールタイム，カメラ映像を確認する．
5. 給餌中に同時リクエストを送り，二重にサーボが動かないことを確認する．
6. サービスを再起動し，設定，履歴，将来のスケジュールが維持されることを確認する．
7. `journalctl -u pi-auto-feeder` にDB記録失敗，GPIO失敗，`ffmpeg`終了がないことを確認する．

物理給餌の成功後にDB記録だけが失敗した場合，ログには `physical feed succeeded but its history could not be recorded` が残る．この場合は直ちに再給餌せず，給餌器とDBの状態を手動で確認する．

## ロールバック

サービスを停止し，直前のバイナリへ戻す．新バージョンによるDB移行後は，旧バイナリが新スキーマを扱えると確認できない限り，バックアップしたDBも同時に復元する．
