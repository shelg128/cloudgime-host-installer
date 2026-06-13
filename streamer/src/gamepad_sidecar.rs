use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use anyhow::{Context, Result};
use moonlight_common::stream::control::ControllerType;
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, Command},
    sync::Mutex,
    task::spawn,
};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GamepadProfile {
    Xbox,
    Ds4,
}

impl From<ControllerType> for GamepadProfile {
    fn from(value: ControllerType) -> Self {
        match value {
            ControllerType::PlayStation => Self::Ds4,
            _ => Self::Xbox,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GamepadBrokerCommand {
    Connect {
        id: u8,
        profile: GamepadProfile,
    },
    Disconnect {
        id: u8,
    },
    State {
        id: u8,
        buttons: u32,
        left_trigger: u8,
        right_trigger: u8,
        left_stick_x: i16,
        left_stick_y: i16,
        right_stick_x: i16,
        right_stick_y: i16,
    },
    Stop,
}

pub struct GamepadBroker {
    path: PathBuf,
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
}

impl GamepadBroker {
    pub async fn spawn(path: &Path) -> Result<Self> {
        let mut command = Command::new(path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn gamepad sidecar {}", path.display()))?;

        let stdin = child
            .stdin
            .take()
            .context("gamepad sidecar missing stdin")?;

        if let Some(stderr) = child.stderr.take() {
            spawn(log_stderr(stderr));
        }

        info!("[Gamepad Broker]: spawned {}", path.display());

        Ok(Self {
            path: path.to_path_buf(),
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
        })
    }

    async fn send(&self, command: GamepadBrokerCommand) -> Result<()> {
        let mut payload =
            serde_json::to_string(&command).context("failed to encode gamepad broker command")?;
        payload.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(payload.as_bytes()).await.with_context(|| {
            format!(
                "failed to write command to gamepad sidecar {}",
                self.path.display()
            )
        })?;
        stdin.flush().await.with_context(|| {
            format!(
                "failed to flush command to gamepad sidecar {}",
                self.path.display()
            )
        })?;
        Ok(())
    }

    pub async fn connect(&self, id: u8, profile: GamepadProfile) -> Result<()> {
        self.send(GamepadBrokerCommand::Connect { id, profile })
            .await
    }

    pub async fn disconnect(&self, id: u8) -> Result<()> {
        self.send(GamepadBrokerCommand::Disconnect { id }).await
    }

    pub async fn update_state(
        &self,
        id: u8,
        buttons: u32,
        left_trigger: u8,
        right_trigger: u8,
        left_stick_x: i16,
        left_stick_y: i16,
        right_stick_x: i16,
        right_stick_y: i16,
    ) -> Result<()> {
        self.send(GamepadBrokerCommand::State {
            id,
            buttons,
            left_trigger,
            right_trigger,
            left_stick_x,
            left_stick_y,
            right_stick_x,
            right_stick_y,
        })
        .await
    }

    pub async fn shutdown(&self) {
        if let Err(err) = self.send(GamepadBrokerCommand::Stop).await {
            warn!("[Gamepad Broker]: failed to send stop: {err}");
        }
        let mut child = self.child.lock().await;
        if let Err(err) = child.kill().await {
            warn!("[Gamepad Broker]: failed to kill child: {err}");
        }
        if let Err(err) = child.wait().await {
            warn!("[Gamepad Broker]: failed to wait for child: {err}");
        }
    }
}

async fn log_stderr(stderr: ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => info!("[Gamepad Broker] {line}"),
            Ok(None) => return,
            Err(err) => {
                warn!("[Gamepad Broker]: failed to read stderr: {err}");
                return;
            }
        }
    }
}
