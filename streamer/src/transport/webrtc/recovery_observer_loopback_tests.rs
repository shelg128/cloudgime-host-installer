use super::*;
use webrtc::{api::{APIBuilder,media_engine::MediaEngine,setting_engine::SettingEngine,interceptor_registry::register_default_interceptors},interceptor::registry::Registry,peer_connection::{configuration::RTCConfiguration,peer_connection_state::RTCPeerConnectionState},ice::network_type::NetworkType,rtp_transceiver::rtp_codec::RTCRtpCodecCapability,track::track_local::{track_local_static_rtp::TrackLocalStaticRTP,TrackLocalWriter}};

#[tokio::test]
async fn encrypted_peer_loopback_observes_final_identity_sr_and_rr(){
 let _=rustls::crypto::aws_lc_rs::default_provider().install_default();
 let(tap,trace_path)=Tap::test_file();let mut media=MediaEngine::default();media.register_default_codecs().unwrap();
 let mut registry=Registry::new();registry.add(Box::new(ObserverBuilder{tap:tap.clone(),consumed:AtomicBool::new(false)}));registry=register_default_interceptors(registry,&mut media).unwrap();
 let settings=||{let mut s=SettingEngine::default();s.set_network_types(vec![NetworkType::Udp4]);s.set_include_loopback_candidate(true);s.set_ip_filter(Box::new(|ip|ip.is_loopback()));s};
 let a=Arc::new(APIBuilder::new().with_setting_engine(settings()).with_media_engine(media).with_interceptor_registry(registry).build().new_peer_connection(RTCConfiguration::default()).await.unwrap());
 let mut media=MediaEngine::default();media.register_default_codecs().unwrap();let registry=register_default_interceptors(Registry::new(),&mut media).unwrap();
 let b=Arc::new(APIBuilder::new().with_setting_engine(settings()).with_media_engine(media).with_interceptor_registry(registry).build().new_peer_connection(RTCConfiguration::default()).await.unwrap());
 let(tx,mut received)=tokio::sync::mpsc::channel(32);
 b.on_track(Box::new(move|track,receiver,_transceiver|{let tx=tx.clone();Box::pin(async move{tokio::spawn(async move{let mut buf=[0u8;1500];while receiver.read(&mut buf).await.is_ok(){}});tokio::spawn(async move{for _ in 0..20{match track.read_rtp().await{Ok((packet,_))=>{let _=tx.send(packet).await;},Err(_)=>break}}});})}));
 let track=Arc::new(TrackLocalStaticRTP::new(RTCRtpCodecCapability{mime_type:"video/H264".into(),clock_rate:90000,..Default::default()},"synthetic".into(),"local".into()));let sender=a.add_track(track.clone()).await.unwrap();
 let read_task=tokio::spawn(async move{let mut buf=[0;1500];while sender.read(&mut buf).await.is_ok(){}});
 let mut gather=a.gathering_complete_promise().await;let offer=a.create_offer(None).await.unwrap();a.set_local_description(offer).await.unwrap();tokio::time::timeout(std::time::Duration::from_secs(5),gather.recv()).await.unwrap();
 b.set_remote_description(a.local_description().await.unwrap()).await.unwrap();let mut gather=b.gathering_complete_promise().await;let answer=b.create_answer(None).await.unwrap();b.set_local_description(answer).await.unwrap();tokio::time::timeout(std::time::Duration::from_secs(5),gather.recv()).await.unwrap();a.set_remote_description(b.local_description().await.unwrap()).await.unwrap();
 let test=async{
  tokio::time::timeout(std::time::Duration::from_secs(5),async{while a.connection_state()!=RTCPeerConnectionState::Connected||b.connection_state()!=RTCPeerConnectionState::Connected{tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.expect("loopback ICE/DTLS");
  for i in 0..20u16{let mut p=packet();p.header.ssrc=0;p.header.payload_type=0;p.header.sequence_number=100+i;p.header.timestamp=90000+u32::from(i)*3000;track.write_rtp(&p).await.unwrap();tokio::time::sleep(std::time::Duration::from_millis(5)).await;}
  let mut actual=Vec::new();for _ in 0..20{actual.push(tokio::time::timeout(std::time::Duration::from_secs(2),received.recv()).await.unwrap().unwrap());}
  tokio::time::sleep(std::time::Duration::from_millis(2300)).await;
  actual
 };
 let actual=test.await;a.close().await.unwrap();b.close().await.unwrap();read_task.abort();let rows:Vec<recorder::Event>={let until=std::time::Instant::now()+std::time::Duration::from_secs(2);loop{let text=std::fs::read_to_string(&trace_path).unwrap();let lines:Vec<serde_json::Value>=text.lines().filter_map(|s|serde_json::from_str(s).ok()).collect();if let Some(footer)=lines.last().filter(|f|f["type"]=="footer"){assert_eq!(footer["observation_complete"],true);break lines.into_iter().filter(|x|x.get("data").is_some()).map(|x|serde_json::from_value(x).unwrap()).collect();}assert!(std::time::Instant::now()<until,"writer footer deadline");tokio::time::sleep(std::time::Duration::from_millis(20)).await;}};
 assert_eq!(actual.len(),20);for p in &actual{assert_eq!(p.payload,packet().payload);assert_ne!(p.header.ssrc,0);assert_ne!(p.header.payload_type,0);assert!(rows.iter().any(|e|matches!(e.data,Data::Rtp{ssrc,seq,rtp,pt,ok:true,..}if ssrc==p.header.ssrc&&seq==p.header.sequence_number&&rtp==p.header.timestamp&&pt==p.header.payload_type)));}
 let ssrc=actual[0].header.ssrc;assert!(rows.iter().any(|e|matches!(e.data,Data::Sr{ssrc:s,ok:true,..}if s==ssrc)));assert!(rows.iter().any(|e|matches!(e.data,Data::Rr{ssrc:s,lsr,..}if s==ssrc&&lsr!=0)));
 let matched:Vec<_>=rows.iter().filter_map(|rr|{if let Data::Rr{ssrc:s,lsr,dlsr,..}=rr.data {if s!=ssrc||lsr==0{return None;}let sr=rows.iter().find(|sr|matches!(sr.data,Data::Sr{ssrc:t,compact_ntp:n,ok:true,..}if t==s&&n==lsr)&&sr.completed_us<=rr.completed_us)?;let dwell=f64::from(dlsr)*1000.0/65536.0;Some(serde_json::json!({"lsr":lsr,"sr_start_us":sr.started_us,"sr_complete_us":sr.completed_us,"rr_read_us":rr.completed_us,"dlsr_ms":dwell,"sdk_call_rtt_lower_ms":(rr.completed_us-sr.completed_us)as f64/1000.0-dwell,"sdk_call_rtt_upper_ms":(rr.completed_us-sr.started_us)as f64/1000.0-dwell}))}else{None}}).collect();assert!(!matched.is_empty(),"actual matching SR/RR required");
 if let Ok(dir)=std::env::var("CG_QA_TEST_EVIDENCE_DIR"){
  let path=std::path::PathBuf::from(dir).join(format!("sdk-loopback-{}.json",std::process::id()));let file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();serde_json::to_writer_pretty(file,&serde_json::json!({"scope":"actual SDK PeerConnection UDP loopback / ICE DTLS SRTP, synthetic payload not decodable video; not physical cloud gaming","packets_received":actual.len(),"final_ssrc":ssrc,"synthetic_payload_preserved":true,"peer_a_closed":true,"peer_b_closed":true,"events":rows,"matched_sr_rr":matched,"trace_file":trace_path,"snapshot_source":"actual bounded JSONL writer with complete footer, replayed into event records","quality_accepted":false})).unwrap();
 }
}
