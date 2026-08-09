# リポジトリガイドライン

## プロジェクト概要

- Raspberry Pi 4 に接続したサーボ式給餌器と Web カメラを、tailnet 内の少人数で操作する Rust サーバである．
- Web UI から手動給餌、1 回限りの日時指定給餌、スケジュールの一覧・追加・削除、カメラ監視、設定変更を行う．
- サーバ時刻、最終給餌時刻、クールタイム残り時間、最新 10 件の成功履歴を表示する．
- Raspberry Pi 単体で動作し、スケジュール実行や永続化を外部サービスへ依存させない．

## 現行アーキテクチャ

- 単一の Rust バイナリが Topcoat 0.5 の HTTP サーバと Tokio のバックグラウンドスケジューラを起動する．Wasm やクライアント用 Rust ビルドは使用しない．
- Topcoat の `#[page]`、`#[layout]`、`#[route]`、`view!` で SSR UI と HTTP ルートを構成する．ルートは `Router::builder().discover()` で収集する．
- GET の page handler が `ScheduleStore` から表示データを取得する．更新は `Form<T>` を受け取る POST route に集約する．
- htmx リクエストには `HX-Location` で `#app` の再取得・置換を指示し、通常のフォームには `303 See Other` を返す．JavaScript 無効時も同じ route を利用できる．
- 永続状態の変更は `ScheduleStore` 内の broadcast channel から `/events` の SSE へ通知する．イベントは変更通知だけを持ち、トップ画面のクライアントが `/` を再取得してサーバ状態へ同期する．
- `src/app/client.js` の独自処理は SSE 後の再取得と、表示中のサーバ時刻・クールタイムの 1 秒補間に限定する．
- 静的資産は Topcoat の `asset!` で登録し、`AssetBundle` から配信する．htmx 2.0.10 もビルド時に資産へ取り込む．
- SQLite を永続状態の正とし、SQLx の接続プールを WAL モード、busy timeout 5 秒で開く．起動時にスキーマを検査し、新規 DB はトランザクション内で初期化する．
- `OriginLayer` により状態変更リクエストを同一オリジンに限定する．アプリ自身にユーザ認証は持たせず、到達制御は Tailscale に任せる．

## 給餌とスケジュールの安全性

- 手動給餌とスケジュール給餌は同じ `FeedService` を使う．プロセス内 mutex の取得後にクールタイムを再確認し、同時要求による二重駆動を防ぐ．
- 物理操作が成功した後だけ、最終給餌時刻と履歴を同じ DB トランザクションへ記録する．物理操作後の記録失敗は再給餌せず、文脈付きエラーとして扱う．
- 実機では PWM0（BCM GPIO18、物理ピン 12）を 20 ms 周期で使用する．連続回転サーボの制御パルス幅は 700～2,300 µs とし、1,500 µs を停止に使用する．給餌速度 1～100% を 1,508～2,300 µs へ線形変換して給餌方向へ回転し、回転前後に各 1 秒の停止時間を設ける．
- スケジュールはサーバのローカル日時、分精度の `YYYY-MM-DDTHH:MM` で管理し、DB の現在時刻より未来かつ重複しない日時だけを登録する．
- スケジューラは起動直後と以後 30 秒ごとに、現在の分と一致する未処理予定を原子的に 1 件確保する．確保時に暫定的な失敗理由を保存し、処理中の停止後に再実行して二重給餌しないようにする．
- 成功した予定は削除する．クールタイム中または結果不明の予定は理由を付けて残す．停止中などに予定時刻を過ぎた予定は実行せず、一覧で期限切れとして表示する．
- DB は給餌速度を含む schema version 1 のみを扱う．新規 DB はトランザクション内で初期化し、未対応の既存スキーマや実装より新しい `user_version` は起動時に拒否する．

## モジュール構成

- `src/main.rs`: 環境設定、各サービス、Topcoat ルータ、資産、スケジューラ、リスナの初期化と停止処理．
- `src/lib.rs`: Web 層とサーバ側サービスのモジュール公開．
- `src/app.rs`: layout、ページ、コンポーネント、フォーム用 DTO、POST route、htmx/通常フォーム共通の応答処理．
- `src/app/style.css`: SSR UI のスタイル．
- `src/app/client.js`: SSE 再同期と画面上の時計・クールタイム補間．
- `src/feed.rs`: サーボの実機・モックドライバと、排他制御・クールタイム・履歴記録をまとめる `FeedService`．
- `src/camera.rs`: `/camera/stream`、モック SVG、`ffmpeg` 子プロセスによる MJPEG ストリーム．
- `src/events.rs`: `/events` の Topcoat SSE route．
- `src/schedule.rs`: schedule ドメイン型、設定値の既定値と範囲、サブモジュールの公開境界．
- `src/schedule/store.rs`: SQLite 接続、問い合わせ、日時・設定検証、変更通知、予定の原子的な確保・完了・失敗処理．
- `src/schedule/migration.rs`: 新規 DB の schema version 1 初期化と対応バージョンの検査．
- `src/schedule/scheduler.rs`: 30 秒間隔の一回限り予定の実行タスク．
- `deploy/pi-auto-feeder.service`: Raspberry Pi 上の systemd unit．
- `docs/README.md`: 利用・開発・構造の入口．
- `docs/release.md`: Raspberry Pi へのリリース、DB バックアップ、確認、ロールバック手順．

