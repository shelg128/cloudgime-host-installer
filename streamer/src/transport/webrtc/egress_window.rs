//! Opt-in bounded metadata only. No packets, endpoints or credentials retained.
#[derive(Default)]
pub(super) struct EgressWindow {
    frames:u64, packets:u64, bytes:u64, zeros:u64, errors:u64,
    queue_max_us:u64, send_max_us:u64, write_max_us:u64,
}
impl EgressWindow {
    pub fn frame(&mut self,queue_us:u64,send_us:u64){self.frames+=1;self.queue_max_us=self.queue_max_us.max(queue_us);self.send_max_us=self.send_max_us.max(send_us);}
    pub fn packet(&mut self,elapsed_us:u64,result:Option<usize>){self.write_max_us=self.write_max_us.max(elapsed_us);match result{Some(0)=>self.zeros+=1,Some(n)=>{self.packets+=1;self.bytes+=n as u64},None=>self.errors+=1}}
    pub fn take(&mut self)->Self{std::mem::take(self)}
    pub fn line(&self,label:&str,elapsed_ms:u64)->String{format!("CG_EG k={} ms={} f={} p={} b={} q={} s={} w={} z={} e={}",label,elapsed_ms,self.frames,self.packets,self.bytes,self.queue_max_us,self.send_max_us,self.write_max_us,self.zeros,self.errors)}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn records_zero_and_error_not_success(){let mut w=EgressWindow::default();w.packet(5,Some(100));w.packet(8,Some(0));w.packet(7,None);assert_eq!((w.packets,w.bytes,w.zeros,w.errors),(1,100,1,1));assert_eq!(w.write_max_us,8);}
    #[test] fn independent_frames_and_window_reset(){let mut w=EgressWindow::default();w.frame(10,20);w.frame(4,80);let first=w.take();assert_eq!((first.frames,first.queue_max_us,first.send_max_us),(2,10,80));assert_eq!(w.frames,0);assert_eq!(w.queue_max_us,0);}
    #[test] fn output_bounded_and_metadata_only(){let mut w=EgressWindow::default();for _ in 0..1000{w.frame(1500000,1500000);w.packet(10000,Some(1200));}let line=w.line("video",2000);assert!(line.len()<180);assert!(line.starts_with("CG_EG k=video"));assert!(!line.contains("token"));}
}
