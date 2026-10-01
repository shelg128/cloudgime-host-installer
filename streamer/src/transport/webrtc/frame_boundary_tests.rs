use super::*;
fn nal(b:&[u8])->BytesMut { BytesMut::from(b) }
fn markers(p:&[Packet])->usize{p.iter().filter(|x|x.header.marker).count()}
#[test]
fn ignored_trailing_h264_delimiter_does_not_remove_end_marker(){
 let mut p=H264Payloader::default();let mut n=vec![nal(&[0x61,1,2]),nal(&[0x09,0xf0])];
 let packets=packetize_access_unit(&mut n,&mut p,123).unwrap();
 assert_eq!(packets.len(),1);assert_eq!(markers(&packets),1);assert!(packets.last().unwrap().header.marker);
}
#[test]
fn buffered_trailing_parameter_set_does_not_remove_end_marker(){
 let mut p=H264Payloader::default();let mut n=vec![nal(&[0x61,1,2]),nal(&[0x67,0x42,0,0x1f])];
 let packets=packetize_access_unit(&mut n,&mut p,123).unwrap();assert_eq!(markers(&packets),1);
}
#[test]
fn nal_error_rejects_whole_access_unit_not_partial_picture(){
 let mut p=H264Payloader::default();let mut n=vec![nal(&[0x61,1,2]),BytesMut::new(),nal(&[0x61,3,4])];
 assert!(packetize_access_unit(&mut n,&mut p,123).is_err());assert!(n.is_empty());
}
#[test]
fn multi_slice_fragmented_picture_has_one_final_marker(){
 let mut p=H264Payloader::default();let mut large=vec![0x55;4000];large[0]=0x65;
 let mut n=vec![nal(&[0x67,0x42,0,0x1f]),nal(&[0x68,0xce,6]),nal(&large),nal(&[0x65,2,3])];
 let packets=packetize_access_unit(&mut n,&mut p,u32::MAX).unwrap();assert!(packets.len()>4);assert_eq!(markers(&packets),1);assert!(packets.last().unwrap().header.marker);
 assert!(packets.iter().all(|p|p.header.timestamp==u32::MAX&&p.payload.len()+12<=RTP_OUTBOUND_MTU));
}
#[test]
fn metadata_only_access_unit_is_not_a_video_frame(){
 let mut p=H264Payloader::default();let mut n=vec![nal(&[0x09,0xf0])];assert!(packetize_access_unit(&mut n,&mut p,123).unwrap().is_empty());
}
