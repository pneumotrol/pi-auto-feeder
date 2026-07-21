//! モック画像または `ffmpeg` による MJPEG カメラストリームを配信する。

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use color_eyre::eyre::{Result, WrapErr, eyre};
use futures_util::{StreamExt, stream};
use leptos::prelude::LeptosOptions;
use std::{
    env::{self, VarError},
    process::Stdio,
};
use tokio::process::Command;
use tokio_util::io::ReaderStream;

const CAMERA_DEVICE: &str = "/dev/video0";
const MOCK_IMAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480" viewBox="0 0 640 480">
<rect width="640" height="480" fill="#222"/>
<text x="320" y="240" fill="#fff" font-family="sans-serif" font-size="32" text-anchor="middle" dominant-baseline="middle">Camera mock</text>
</svg>"##;

#[derive(Clone)]
/// リクエストごとに実機ストリームかモック画像を生成するカメラ設定。
pub struct Camera {
    mock: bool,
}

impl Camera {
    /// `CAMERA_MOCK` から動作モードを選ぶ。未指定時は `/dev/video0` を使用する。
    pub fn from_env() -> Result<Self> {
        let mock = match env::var("CAMERA_MOCK") {
            Ok(value) => value
                .parse::<bool>()
                .wrap_err("CAMERA_MOCK must be true or false")?,
            Err(VarError::NotPresent) => false,
            Err(error) => return Err(error).wrap_err("CAMERA_MOCK is not valid Unicode"),
        };

        Ok(Self { mock })
    }

    /// 起動ログに表示できる現在のカメラモードを返す。
    pub fn mode(&self) -> &'static str {
        if self.mock { "mock" } else { "hardware" }
    }

    /// モック SVG、または `ffmpeg` の標準出力をそのまま流す HTTP 応答を作る。
    async fn response(&self) -> Result<Response> {
        if self.mock {
            return Ok(([(header::CONTENT_TYPE, "image/svg+xml")], MOCK_IMAGE).into_response());
        }

        tokio::fs::metadata(CAMERA_DEVICE)
            .await
            .wrap_err_with(|| format!("camera device {CAMERA_DEVICE} is not available"))?;

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "v4l2",
                "-i",
                CAMERA_DEVICE,
                "-an",
                "-c:v",
                "mjpeg",
                "-f",
                "mpjpeg",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .wrap_err("failed to start ffmpeg")?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("failed to capture ffmpeg output"))?;
        // `child` をストリーム状態に保持し、クライアント切断時の Drop で ffmpeg も停止する。
        let stream = stream::unfold(
            (ReaderStream::new(stdout), child),
            |(mut reader, child)| async move {
                reader.next().await.map(|chunk| (chunk, (reader, child)))
            },
        );

        Response::builder()
            .header(
                header::CONTENT_TYPE,
                "multipart/x-mixed-replace; boundary=ffmpeg",
            )
            .body(Body::from_stream(stream))
            .wrap_err("failed to build camera response")
    }
}

/// カメラ配信用の専用ルートを Leptos と同じ Axum state 型で構築する。
pub fn router(camera: Camera) -> Router<LeptosOptions> {
    Router::new()
        .route("/camera/stream", get(stream))
        .with_state(camera)
}

/// 内部エラーをログへ残し、クライアントには詳細を公開せず 500 を返す。
async fn stream(State(camera): State<Camera>) -> Result<Response, StatusCode> {
    camera.response().await.map_err(|error| {
        eprintln!("Failed to stream camera: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
