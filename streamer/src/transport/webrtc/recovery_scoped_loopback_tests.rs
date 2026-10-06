use super::*;
use webrtc::{api::{APIBuilder,media_engine::MediaEngine,setting_engine::SettingEngine,interceptor_registry::register_default_interceptors},interceptor::registry::Registry,peer_connection::{configuration::RTCConfiguration,peer_connection_state::RTCPeerConnectionState},ice::network_type::NetworkType,rtp_transceiver::rtp_codec::RTCRtpCodecCapability,track::track_local::{track_local_static_rtp::TrackLocalStaticRTP,TrackLocalWriter}};

#[tokio::test]
async fn scoped_ready_encrypted_peer_preserves_frame_mapping(){
 let _=rustls::crypto::aws_lc_rs::default_provider().install_default();
 let root=std::env::var_os("CG_QA_TEST_EVIDENCE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
 let dir=root.join(format!("scoped-peer-{}",std::process::id()));std::fs::create_dir(&dir).unwrap();
 let ctx=common::qa_trace_scope::TraceContext{native_session_id:"native_loopback".into(),claim_id:"claim_loopback".into(),handoff_binding_id:"handoff_loopback".into(),rental_session_id:Some("rental_loopback".into()),connection_nonce:"c".repeat(32),generation:1,authority_expires_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64+120000};
 let rental=dir.join("rental.json");std::fs::write(&rental,serde_json::json!({"PcId":"2","ClaimId":"claim_loopback","RentalSessionId":"rental_loopback"}).to_string()).unwrap();
 let capture=super::super::scoped::ScopedCapture::start(ctx.clone(),dir.clone(),rental).unwrap();
 let trace_path=dir.join(format!("capture-{}.jsonl","d".repeat(32)));
 let mut media=MediaEngine::default();media.register_default_codecs().unwrap();
 let mut registry=Registry::new();registry.add(Box::new(ObserverBuilder{tap:Capture::Scoped(capture.clone()),consumed:AtomicBool::new(false)}));registry=register_default_interceptors(registry,&mut media).unwrap();
 let settings=||{let mut s=SettingEngine::default();s.set_network_types(vec![NetworkType::Udp4]);s.set_include_loopback_candidate(true);s.set_ip_filter(Box::new(|ip|ip.is_loopback()));s};
 let a=Arc::new(APIBuilder::new().with_setting_engine(settings()).with_media_engine(media).with_interceptor_registry(registry).build().new_peer_connection(RTCConfiguration::default()).await.unwrap());
 let mut media=MediaEngine::default();media.register_default_codecs().unwrap();let registry=register_default_interceptors(Registry::new(),&mut media).unwrap();
 let b=Arc::new(APIBuilder::new().with_setting_engine(settings()).with_media_engine(media).with_interceptor_registry(registry).build().new_peer_connection(RTCConfiguration::default()).await.unwrap());
 let(tx,mut received)=tokio::sync::mpsc::channel(32);
 b.on_track(Box::new(move|track,receiver,_transceiver|{let tx=tx.clone();Box::pin(async move{tokio::spawn(async move{let mut buf=[0u8;1500];while receiver.read(&mut buf).await.is_ok(){}});tokio::spawn(async move{for _ in 0..24{match track.read_rtp().await{Ok((packet,_))=>{let _=tx.send(packet).await;},Err(_)=>break}}});})}));
 use crate::transport::webrtc::sender::{SequencedTrackLocalStaticRTP,TrackLike};
 let track=Arc::new(SequencedTrackLocalStaticRTP::from(TrackLocalStaticRTP::new(RTCRtpCodecCapability{mime_type:"video/H264".into(),clock_rate:90000,..Default::default()},"synthetic".into(),"local".into())));let sender=a.add_track(track.clone().track()).await.unwrap();
 let read_task=tokio::spawn(async move{let mut buf=[0;1500];while sender.read(&mut buf).await.is_ok(){}});
 let mut gather=a.gathering_complete_promise().await;let offer=a.create_offer(None).await.unwrap();a.set_local_description(offer).await.unwrap();tokio::time::timeout(std::time::Duration::from_secs(5),gather.recv()).await.unwrap();
 b.set_remote_description(a.local_description().await.unwrap()).await.unwrap();let mut gather=b.gathering_complete_promise().await;let answer=b.create_answer(None).await.unwrap();b.set_local_description(answer).await.unwrap();tokio::time::timeout(std::time::Duration::from_secs(5),gather.recv()).await.unwrap();a.set_remote_description(b.local_description().await.unwrap()).await.unwrap();
 let test=async{
  tokio::time::timeout(std::time::Duration::from_secs(5),async{while a.connection_state()!=RTCPeerConnectionState::Connected||b.connection_state()!=RTCPeerConnectionState::Connected{tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.expect("loopback ICE/DTLS");
  for i in 0..3u32{let mut p=packet();p.header.timestamp=1000+i*3000;for sample in crate::transport::webrtc::recovery_frame::attach(vec![p],0){track.write_with_extensions(sample,&[]).await.unwrap();}}
  for _ in 0..3{tokio::time::timeout(std::time::Duration::from_secs(2),received.recv()).await.unwrap().unwrap();}
  assert!(!trace_path.exists());
  let request=common::qa_trace_scope::TraceRequest{context:ctx,request_nonce:"d".repeat(32),expires_ms:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()as u64+5000,duration_ms:8000};std::fs::write(dir.join("request.json"),serde_json::to_vec(&request).unwrap()).unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(2),async{while capture.acquire().is_none(){tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
  let mut expected=Vec::new();for i in 0..4u32{for sample in crate::transport::webrtc::video::qa_packetize_frame(i){expected.push(sample.packet.clone());track.write_with_extensions(sample,&[]).await.unwrap();}}
  let mut actual=Vec::new();for _ in 0..20{actual.push(tokio::time::timeout(std::time::Duration::from_secs(2),received.recv()).await.unwrap().unwrap());}
  let original=actual[6].clone();
  b.write_rtcp(&[Box::new(TransportLayerNack{sender_ssrc:17,media_ssrc:original.header.ssrc,nacks:vec![NackPair{packet_id:original.header.sequence_number,lost_packets:0}]})]).await.unwrap();
  // SDK resend is verified in the sender trace below. A duplicate already received
  // by this peer need not be surfaced again by its SRTP replay protection.
  tokio::time::timeout(std::time::Duration::from_secs(6),async{while !capture.acquire().is_some_and(|t|t.has_rr_lsr()){tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.expect("SDK scoped RR with LSR deadline");
  (actual,expected)
 };
 let (actual,expected)=test.await;a.close().await.unwrap();b.close().await.unwrap();read_task.abort();let rows:Vec<recorder::Event>={let until=std::time::Instant::now()+std::time::Duration::from_secs(2);loop{let text=std::fs::read_to_string(&trace_path).unwrap();let lines:Vec<serde_json::Value>=text.lines().filter_map(|s|serde_json::from_str(s).ok()).collect();if let Some(footer)=lines.last().filter(|f|f["type"]=="footer"){assert_eq!(footer["observation_complete"],true);break lines.into_iter().filter(|x|x.get("data").is_some()).map(|x|serde_json::from_value(x).unwrap()).collect();}assert!(std::time::Instant::now()<until,"writer footer deadline");tokio::time::sleep(std::time::Duration::from_millis(20)).await;}};
 assert_eq!(actual.len(),20);for (p,want) in actual.iter().zip(expected.iter()){assert_eq!(p.payload,want.payload);assert_eq!(p.header.timestamp,want.header.timestamp);assert_eq!(p.header.marker,want.header.marker);assert_ne!(p.header.ssrc,0);assert_ne!(p.header.payload_type,0);assert!(rows.iter().any(|e|matches!(e.data,Data::Rtp{ssrc,seq,rtp,pt,ok:true,..}if ssrc==p.header.ssrc&&seq==p.header.sequence_number&&rtp==p.header.timestamp&&pt==p.header.payload_type)));}
 let maps:Vec<_>=rows.iter().filter(|e|matches!(e.data,Data::FramePacket{..})).collect();assert!(maps.len()==20||maps.len()==21);
 let mut grouped=std::collections::HashMap::new();for e in maps{if let Data::FramePacket{frame_id,index,count,reference,ssrc,seq,rtp,..}=e.data{assert_eq!(count,5);assert_eq!(reference,2);assert!(actual.iter().any(|p|p.header.ssrc==ssrc&&p.header.sequence_number==seq&&p.header.timestamp==rtp));grouped.entry(frame_id).or_insert_with(Vec::new).push(index);}}
 assert_eq!(grouped.len(),4);for indices in grouped.values_mut(){indices.sort();indices.dedup();assert_eq!(*indices,vec![0,1,2,3,4]);}
 assert_eq!(rows.iter().filter(|e|matches!(e.data,Data::Rtp{ok:true,..})).count(),21);
 let requested=&actual[6];assert!(rows.iter().any(|e|matches!(e.data,Data::Nack{ssrc,pid,..}if ssrc==requested.header.ssrc&&pid==requested.header.sequence_number)));assert_eq!(rows.iter().filter(|e|matches!(e.data,Data::Rtp{ssrc,seq,rtp,ok:true,..}if ssrc==requested.header.ssrc&&seq==requested.header.sequence_number&&rtp==requested.header.timestamp)).count(),2);
 let ssrc=actual[0].header.ssrc;assert!(rows.iter().any(|e|matches!(e.data,Data::Sr{ssrc:s,ok:true,..}if s==ssrc)));assert!(rows.iter().any(|e|matches!(e.data,Data::Rr{ssrc:s,lsr,..}if s==ssrc&&lsr!=0)));
 let matched:Vec<_>=rows.iter().filter_map(|rr|{if let Data::Rr{ssrc:s,lsr,dlsr,..}=rr.data {if s!=ssrc||lsr==0{return None;}let sr=rows.iter().find(|sr|matches!(sr.data,Data::Sr{ssrc:t,compact_ntp:n,ok:true,..}if t==s&&n==lsr)&&sr.completed_us<=rr.completed_us)?;let dwell=f64::from(dlsr)*1000.0/65536.0;Some(serde_json::json!({"lsr":lsr,"sr_start_us":sr.started_us,"sr_complete_us":sr.completed_us,"rr_read_us":rr.completed_us,"dlsr_ms":dwell,"sdk_call_rtt_lower_ms":(rr.completed_us-sr.completed_us)as f64/1000.0-dwell,"sdk_call_rtt_upper_ms":(rr.completed_us-sr.started_us)as f64/1000.0-dwell}))}else{None}}).collect();assert!(!matched.is_empty(),"actual matching SR/RR required");
 if let Ok(dir)=std::env::var("CG_QA_TEST_EVIDENCE_DIR"){
  let path=std::path::PathBuf::from(dir).join(format!("scoped-sdk-loopback-{}.json",std::process::id()));let file=std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();serde_json::to_writer_pretty(file,&serde_json::json!({"scope":"actual SDK PeerConnection UDP loopback / ICE DTLS SRTP, synthetic payload not decodable video; not physical cloud gaming","packets_received":actual.len(),"retransmitted_writes_observed":1,"duplicate_delivery_claimed":false,"mapped_frames":grouped.len(),"warmup_packets_excluded":3,"scope_test":"synthetic parent/rental fixtures through ScopedCapture; not actual rental authority","final_ssrc":ssrc,"synthetic_payload_preserved":true,"peer_a_closed":true,"peer_b_closed":true,"events":rows,"matched_sr_rr":matched,"trace_file":trace_path,"snapshot_source":"actual bounded JSONL writer with complete footer, replayed into event records","quality_accepted":false})).unwrap();
 }
}
