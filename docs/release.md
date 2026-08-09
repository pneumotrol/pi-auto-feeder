# リリース手順

この文書は Raspberry Pi 上で systemd と Tailscale Serve を使って配備する手順を示す．アプリは `127.0.0.1:3000` だけで待ち受け、tailnet 内から Tailscale Serve 経由で利用する．インターネットへ公開する Tailscale Funnel は使用しない．

## 前提

- Raspberry Pi OS 上で Raspberry Pi 4 向けにビルドするか、同じ OS・CPU アーキテクチャ向けのバイナリを用意する．
- Rust、Cargo、Topcoat CLI、`ffmpeg`、`sqlite3`、Tailscale をインストールする．
- Tailscale へログインし、利用者と ACL を tailnet 側で設定する．
- `/dev/video0` が利用でき、PWM0 を BCM GPIO18（物理ピン 12）から使用できるようにする．
- `pi-auto-feeder` 専用ユーザを作成し、`gpio` と `video` グループへ所属させる．systemd unit はこのユーザで動作する．

例として Debian 系では次のように作成する．既存ユーザがある場合は重複して実行しない．

```sh
sudo useradd --system --home-dir /var/lib/pi-auto-feeder --shell /usr/sbin/nologin pi-auto-feeder
sudo usermod -aG gpio,video pi-auto-feeder
sudo install -d -m 0700 -o pi-auto-feeder -g pi-auto-feeder /var/lib/pi-auto-feeder
```

## リリース前検証

リポジトリ直下で、次をすべて成功させる．`topcoat asset bundle` は CSS、クライアントスクリプト、htmx と manifest を既定で `target/assets` へ出力する．

```sh
cargo fmt
topcoat fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
topcoat asset bundle --release
```

開発・スモーク確認では必ずモックを指定し、実機の GPIO とカメラを意図せず操作しないようにする．

```sh
FEEDER_MOCK=true CAMERA_MOCK=true DATABASE_URL=sqlite:///tmp/pi-auto-feeder-smoke.sqlite3 \
  target/release/pi-auto-feeder
```

ブラウザで `http://127.0.0.1:3000` を開き、SSR、手動給餌、予定の追加・削除、設定保存、履歴、SSE による再同期を確認する．終了後、スモーク確認用 DB は不要であれば削除してよい．

## 既存 DB のバックアップ

アップグレード前に、稼働中の SQLite DB を SQLite の backup API で退避する．WAL の未チェックポイント分も含む一貫したバックアップになる．バックアップ名の日付はリリース日に置き換える．

```sh
sudo -u pi-auto-feeder sqlite3 /var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3 \
  ".backup '/var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3.bak-YYYYMMDD'"
sudo -u pi-auto-feeder sqlite3 /var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3.bak-YYYYMMDD \
  'PRAGMA integrity_check;'
```

`ok` を確認してから進む．初回配置で DB がまだ存在しない場合、この手順は不要である．

## 停止と配置

アップグレード時はバックアップ確認後にサービスを停止する．初回配置で unit がまだ存在しない場合、停止コマンドの失敗は無視してよい．

```sh
sudo systemctl stop pi-auto-feeder
sudo install -m 0755 target/release/pi-auto-feeder /usr/local/bin/pi-auto-feeder
sudo install -d -m 0755 /usr/local/bin/assets
sudo cp -a target/assets/. /usr/local/bin/assets/
sudo install -m 0644 deploy/pi-auto-feeder.service /etc/systemd/system/pi-auto-feeder.service
sudo install -d -m 0700 -o pi-auto-feeder -g pi-auto-feeder /var/lib/pi-auto-feeder
```

`AssetBundle::load()` が実行ファイルと共に bundle を読めるよう、unit の `ExecStart` に対応する `/usr/local/bin/assets` へ `manifest.toml` とハッシュ付き資産をまとめて配置する．バイナリだけを更新しない．

## 実機設定

`deploy/pi-auto-feeder.service` は次を既定値として設定する．`EnvironmentFile` の値は unit の `Environment` より優先されるため、必要な場合は `/etc/pi-auto-feeder.env` から上書きできる．

