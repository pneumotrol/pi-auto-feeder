# Pi Auto Feeder

Pi Auto Feeder は、Raspberry Pi 4 に接続したサーボ式給餌器と Web カメラを tailnet 内から操作する小規模な Web アプリである．単一の Rust バイナリで Topcoat の SSR サーバ、SQLite 永続化、30 秒周期のスケジューラ、GPIO 制御、カメラ配信を実行する．

## 主な機能

- Web UI からの手動給餌
- サーバのローカル日時を使う 1 回限りのスケジュール給餌
- スケジュールの一覧、追加、削除、期限切れ・失敗理由の表示
- `/dev/video0` と `ffmpeg` による MJPEG カメラ監視
- サーバ時刻、最終給餌時刻、クールタイム、最新 10 件の成功履歴の表示
- クールタイム、サーボ回転時間、給餌速度の永続設定
- htmx による部分更新、SSE による別画面との同期、JavaScript 無効時の通常フォーム
- GPIO とカメラを使用しない開発用モック

## 動作の概要

ブラウザの GET は Topcoat の page handler が SQLite から状態を読み、HTML を SSR して返す．フォーム更新は同じ POST route が htmx と通常フォームを処理し、更新後にページを再取得する．永続状態が変わると `/events` が変更イベントを送り、トップ画面は `/` を再取得する．クライアント側で独自に保持する業務状態はない．

手動給餌と予定給餌は同じ `FeedService` を通る．サービスは mutex で物理操作を直列化し、ロック取得後に SQLite 上の最終成功時刻と設定からクールタイムを確認する．サーボ操作に成功した場合だけ、最終給餌時刻と履歴を 1 トランザクションで記録する．

予定は `YYYY-MM-DDTHH:MM` の分精度で保存される．スケジューラは起動直後と 30 秒ごとに現在の分と一致する予定を確保し、二重実行を避けるため実行前に暫定失敗状態を保存する．成功時は予定を削除し、クールタイム中や成否不明の場合は理由付きで残す．停止中に時刻を過ぎた予定は実行しない．

## 構成

```text
src/
├── main.rs                  起動、ルータ、サービス、スケジューラ
├── lib.rs                   モジュール公開
├── app.rs                   SSR ページ、フォーム、POST route
├── app/
│   ├── client.js            SSE 再同期、時計・クールタイム補間
│   └── style.css            UI スタイル
├── feed.rs                  サーボ制御と共通給餌サービス
├── camera.rs                モック SVG / ffmpeg MJPEG 配信
├── events.rs                SSE endpoint
├── schedule.rs              schedule ドメイン型と設定範囲
└── schedule/
    ├── migration.rs         SQLite schema version 1 の初期化
    ├── scheduler.rs         30 秒周期の予定実行
    └── store.rs             SQLite 永続化と変更通知

deploy/pi-auto-feeder.service  systemd unit
docs/release.md                 Raspberry Pi へのリリース手順
```

詳しい設計制約と変更時の規約は [`../AGENTS.md`](../AGENTS.md)、実機への配置は [`release.md`](release.md) を参照する．

## 必要なもの

開発には Rust 2024 edition 対応ツールチェーンと Topcoat CLI が必要である．実機では加えて Raspberry Pi 4、PWM0 を使うサーボ、`/dev/video0` のカメラ、`ffmpeg`、SQLite、Tailscale が必要になる．

主要な Rust 依存は Topcoat 0.5、Tokio、SQLx、rppal、chrono、color-eyre である．Web UI は htmx 2.0.10 をビルド資産として同梱し、Wasm や Node.js のクライアントビルドを必要としない．

## ローカル開発

意図しない GPIO・カメラ操作を防ぐため、開発時は必ず両方のモックを有効にする．

```sh
FEEDER_MOCK=true CAMERA_MOCK=true DATABASE_URL=sqlite:///tmp/pi-auto-feeder-dev.sqlite3 \
  topcoat dev
```

既定では `http://127.0.0.1:3000` で待ち受ける．別ポートを使う場合は `PORT`、別アドレスを使う場合は `HOST` を設定する．

モックモードでは手動・予定給餌は GPIO を操作せず成功履歴を記録し、カメラ route は固定 SVG を返す．DB は指定したパスへ新規作成され、WAL モードで使用される．

## 設定値

| 環境変数           | 既定値                            | 内容                                                           |
| ------------------ | --------------------------------- | -------------------------------------------------------------- |
| `DATABASE_URL`     | `sqlite://pi-auto-feeder.sqlite3` | SQLite 接続 URL                                                |
| `FEEDER_MOCK`      | `false`                           | `true` なら GPIO を使わない                                    |
| `CAMERA_MOCK`      | `false`                           | `true` なら固定 SVG を返す                                     |
| `HOST`             | `127.0.0.1`                       | 待受ホスト                                                     |
| `PORT`             | `3000`                            | 待受ポート                                                     |

アプリ画面から保存する給餌設定は SQLite に保持される．

| 設定           |   既定値 |       許容範囲 |
| -------------- | -------: | -------------: |
| クールタイム   |   300 秒 |   0～86,400 秒 |
| サーボ回転時間 | 1,000 ms | 100～10,000 ms |
| 給餌速度       |     100% |         1～100% |

## HTTP ルート

| Method | Path                | 用途                   |
| ------ | ------------------- | ---------------------- |
| GET    | `/`                 | トップ画面             |
| GET    | `/settings`         | 設定画面               |
| POST   | `/feed`             | 手動給餌               |
| POST   | `/schedules`        | 予定追加               |
| POST   | `/schedules/delete` | 予定削除               |
| POST   | `/settings`         | 設定保存               |
| GET    | `/camera/stream`    | モック画像または MJPEG |
| GET    | `/events`           | 状態変更 SSE           |

## DB

SQLite schema version は 1 である．新規 DB の初回起動時に次のテーブルを作成する．

- `schedules`: 必須の `scheduled_at` と任意の `failure_reason` を持つ予定
- `feeder_status`: 最終給餌時刻を持つ単一行
- `feed_history`: 成功した給餌の追記履歴
- `settings`: クールタイム、回転時間、給餌速度を持つ単一行

新規 DB はトランザクション内で schema version 1 に初期化する．version 0 の既存スキーマや version 1 より新しい DB は変更せず起動を停止する．実 DB を更新する前は [`release.md`](release.md) の手順でバックアップする．

## 検証

変更後は次を実行する．

```sh
cargo fmt
topcoat fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
topcoat asset bundle --release
```

UI 変更では htmx、SSE、JavaScript 無効時の 303 redirect を確認する．給餌・DB・スケジューラ変更では、物理失敗、クールタイム、重複、期限切れ、処理中断、スキーマバージョン不一致を含めて確認する．

## 実機運用上の注意

- アプリはユーザ認証を持たないため、ループバックだけで待ち受け、Tailscale Serve から tailnet 内へ公開する．インターネットへ直接公開しない．
- 実機モードは PWM0（BCM GPIO18、物理ピン 12）と `/dev/video0` を固定で使用する．連続回転サーボへ 20 ms 周期で制御信号を送り、1,500 µs で停止する．給餌速度 1～100% を 1,508～2,300 µs へ線形変換して給餌方向へ回転する．
- カメラ閲覧ごとに `ffmpeg` 子プロセスを起動し、接続終了時に停止する．
- 給餌後の履歴記録に失敗した場合は二重給餌を避け、自動再試行せず実機と DB を確認する．
- SQLite の日時判定はサーバのローカルタイムを使うため、OS のタイムゾーンと時刻同期を正しく設定する．
