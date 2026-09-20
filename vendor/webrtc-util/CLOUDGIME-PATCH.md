# CloudGime local patch: exclusive Windows UDP binding

Source: crates.io `webrtc-util` 0.17.1, upstream git e9fe1bc4207a0c6b8d28d5583a118419be60f3c2, path `util`.
Original registry checksum: b65c1e0143a43d40f69e1d8c2ffc8734e379b49c06d45892ea4104c388bf9ead.
The MIT and Apache-2.0 license files are preserved.

Changes:
- `src/vnet/net.rs`: real Windows UDP bind/dial delegates to `windows_udp.rs`; non-Windows and virtual networks unchanged.
- `src/vnet/windows_udp.rs`: sets SO_EXCLUSIVEADDRUSE before bind, converts to a nonblocking Tokio socket. No SO_REUSEADDR fallback, no change to authentication, port limits, ICE role, consent or revoke.
- `Cargo.toml`: Windows socket2 0.5.10 and existing winapi winsock2 feature.

The published crate omits upstream *_test.rs files. Production regression coverage is in `streamer/src/transport/webrtc/windows_udp_ownership_tests.rs` of the CloudGime repository. Tests use the public ICE `Net` bind path, prove wildcard/interface overlap is refused in both orders, check the original socket still receives data, and verify rebinding after its owner is dropped.

This fixes a reproduced socket ownership defect. Whether it resolves the observed two-viewer streaming failure still requires live soak testing; the unit tests alone are not a media acceptance result.
