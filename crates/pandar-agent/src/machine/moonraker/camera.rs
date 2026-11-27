use std::{process::Stdio, time::Duration};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
    time::{MissedTickBehavior, interval, timeout},
};

use super::{
    MoonrakerCameraSource, MoonrakerClient, client::Webcam, http_error::MoonrakerHttpError,
};
use crate::machine::camera::{ffmpeg_executable, mjpeg_part, take_jpeg_frame};

const FRAME_INTERVAL: Duration = Duration::from_millis(200);
const STREAM_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) async fn resolve_camera(
    client: &MoonrakerClient,
) -> anyhow::Result<MoonrakerCameraSource> {
    let webcam: Webcam = client
        .webcams()
        .await
        .context("list Moonraker webcams")?
        .into_iter()
        .find(|webcam| webcam.enabled && !webcam.snapshot_url.trim().is_empty())
        .context("no enabled Moonraker webcam with a snapshot URL is configured")?;
    let snapshot_url = webcam.snapshot_url.trim().to_owned();
    let client = camera_client(client, client.default_port_client(), &snapshot_url).await?;
    let stream_url = mjpeg_stream_url(&client, &snapshot_url, webcam.stream_url.trim()).await;
    Ok(MoonrakerCameraSource {
        client,
        snapshot_url,
        stream_url,
    })
}

/// The webcam's multipart MJPEG stream, or `None` when only snapshots exist.
/// Moonraker's `stream_url` is whatever the webcam service advertises (camera-
/// streamer publishes its WebRTC page there), so it is trusted only when it
/// answers with a multipart response; otherwise the conventional MJPEG paths
/// next to the snapshot URL are probed the same way.
async fn mjpeg_stream_url(
    client: &MoonrakerClient,
    snapshot_url: &str,
    advertised: &str,
) -> Option<String> {
    let mut candidates = Vec::new();
    if !advertised.is_empty() {
        candidates.push(advertised.to_owned());
    }
    if let Some((directory, _)) = snapshot_url.rsplit_once('/') {
        candidates.push(format!("{directory}/stream.mjpg"));
        candidates.push(format!("{directory}/stream"));
        candidates.push(format!("{directory}/?action=stream"));
    }
    for candidate in candidates {
        if serves_multipart(client, &candidate).await {
            return Some(candidate);
        }
    }
    None
}

async fn serves_multipart(client: &MoonrakerClient, url: &str) -> bool {
    let response = timeout(STREAM_PROBE_TIMEOUT, client.open_stream(url)).await;
    matches!(
        response,
        Ok(Ok(response)) if response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|content_type| content_type.starts_with("multipart/"))
    )
}

/// Webcams are usually served by the Fluidd/Mainsail nginx on port 80 rather
/// than by Moonraker's own port, so a snapshot the API port does not serve is
/// fetched through `fallback` (the port-80 client) instead. A connection
/// failure on the API port is propagated as is: the webcam list was just read
/// from that port, so the failure is not about the camera location.
pub(super) async fn camera_client(
    client: &MoonrakerClient,
    fallback: Option<MoonrakerClient>,
    snapshot_url: &str,
) -> anyhow::Result<MoonrakerClient> {
    let Some(fallback) = fallback else {
        return Ok(client.clone());
    };
    match client.get_bytes(snapshot_url).await {
        Ok(_) => Ok(client.clone()),
        Err(error) if error.is::<MoonrakerHttpError>() => {
            fallback.get_bytes(snapshot_url).await.with_context(|| {
                format!(
                    "fetch Moonraker camera snapshot from {}",
                    fallback.base_url()
                )
            })?;
            Ok(fallback)
        }
        Err(error) => Err(error),
    }
}

impl MoonrakerCameraSource {
    pub async fn stream_mjpeg(self, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        match &self.stream_url {
            Some(stream_url) => self.pipe_stream(stream_url, &sender).await,
            None => self.poll_snapshots(&sender).await,
        }
    }

