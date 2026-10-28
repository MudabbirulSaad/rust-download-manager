use anyhow::{Result, bail};
use rdm_protocol::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub async fn write_frame<W: AsyncWriteExt + Unpin, T: serde::Serialize>(
    stream: &mut W,
    value: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    anyhow::ensure!(bytes.len() <= MAX_FRAME, "Message exceeds 16 MiB");
    stream.write_u32_le(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}
pub async fn read_frame<R: AsyncReadExt + Unpin, T: serde::de::DeserializeOwned>(
    stream: &mut R,
) -> Result<T> {
    let len = stream.read_u32_le().await? as usize;
    anyhow::ensure!(len <= MAX_FRAME && len > 0, "Invalid message length");
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub async fn request(command: Command) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(crate::paths::socket_path()).await?;
    let id = uuid::Uuid::new_v4().to_string();
    write_frame(
        &mut stream,
        &Request {
            version: VERSION,
            id: id.clone(),
            command,
        },
    )
    .await?;
    let response: Response =
        tokio::time::timeout(std::time::Duration::from_secs(15), read_frame(&mut stream)).await??;
    anyhow::ensure!(
        response.id == id && response.version == VERSION,
        "Service protocol mismatch"
    );
    if !response.ok {
        bail!(
            "{}",
            response.error.unwrap_or_else(|| "Service error".into())
        );
    }
    Ok(response.data)
}
