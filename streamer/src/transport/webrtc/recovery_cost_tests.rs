use super::*;
use std::time::{Instant,Duration};
struct Counting{count:AtomicUsize,bytes:AtomicUsize}
#[async_trait]impl RTPWriter for Counting{async fn write(&self,p:&Packet,_a:&Attributes)->Result<usize,webrtc::interceptor::Error>{assert_eq!(p.payload.len(),1200);assert!(p.payload.iter().all(|v|*v==0x55));self.count.fetch_add(1,Ordering::Relaxed);self.bytes.fetch_add(p.payload.len(),Ordering::Relaxed);Ok(1240)}}
fn percentile(a:&[u128],p:usize)->u128{a[((a.len()-1)*p)/100]}
#[tokio::test]
#[ignore="explicit predeclared producer overhead qualification"]
async fn observer_producer_cost(){
 let dir=std::path::PathBuf::from(std::env::var("CG_QA_TEST_EVIDENCE_DIR").expect("evidence directory"));let mut results=Vec::new();
 for round in 0..3{let modes=if round%2==0{["plain","attributes","recording"]}else{["recording","attributes","plain"]};for mode in modes{
  let sink=Arc::new(Counting{count:AtomicUsize::new(0),bytes:AtomicUsize::new(0)});let mut owner=None;let mut file=None;
  let writer:Arc<dyn RTPWriter+Send+Sync>=if mode=="recording"{let(tap,path)=Tap::test_file();let o=observer(tap);let w=o.bind_local_stream(&info(),sink.clone()).await;owner=Some(o);file=Some(path);w}else{sink.clone()};
  let began=Instant::now();let mut samples=Vec::new();for i in 0..4000u32{let target=began+Duration::from_micros(i as u64*500);let left=target.saturating_duration_since(Instant::now());if !left.is_zero(){tokio::time::sleep(left).await;}
   let p=Packet{header:Header{version:2,ssrc:42,payload_type:98,sequence_number:i as u16,timestamp:(i/20)*3000,marker:i%20==19,..Default::default()},payload:Bytes::from(vec![0x55;1200])};
   let begin=Instant::now();let attrs=if mode=="plain"{Attributes::new()}else{use crate::transport::webrtc::recovery_frame::{FRAME,INDEX,COUNT,REFERENCE};Attributes::from([(FRAME,(i/20+1)as usize),(INDEX,(i%20)as usize),(COUNT,20),(REFERENCE,2)])};assert_eq!(writer.write(&p,&attrs).await.unwrap(),1240);samples.push(begin.elapsed().as_nanos());
  }
  let elapsed=began.elapsed().as_micros();if let Some(o)=owner{o.close().await.unwrap();}assert_eq!(sink.count.load(Ordering::Relaxed),4000);assert_eq!(sink.bytes.load(Ordering::Relaxed),4_800_000);samples.sort_unstable();
  let footer=if let Some(path)=file.as_ref(){let deadline=Instant::now()+Duration::from_secs(2);loop{let text=std::fs::read_to_string(path).unwrap();if let Some(f)=text.lines().last().and_then(|x|serde_json::from_str::<serde_json::Value>(x).ok()){if f["type"]=="footer"{break Some(f);}}assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(10)).await;}}else{None};
  results.push(serde_json::json!({"round":round,"mode":mode,"packets":4000,"payload_bytes":4800000,"elapsed_us":elapsed,"p50_ns":percentile(&samples,50),"p95_ns":percentile(&samples,95),"p99_ns":percentile(&samples,99),"max_ns":samples.last(),"trace":file,"footer":footer}));
 }}
 let active:Vec<_>=results.iter().filter(|r|r["mode"]=="recording").collect();let mut delta:Vec<f64>=(0..3).map(|round|{let a=results.iter().find(|r|r["mode"]=="recording"&&r["round"]==round).unwrap();let b=results.iter().find(|r|r["mode"]=="attributes"&&r["round"]==round).unwrap();a["p95_ns"].as_f64().unwrap()-b["p95_ns"].as_f64().unwrap()}).collect();delta.sort_by(f64::total_cmp);
 let pass=active.iter().all(|r|r["p99_ns"].as_u64().unwrap()<1_000_000&&r["footer"]["observation_complete"]==true)&&delta[1]<250000.0;
 let out=serde_json::json!({"local_screen_pass":pass,"median_active_minus_attributes_p95_ns":delta[1],"runs":results,"physical_overhead_qualified":false,"scope":"paced producer-call microbenchmark, counting inner writer not codec/network/GPU; metadata serialization worker concurrent; no causality or phone quality claim"});let path=dir.join(format!("producer-cost-{}.json",std::process::id()));let f=std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();serde_json::to_writer_pretty(f,&out).unwrap(); // Evidence retained even when threshold is not met; consumer decides qualification.
}