    /// Produces fragmented MP4 with ffmpeg: by reading the camera's own MJPEG
    /// stream when it exposes one, otherwise by re-encoding polled snapshots.
    pub async fn stream_fragmented_mp4(self, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        let Some(stream_url) = &self.stream_url else {
            return self.encode_snapshots(sender).await;
        };
        let url = self.client.resolve_url(stream_url);
        let mut command = Command::new(ffmpeg_executable());
        command.args([
            "-use_wallclock_as_timestamps",
            "1",
            "-f",
            "mjpeg",
            "-i",
            &url,
        ]);
        encode_fragmented_mp4(&mut command);
        forward_ffmpeg(command, sender).await
    }

    /// Re-frames the camera's multipart stream with Pandar's own boundary.
    async fn pipe_stream(
        &self,
        stream_url: &str,
        sender: &mpsc::Sender<Vec<u8>>,
    ) -> anyhow::Result<()> {
        let mut response = self
            .client
            .open_stream(stream_url)
            .await
            .context("open Moonraker camera stream")?;
        let mut buffer = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("read Moonraker camera stream")?
        {
            buffer.extend_from_slice(&chunk);
            while let Some(frame) = take_jpeg_frame(&mut buffer) {
                if sender.send(mjpeg_part(&frame)).await.is_err() {
                    return Ok(());
                }
            }
        }
        bail!("Moonraker camera stream closed")
    }

    async fn poll_snapshots(&self, sender: &mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        let mut ticker = frame_ticker();
        loop {
            ticker.tick().await;
            let frame = self.snapshot().await?;
            if sender.send(mjpeg_part(&frame)).await.is_err() {
                return Ok(());
            }
        }
    }

    async fn encode_snapshots(self, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        let mut command = Command::new(ffmpeg_executable());
        command.args(["-f", "mjpeg", "-framerate", "5", "-i", "pipe:0"]);
        encode_fragmented_mp4(&mut command);
        command.stdin(Stdio::piped());
        let mut process = spawn_ffmpeg(command)?;
        let mut stdin = process
            .stdin
            .take()
            .context("ffmpeg stdin should be piped")?;
        let feed = async {
            let mut ticker = frame_ticker();
            loop {
                ticker.tick().await;
                let frame = self.snapshot().await?;
                stdin
                    .write_all(&frame)
                    .await
                    .context("write camera snapshot to ffmpeg")?;
            }
        };
        tokio::select! {
            result = feed => result,
            result = forward_ffmpeg_output(process, sender) => result,
        }
    }

    async fn snapshot(&self) -> anyhow::Result<Vec<u8>> {
        self.client
            .get_bytes(&self.snapshot_url)
            .await
            .context("fetch Moonraker camera snapshot")
    }
}

/// Spawns ffmpeg and forwards its output until the consumer stops reading.
async fn forward_ffmpeg(command: Command, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
    let process = spawn_ffmpeg(command)?;
    forward_ffmpeg_output(process, sender).await
}

async fn forward_ffmpeg_output(
    mut process: Child,
    sender: mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<()> {
    let mut stdout = process
        .stdout
        .take()
        .context("ffmpeg stdout should be piped")?;
    let mut chunk = vec![0_u8; 16384];
    loop {
        let read = stdout
            .read(&mut chunk)
            .await
            .context("read ffmpeg camera output")?;
        if read == 0 {
            let status = process
                .wait()
                .await
                .context("wait for ffmpeg camera stream")?;
            bail!("ffmpeg camera stream exited with {status}");
        }
        if sender.send(chunk[..read].to_vec()).await.is_err() {
            return Ok(());
        }
    }
}

fn spawn_ffmpeg(mut command: Command) -> anyhow::Result<Child> {
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    command.kill_on_drop(true);
    command
        .spawn()
        .with_context(|| format!("spawn ffmpeg executable {}", ffmpeg_executable()))
}

fn frame_ticker() -> tokio::time::Interval {
    let mut ticker = interval(FRAME_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

fn encode_fragmented_mp4(command: &mut Command) {
    command
        .args(["-an", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-tune", "zerolatency", "-pix_fmt", "yuv420p"])
        .args(["-g", "10", "-keyint_min", "10", "-sc_threshold", "0"])
        .args(["-movflags", "frag_keyframe+empty_moov+default_base_moof"])
        .args(["-f", "mp4", "-"]);
}
