use std::{process::Stdio, time::Duration};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::mpsc,
    time::{MissedTickBehavior, interval},
};

use super::{MoonrakerCameraSource, MoonrakerClient};
use crate::machine::camera::{ffmpeg_executable, mjpeg_part};

const FRAME_INTERVAL: Duration = Duration::from_millis(200);

pub(super) async fn resolve_camera(
    client: &MoonrakerClient,
) -> anyhow::Result<MoonrakerCameraSource> {
    let webcam = client
        .webcams()
        .await
        .context("list Moonraker webcams")?
        .into_iter()
        .find(|webcam| webcam.enabled && !webcam.snapshot_url.trim().is_empty())
        .context("no enabled Moonraker webcam with a snapshot URL is configured")?;
    Ok(MoonrakerCameraSource {
        client: client.clone(),
        snapshot_url: webcam.snapshot_url.trim().to_owned(),
    })
}

impl MoonrakerCameraSource {
    pub async fn stream_mjpeg(self, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        let mut ticker = frame_ticker();
        loop {
            ticker.tick().await;
            let frame = self.snapshot().await?;
            if sender.send(mjpeg_part(&frame)).await.is_err() {
                return Ok(());
            }
        }
    }

    /// Re-encodes polled JPEG snapshots into fragmented MP4 with ffmpeg.
    pub async fn stream_fragmented_mp4(self, sender: mpsc::Sender<Vec<u8>>) -> anyhow::Result<()> {
        let mut process = ffmpeg_jpeg_to_fragmented_mp4_command()
            .spawn()
            .with_context(|| format!("spawn ffmpeg executable {}", ffmpeg_executable()))?;
        let mut stdin = process
            .stdin
            .take()
            .context("ffmpeg stdin should be piped")?;
        let mut stdout = process
            .stdout
            .take()
            .context("ffmpeg stdout should be piped")?;

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
        let forward = async {
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
        };
        tokio::select! {
            result = feed => result,
            result = forward => result,
        }
    }

    async fn snapshot(&self) -> anyhow::Result<Vec<u8>> {
        self.client
            .get_bytes(&self.snapshot_url)
            .await
            .context("fetch Moonraker camera snapshot")
    }
}

fn frame_ticker() -> tokio::time::Interval {
    let mut ticker = interval(FRAME_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

fn ffmpeg_jpeg_to_fragmented_mp4_command() -> Command {
    let mut command = Command::new(ffmpeg_executable());
    command
        .args(["-f", "mjpeg", "-framerate", "5", "-i", "pipe:0"])
        .args(["-an", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-tune", "zerolatency", "-pix_fmt", "yuv420p"])
        .args(["-g", "10", "-keyint_min", "10", "-sc_threshold", "0"])
        .args(["-movflags", "frag_keyframe+empty_moov+default_base_moof"])
        .args(["-f", "mp4", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}
