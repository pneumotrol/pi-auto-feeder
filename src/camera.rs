//! モック画像または `ffmpeg` による MJPEG カメラストリームを Topcoat から配信する。

use color_eyre::eyre::{Result as EyreResult, WrapErr, eyre};
use futures_util::{StreamExt, stream};
use http_body_util::StreamBody;
use std::{
    env::{self, VarError},
    process::Stdio,
};
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{Body, Response, header, route},
};

const CAMERA_DEVICE: &str = "/dev/video0";
const MOCK_IMAGE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480" viewBox="0 0 640 480">
<rect width="640" height="480" fill="#18181b"/>
<text x="320" y="240" fill="#fafafa" font-family="sans-serif" font-size="32" text-anchor="middle" dominant-baseline="middle">Camera mock</text>
</svg>"##;

#[derive(Clone)]
/// リクエストごとに実機ストリームかモック画像を生成するカメラ設定。
pub struct Camera {
    mock: bool,
}

impl Camera {
    /// `CAMERA_MOCK` から動作モードを選ぶ。未指定時は `/dev/video0` を使用する。
    pub fn from_env() -> EyreResult<Self> {
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

    async fn response(&self) -> EyreResult<Response> {
        if self.mock {
            return Response::builder()
                .header(header::CONTENT_TYPE, "image/svg+xml")
                .body(Body::from(MOCK_IMAGE))
                .wrap_err("failed to build mock camera response");
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
        let chunks = stream::unfold(
            (ReaderStream::new(stdout), child),
            |(mut reader, child)| async move {
                reader
                    .next()
                    .await
                    .map(|chunk| (chunk.map(http_body::Frame::data), (reader, child)))
            },
        );
        let body = Body::new(StreamBody::new(chunks));
        Response::builder()
            .header(
                header::CONTENT_TYPE,
                "multipart/x-mixed-replace; boundary=ffmpeg",
            )
            .body(body)
            .wrap_err("failed to build camera response")
    }
}

#[route(GET "/camera/stream")]
async fn camera_stream(cx: &Cx) -> Result<Response> {
    app_context::<Camera>(cx).response().await.map_err(|error| {
        eprintln!("Failed to stream camera: {error}");
        std::io::Error::other("camera stream failed").into()
    })
}