## DB と設定

- `schedules`: 一回限りの必須日時と失敗理由を保存する．期限切れは取得時に現在時刻との比較で算出する．
- `feeder_status`: 単一行で最後に正常記録された給餌日時を保持する．
- `feed_history`: 成功した給餌を追記し、UI は新しい順に最大 10 件取得する．
- `settings`: 単一行でクールタイム、サーボ回転時間、給餌速度を保持する．
- クールタイムの既定値は 300 秒、範囲は 0～86,400 秒である．
- サーボ回転時間の既定値は 1,000 ms、範囲は 100～10,000 ms である．
- 給餌速度の既定値は 100%、範囲は 1～100% である．

## 実行時設定

- `DATABASE_URL`: SQLite URL．アプリ単体の既定値は `sqlite://pi-auto-feeder.sqlite3`．
- `FEEDER_MOCK`: `true` で GPIO を使用しない．未指定時は実機モード．`true` / `false` 以外は起動エラー．
- `CAMERA_MOCK`: `true` で固定 SVG を返す．未指定時は `/dev/video0` と `ffmpeg` を使用．`true` / `false` 以外は起動エラー．
- `HOST` / `PORT`: 待受アドレス．既定値は `127.0.0.1:3000`．
- 開発・自動確認では、意図しない物理操作を避けるため必ず `FEEDER_MOCK=true CAMERA_MOCK=true` を指定する．

## Web UI とルーティング

- `GET /`: カメラ、手動給餌、予定、サーバ状態、履歴を表示する．
- `GET /settings`: クールタイム、回転時間、給餌速度を表示・編集する．
- `POST /feed`: 共通給餌サービスを実行する．
- `POST /schedules`: 未来の予定を追加する．
- `POST /schedules/delete`: ID 指定で予定を削除する．
- `POST /settings`: 許容範囲内の設定を保存する．
- `GET /camera/stream`: モック SVG または `ffmpeg` の MJPEG を配信する．実機ではリクエストごとに子プロセスを起動し、切断時に終了する．
- `GET /events`: 永続状態の変更イベントと keep-alive を配信する．
- htmx の CDN URL をソースに指定しているが、リリース実行時は bundle 済み資産を配信し、ブラウザを外部 CDN に依存させない．

## 設計・実装方針

- Raspberry Pi 単体で完結する小規模な個人利用を前提とし、保守性、物理操作の安全性、既存データの保持を優先する．
- Topcoat 0.5 の page、layout、route、component、asset、htmx、SSE の標準パターンを優先する．独自 API やクライアント状態管理を重複させない．
- サーバと SQLite を正とし、クライアントは SSR 結果の再取得で同期する．
- 新しい依存や抽象化は、標準ライブラリまたは既存依存で実現できない明確な理由がある場合だけ提案する．YAGNI を守り、大規模リファクタリングは利用者の同意なく行わない．
- リリース後の DB 変更は schema version ごとの明示的な移行としてトランザクション内で行い、既存値を推測で変換しない．
- 物理操作の再試行は二重給餌につながるため、成否が不明な場合は自動再試行せず利用者の確認を求める．

## コーディング規約

- Rust API Guidelines と一般的な Rust の命名規則に従い、既存コードのスタイルを維持する．
- Rust は `cargo fmt`、Topcoat の `view!` は `topcoat fmt` で整形する．
- 通常のエラー処理に `panic!` を使わず、アプリケーションエラーには `color_eyre` を用い、Topcoat route/page 境界で利用者向けの一般化したエラーへ変換する．
- `unsafe` は必要最小限にする．秘密情報や不要な内部情報をログへ出さない．物理操作や永続化の障害を診断できる文脈は残す．
- UI 更新では htmx と 303 redirect の両方、二重送信防止、`aria-live`、キーボード操作を維持する．

## セキュリティと配置

- 既定のループバック待受を維持し、Tailscale Serve から tailnet 内だけへ公開する．Tailscale Funnel やインターネットへの直接公開は採用しない．
- 利用者の追加・削除とアクセス制御は Tailscale 側で管理する．
- systemd unit は専用ユーザ、`gpio` / `video` 補助グループ、限定した書込パス、`NoNewPrivileges` などの制約で起動する．
- 実機では `/dev/video0`、`ffmpeg`、PWM0 と `/sys/class/pwm` への適切な権限が必要である．

## ビルド・テスト

変更後は次を実行する．

```sh
cargo fmt
topcoat fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
topcoat asset bundle --release
```

- DB、スケジューラ、クールタイム、給餌処理の変更では、正常系に加えて期限切れ、重複、クールタイム、物理失敗、処理中断、スキーマバージョン不一致を確認する．
- UI または route の変更では SSR、htmx、SSE 再同期、JavaScript 無効時の 303 フォールバックを確認する．
- 自動確認はモックで行い、リリース前だけ実機で GPIO、カメラ、ローカルタイムゾーン、再起動後の DB 継続性を確認する．

## AI エージェント向け

- 実装を一次情報として確認し、この文書と `docs/README.md`、`docs/release.md` の記述を同期する．
- 不明点を推測で実装しない．特に GPIO、DB 移行、公開範囲を変える場合は利用者へ確認する．
- 利用者の未追跡ファイルや無関係な変更を保持し、必要以上の整形や構造変更を行わない．
- 説明だけで終わらせず、依頼範囲の実装と妥当な検証まで行う．
