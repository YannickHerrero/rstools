use std::sync::Arc;

use anyhow::{Context, Result};
use russh::client;
use russh::keys;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Configuration for an SSH tunnel.
#[derive(Debug, Clone)]
pub struct SshTunnelConfig {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub private_key_path: String,
    pub passphrase: Option<String>,
    pub remote_host: String,
    pub remote_port: u16,
}

/// Create an SSH tunnel that forwards a local port to a remote host:port.
/// Returns (local_port, background_task_handle).
pub async fn create_ssh_tunnel(config: &SshTunnelConfig) -> Result<(u16, JoinHandle<()>)> {
    // Load private key
    let key_path = shellexpand_tilde(&config.private_key_path);
    let key_pair = keys::load_secret_key(&key_path, config.passphrase.as_deref())
        .context("Failed to load SSH private key")?;

    // Connect to SSH server
    let ssh_config = Arc::new(client::Config::default());
    let handler = TunnelHandler;
    let mut session = client::connect(
        ssh_config,
        (config.ssh_host.as_str(), config.ssh_port),
        handler,
    )
    .await
    .context("Failed to connect to SSH server")?;

    // Authenticate
    let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(
        Arc::new(key_pair),
        None, // Use default hash algorithm
    );

    let auth_result = session
        .authenticate_publickey(&config.ssh_username, key_with_hash)
        .await
        .context("SSH authentication failed")?;

    if !auth_result.success() {
        anyhow::bail!("SSH authentication rejected by server");
    }

    // Bind local listener on random port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind local tunnel port")?;
    let local_port = listener.local_addr()?.port();

    let remote_host = config.remote_host.clone();
    let remote_port = config.remote_port as u32;
    let session = Arc::new(session);

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut local_stream, _)) = listener.accept().await else {
                break;
            };

            let session = session.clone();
            let remote_host = remote_host.clone();

            tokio::spawn(async move {
                let channel = match session
                    .channel_open_direct_tcpip(&remote_host, remote_port, "127.0.0.1", 0)
                    .await
                {
                    Ok(ch) => ch,
                    Err(_) => return,
                };

                let mut stream = channel.into_stream();

                let mut local_buf = vec![0u8; 8192];
                let mut remote_buf = vec![0u8; 8192];

                loop {
                    tokio::select! {
                        result = local_stream.read(&mut local_buf) => {
                            match result {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if stream.write_all(&local_buf[..n]).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        result = stream.read(&mut remote_buf) => {
                            match result {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if local_stream.write_all(&remote_buf[..n]).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
    });

    Ok((local_port, handle))
}

/// Minimal SSH client handler for tunneling (no interactive shell needed).
struct TunnelHandler;

impl client::Handler for TunnelHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &keys::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // Accept all server keys (like StrictHostKeyChecking=no)
        Ok(true)
    }
}

/// Expand ~ to home directory in paths.
fn shellexpand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return format!("{}/{}", home, rest);
        }
    }
    path.to_string()
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok()
}
