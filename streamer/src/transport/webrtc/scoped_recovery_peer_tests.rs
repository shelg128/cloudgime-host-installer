use super::*;
use webrtc::{api::{APIBuilder,media_engine::MediaEngine,setting_engine::SettingEngine},ice::network_type::NetworkType,peer_connection::configuration::RTCConfiguration,rtp_transceiver::rtp_codec::RTCRtpCodecCapability,rtp::packet::Packet};
use tokio::{sync::mpsc,time::{sleep,timeout}};
async fn peer()->Arc<RTCPeerConnection>{
 let mut media=MediaEngine::default();media.register_codec(RTCRtpCodecParameters{capability:RTCRtpCodecCapability{mime_type:"video/H264".into(),clock_rate:90000,sdp_fmtp_line:"level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".into(),..Default::default()},payload_type:120,..Default::default()},RTPCodecType::Video).unwrap();
 let mut settings=SettingEngine::default();settings.set_network_types(vec![NetworkType::Udp4]);settings.set_include_loopback_candidate(true);settings.set_ip_filter(Box::new(|ip|ip.is_loopback()));
 let api_builder=APIBuilder::new().with_media_engine(media).with_setting_engine(settings);
 #[cfg(feature="cloudgime-total-egress-qa")]
 let api_builder={let mut registry=webrtc::interceptor::registry::Registry::new();registry.add(Box::new(crate::transport::webrtc::total_egress_qa::Builder));api_builder.with_interceptor_registry(registry)};
 let api=api_builder.build();Arc::new(api.new_peer_connection(RTCConfiguration::default()).await.unwrap())
}
async fn write_original<F:std::future::Future>(seq:u16,ts:u32,send:F)->F::Output{
 #[cfg(feature="cloudgime-total-egress-qa")]
 {crate::transport::webrtc::original_write_qa::within(seq,ts,send).await}
 #[cfg(not(feature="cloudgime-total-egress-qa"))]
 {let _=(seq,ts);send.await}
}
#[tokio::test]
async fn real_local_peer_negotiates_binding_and_scoped_ciphertext_copy(){
 let _=rustls::crypto::aws_lc_rs::default_provider().install_default();
 let a=peer().await;let b=peer().await;let (tx,mut rx)=mpsc::channel(2);
 b.on_track(Box::new(move|track,_,_|{let tx=tx.clone();Box::pin(async move{if let Ok((packet,_))=track.read_rtp().await{let _=tx.send(packet).await;}})}));
 let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()as u64;
 let recovery=Recovery::new(Arc::downgrade(&a),&DirectRecoveryGrant{authority_expires_at_unix_ms:now+30000,max_extra_kbps:2000,max_extra_packets_per_second:1200}).unwrap();
 let raw=Arc::new(TrackLocalStaticRTP::new(RTCRtpCodecCapability{mime_type:"video/H264".into(),clock_rate:90000,sdp_fmtp_line:"level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".into(),..Default::default()},"video".into(),"fixture".into()));
 let track=Arc::new(RecoveryTrack::new(raw.clone(),Some(recovery.clone())));a.add_track(track.clone()).await.unwrap();
 let mut gather=a.gathering_complete_promise().await;let offer=a.create_offer(None).await.unwrap();a.set_local_description(offer).await.unwrap();timeout(Duration::from_secs(5),gather.recv()).await.unwrap();b.set_remote_description(a.local_description().await.unwrap()).await.unwrap();
 let mut gather=b.gathering_complete_promise().await;let answer=b.create_answer(None).await.unwrap();b.set_local_description(answer).await.unwrap();timeout(Duration::from_secs(5),gather.recv()).await.unwrap();a.set_remote_description(b.local_description().await.unwrap()).await.unwrap();
 let outcome=timeout(Duration::from_secs(10),async{
  while a.connection_state()!=RTCPeerConnectionState::Connected||b.connection_state()!=RTCPeerConnectionState::Connected{sleep(Duration::from_millis(20)).await;}
  recovery.loss_window(100,10);sleep(Duration::from_millis(25)).await;
  let scope=track.permission(4).await.expect("real negotiated binding must grant");assert_eq!(scope.payload_type,120);assert_ne!(scope.ssrc,0);
  let ssrc=scope.ssrc;let packet=Packet{header:webrtc::rtp::header::Header{version:2,sequence_number:4,timestamp:90000,marker:true,..Default::default()},payload:bytes::Bytes::from_static(&[0x65,1,2,3,4]),..Default::default()};
  write_original(4,packet.header.timestamp,webrtc_util::conn::scoped_recovery::within(scope,raw.write_rtp_with_extensions(&packet,&[]))).await.unwrap();
  let received=rx.recv().await.expect("receiver packet");assert_eq!(received.header.sequence_number,4);assert_eq!(received.header.ssrc,ssrc);assert_eq!(received.header.payload_type,120);
  if cfg!(feature="cloudgime-recovery-share-qa"){
   assert_eq!(recovery.budget.snapshot().copies,0,"no startup credit for a single original");
   for seq in 5..=(4+100/super::RECOVERY_SHARE as u16-1) {let mut next=packet.clone();next.header.sequence_number=seq;let permission=track.permission(seq).await.unwrap();write_original(seq,next.header.timestamp,webrtc_util::conn::scoped_recovery::within(permission,raw.write_rtp_with_extensions(&next,&[]))).await.unwrap();}
  }
  #[cfg(feature="cloudgime-recovery-delay-qa")]
  {assert_eq!(recovery.budget.snapshot().copies,0);sleep(Duration::from_millis(4)).await;let mut next=packet.clone();next.header.sequence_number=40;let permission=track.permission(40).await.unwrap();webrtc_util::conn::scoped_recovery::within(permission,raw.write_rtp_with_extensions(&next,&[])).await.unwrap();}
  assert_eq!(recovery.budget.snapshot().copies,1,"context must survive encryption/mux to UDP");
 }).await;
 a.close().await.unwrap();b.close().await.unwrap();outcome.expect("local negotiation/ciphertext timed out");
 assert!(track.permission(5).await.is_none());
}
