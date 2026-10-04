//! Host-created general control channel is not a request for legacy audio bytes.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum ChannelOrigin { Host, Remote }
pub(super) fn accepts_legacy_audio(origin:ChannelOrigin,label:&str)->bool {
    origin==ChannelOrigin::Remote && label=="general"
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn host_general_never_opts_client_into_audio_copy(){assert!(!accepts_legacy_audio(ChannelOrigin::Host,"general"));}
    #[test] fn explicit_remote_legacy_general_still_works(){assert!(accepts_legacy_audio(ChannelOrigin::Remote,"general"));}
    #[test] fn unrelated_remote_channels_do_not_enable_audio(){for s in ["stats","rtt","controllers","General",""]{assert!(!accepts_legacy_audio(ChannelOrigin::Remote,s));}}
}
