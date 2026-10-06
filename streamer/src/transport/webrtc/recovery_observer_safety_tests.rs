use super::*;
#[tokio::test]async fn cancelled_rtp_write_marks_trace_incomplete(){
 struct Waiting;
 #[async_trait]impl RTPWriter for Waiting{async fn write(&self,_p:&Packet,_a:&Attributes)->Result<usize,webrtc::interceptor::Error>{std::future::pending().await}}
 let(tap,path)=Tap::test_file();let observer=observer(tap.clone());let writer=observer.bind_local_stream(&info(),Arc::new(Waiting)).await;
 assert!(tokio::time::timeout(std::time::Duration::from_millis(10),writer.write(&packet(),&Attributes::new())).await.is_err());tap.close();
 let footer=wait_footer(&path).await;assert_eq!(footer["cancelled_calls"],1);assert_eq!(footer["observation_complete"],false);
}
async fn wait_footer(path:&std::path::Path)->serde_json::Value{let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);loop{let text=std::fs::read_to_string(path).unwrap();if let Some(v)=text.lines().last().and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok()){if v["type"]=="footer"{return v;}}assert!(std::time::Instant::now()<deadline,"missing footer");tokio::time::sleep(std::time::Duration::from_millis(10)).await;}}
#[tokio::test]async fn in_flight_call_at_close_prevents_complete_footer(){let(tap,path)=Tap::test_file();let call=tap.begin_call();tap.close();let f=wait_footer(&path).await;assert_eq!(f["outstanding_calls"],1);assert_eq!(f["observation_complete"],false);drop(call);}
#[tokio::test]async fn omitted_metadata_marks_file_incomplete(){let(tap,path)=Tap::test_file();tap.omit(2);tap.close();let f=wait_footer(&path).await;assert_eq!(f["omitted"],2);assert_eq!(f["observation_complete"],false);}
#[tokio::test]async fn rtcp_writer_errors_preserved(){struct Failing;
 #[async_trait]impl RTCPWriter for Failing{async fn write(&self,_:&[Box<dyn RtcpPacket+Send+Sync>],_:&Attributes)->Result<usize,webrtc::interceptor::Error>{Err(webrtc::interceptor::Error::Other("rtcp-sentinel".into()))}}
 let(tap,rx)=Tap::memory(8);let writer=RtcpOut{inner:Arc::new(Failing),tap:tap.into()};let sr=SenderReport{ssrc:42,ntp_time:u64::MAX-17,rtp_time:9,..Default::default()};let packets:Vec<Box<dyn RtcpPacket+Send+Sync>>=vec![Box::new(sr.clone())];let err=writer.write(&packets,&Attributes::new()).await.unwrap_err();assert!(err.to_string().contains("rtcp-sentinel"));let e=rx.try_recv().unwrap();assert!(matches!(e.data,Data::Sr{ok:false,ntp_seconds:u32::MAX,ntp_fraction,..}if ntp_fraction==sr.ntp_time as u32));}
#[tokio::test]async fn audio_rtp_writer_is_not_instrumented(){let(t,rx)=Tap::memory(8);let o=observer(t);let sink=Arc::new(Sink::default());let mut stream=info();stream.mime_type="audio/opus".into();let writer=o.bind_local_stream(&stream,sink.clone()).await;assert_eq!(writer.write(&packet(),&Attributes::new()).await.unwrap(),777);assert!(rx.try_recv().is_err());}