- `DATABASE_URL=sqlite:///var/lib/pi-auto-feeder/pi-auto-feeder.sqlite3`
- `HOST=127.0.0.1`
- `PORT=3000`

任意の `/etc/pi-auto-feeder.env` は実機・モック選択や配置固有の上書きに使用する．未指定時も GPIO とカメラは実機モードだが、意図を明示する場合は次のようにする．

```text
FEEDER_MOCK=false
CAMERA_MOCK=false
```

`DATABASE_URL`、`HOST`、`PORT` を変更する場合も `/etc/pi-auto-feeder.env` に記述する．配布 unit 自体は直接編集しない．ループバック以外の待受は公開範囲を変えるため、通常は変更しない．

## 起動と tailnet 内公開

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now pi-auto-feeder
sudo systemctl status pi-auto-feeder
curl --fail http://127.0.0.1:3000/
sudo tailscale serve --bg 3000
sudo tailscale serve status
```

Tailscale Serve は HTTPS を終端し、ループバックのポート 3000 へ転送する．表示された `https://<device>.<tailnet>.ts.net/` を tailnet 内の許可ユーザから開く．既存の Serve 設定がある場合は、上書き前に `tailscale serve status` で影響範囲を確認する．

## DB 初期化

アプリは新規 DB を開くと、1 トランザクションで全テーブルを作成して `PRAGMA user_version = 1` にする．

- 新規 DB には `schedules`、`feeder_status`、`feed_history`、`settings` を作成する．
- `schedules.scheduled_at` は `NOT NULL UNIQUE` とし、日時未設定や重複を許可しない．
- schema version 1 の DB は変更せず開く．
- version 0 の既存スキーマや、version 1 より新しい schema version は変更せず起動エラーにする．

初回起動後にログと画面を確認し、DB の初期化に失敗していないことを確認する．

## リリース後確認

1. `systemctl status pi-auto-feeder` と `journalctl -u pi-auto-feeder -b` に起動、DB 初期化・検査、資産読込、GPIO、`ffmpeg` のエラーがないことを確認する．
2. Tailscale Serve の URL が tailnet 外から直接公開されていないことを確認する．Funnel は有効にしない．
3. トップ画面と設定画面が SSR され、CSS とクライアントスクリプトが読み込まれることを確認する．
4. JavaScript 有効時に、手動給餌、予定追加・削除、設定保存がページ全体の再読込なしで反映されることを確認する．
5. JavaScript を無効にし、同じフォーム操作が 303 redirect で完了することを確認する．
6. 別ブラウザでトップ画面を開き、給餌・予定・設定の変更後に SSE で再取得されることを確認する．
7. 実機で連続回転サーボが 1,500 µs で停止し、給餌速度 1～100% に応じた 1,508～2,300 µs で設定した時間だけ給餌方向へ回転すること、回転前後各 1 秒の停止、クールタイムを確認する．
8. 同時に給餌要求を送り、mutex とクールタイムにより二重駆動しないことを確認する．
9. `/camera/stream` が `/dev/video0` の MJPEG を継続表示し、ブラウザ切断後に対応する `ffmpeg` が残らないことを確認する．
10. 未来の予定が指定した分に 1 回だけ実行され、成功後に削除されることを確認する．クールタイム中・結果不明・期限切れの予定は再実行されず理由付きで残ることも確認する．
11. サービス再起動後も設定、履歴、未来の予定が維持されることを確認する．

物理給餌の成功後に DB 記録だけが失敗した場合、ログに `physical feed succeeded but its history could not be recorded` が残る．この場合は二重給餌を避けるため直ちに再実行せず、給餌器と DB を手動確認する．

## ロールバック

1. サービスを停止する．
2. 直前のバイナリと、それに対応する `/usr/local/bin/assets` を同じリリースの組として戻す．
3. DB を復元する場合は、`pi-auto-feeder.sqlite3-wal` と `pi-auto-feeder.sqlite3-shm` が残っていないことを確認してから、バックアップを正規ファイル名へ戻し、所有者を `pi-auto-feeder` にする．
4. `systemctl daemon-reload` 後に起動し、リリース後確認を繰り返す．

DB や assets の上書きは復旧不能になり得るため、バックアップの `integrity_check` と対象パスを確認してから実施する．
