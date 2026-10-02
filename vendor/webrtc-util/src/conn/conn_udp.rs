#[cfg(all(feature="cloudgime-total-egress-qa",feature="cloudgime-recovery-delay-qa"))]
compile_error!("Total-egress QA and delayed-copy QA require separate experiments; combined accounting is not validated.");

#[cfg(feature = "cloudgime-datagram-redundancy-qa")]
compile_error!("Unscoped QA datagram duplication is prohibited on the scoped recovery branch; use archived experiment bc18bf7 for evidence only.");

use tokio::net::UdpSocket;
#[cfg(all(feature = "cloudgime-datagram-redundancy-qa", not(feature = "cloudgime-scoped-direct-recovery")))]
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
        #[cfg(all(feature = "cloudgime-datagram-redundancy-qa", not(feature = "cloudgime-scoped-direct-recovery")))]
        if sent == buf.len() && protected_datagram::eligible(buf) {
            // Exact protected bytes, never invoke SRTP encryption twice. No
            // retry/wait if the socket is not currently writable.
            let copied = self.try_send_to(buf, target).is_ok_and(|n| n == buf.len());
            static STATS: std::sync::OnceLock<protected_datagram::Stats> = std::sync::OnceLock::new();
            if let Some(line) = STATS.get_or_init(Default::default).record(sent, copied) { log::info!("{line}"); }
        }
        #[cfg(feature = "cloudgime-scoped-direct-recovery")]
        if sent == buf.len() {
            if let Ok(local) = self.local_addr() {
            #[cfg(feature="cloudgime-total-egress-qa")]
            let observed=super::media_egress_qa::observe_success(buf,target,local.port());
            #[cfg(not(feature="cloudgime-recovery-delay-qa"))]
            if let Some((budget, cost)) = super::scoped_recovery::permit(buf, target, local.port()) {
                #[cfg(feature="cloudgime-total-egress-qa")]
                if !observed.as_ref().is_some_and(|l|l.reserve_copy(target,local.port(),cost)){return Ok(sent);}
                let copied = self.try_send_to(buf, target).is_ok_and(|n| n == buf.len());
                budget.result(cost, copied);
                #[cfg(feature="cloudgime-total-egress-qa")]
                if let Some(l)=observed{l.copy_result(cost,copied);}
            }
            #[cfg(feature="cloudgime-recovery-delay-qa")]
            if let Some((budget, bytes, cost))=super::scoped_recovery::delayed_copy(buf,target,local.port()){
                let copied=self.try_send_to(&bytes,target).is_ok_and(|n|n==bytes.len());
                budget.result(cost,copied);
            }
            }
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
