use tokio::net::UdpSocket;
#[cfg(feature = "cloudgime-datagram-redundancy-qa")]
#[path = "protected_datagram.rs"]
mod protected_datagram;

use super::*;

#[async_trait]
impl Conn for UdpSocket {
    async fn connect(&self, addr: SocketAddr) -> Result<()> {
        Ok(self.connect(addr).await?)
    }

    async fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        Ok(self.recv(buf).await?)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        Ok(self.recv_from(buf).await?)
    }

    async fn send(&self, buf: &[u8]) -> Result<usize> {
        Ok(self.send(buf).await?)
    }

    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> Result<usize> {
        let sent = self.send_to(buf, target).await?;
        #[cfg(feature = "cloudgime-datagram-redundancy-qa")]
        if sent == buf.len() && protected_datagram::eligible(buf) {
            // Exact protected bytes, never invoke SRTP encryption twice. No
            // retry/wait if the socket is not currently writable.
            let copied = self.try_send_to(buf, target).is_ok_and(|n| n == buf.len());
            static STATS: std::sync::OnceLock<protected_datagram::Stats> = std::sync::OnceLock::new();
            if let Some(line) = STATS.get_or_init(Default::default).record(sent, copied) { log::info!("{line}"); }
        }
        Ok(sent)
    }

    fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.local_addr()?)
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        None
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &(dyn std::any::Any + Send + Sync) {
        self
    }
}
