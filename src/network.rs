// author : Vipul Vaibhaw
// organization : SpacewalkHq
// License : MIT License

use crate::parse_ip_address;
use async_trait::async_trait;
use futures::future::join_all;
use slog::info;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[async_trait]
pub trait NetworkLayer: Send + Sync {
    async fn send(
        &self,
        address: &str,
        port: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn receive(&self) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;
    async fn broadcast(
        &self,
        data: &[u8],
        addresses: Vec<String>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn open(&self) -> Result<(), Box<dyn Error + Send + Sync>>;
    async fn close(&self) -> Result<(), Box<dyn Error + Send + Sync>>;
}

#[derive(Debug, Clone)]
pub struct TCPManager {
    address: String,
    port: u16,
    listener: Arc<Mutex<Option<TcpListener>>>,
    is_open: Arc<Mutex<bool>>,
    log: slog::Logger,
}

impl TCPManager {
    pub fn new(address: String, port: u16, log: slog::Logger) -> Self {
        TCPManager {
            address,
            port,
            listener: Arc::new(Mutex::new(None)),
            is_open: Arc::new(Mutex::new(false)),
            log,
        }
    }

    async fn async_send(
        data: &[u8],
        address: SocketAddr,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut stream = TcpStream::connect(address).await?;
        stream.write_all(data).await?;
        Ok(())
    }

    async fn handle_receive(&self) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let mut data = Vec::new();
        let listener = self.listener.lock().await;
        if let Some(listener) = &*listener {
            let (mut stream, _) = listener.accept().await?;
            let mut buffer = Vec::new();
            let mut reader = tokio::io::BufReader::new(&mut stream);
            reader.read_to_end(&mut buffer).await?;
            data = buffer;
        }
        Ok(data)
    }
}

#[async_trait]
impl NetworkLayer for TCPManager {
    async fn send(
        &self,
        address: &str,
        port: &str,
        data: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let addr: SocketAddr = format!("{}:{}", address, port).parse()?;
        Self::async_send(data, addr).await?;
        Ok(())
    }

    async fn receive(&self) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        self.handle_receive().await
    }

    async fn broadcast(
        &self,
        data: &[u8],
        addresses: Vec<String>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let futures = addresses.into_iter().map(|address| {
            let (ip, port) = parse_ip_address(&address);
            let addr: SocketAddr = format!("{}:{}", ip, port).parse().unwrap();
            Self::async_send(data, addr)
        });
        join_all(futures)
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;
        Ok(())
    }

    async fn open(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut is_open = self.is_open.lock().await;
        if *is_open {
            return Err("Listener is already open".into());
        }
        let addr: SocketAddr = format!("{}:{}", self.address, self.port).parse()?;
        let listener = TcpListener::bind(addr).await?;
        *self.listener.lock().await = Some(listener);
        *is_open = true;
        info!(self.log, "Listening on {}", addr);
        Ok(())
    }

    async fn close(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut is_open = self.is_open.lock().await;
        if !*is_open {
            return Err("Listener is not open".into());
        }
        *self.listener.lock().await = None;
        *is_open = false;
        info!(self.log, "Listener closed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use slog::{o, Drain};

    use super::*;

    fn get_logger() -> slog::Logger {
        let decorator = slog_term::PlainSyncDecorator::new(std::io::stdout());
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let log = slog::Logger::root(drain, o!());
        return log;
    }

    #[tokio::test]
    async fn test_send() {
        let network = TCPManager::new("127.0.0.1".to_string(), 8082, get_logger());
        let data = vec![1, 2, 3];
        network.open().await.unwrap();
        let network_clone = network.clone();
        let handler = tokio::spawn(async move {
            loop {
                let data = network_clone.receive().await.unwrap();
                if data.is_empty() {
                    continue;
                } else {
                    assert_eq!(data, vec![1, 2, 3]);
                    break;
                }
            }
        });
        network.send("127.0.0.1", "8082", &data).await.unwrap();
        handler.await.unwrap();
    }
}
