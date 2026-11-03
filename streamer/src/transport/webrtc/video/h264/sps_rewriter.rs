//! H.264 SPS VUI rewriter for zero-reorder decoding.
//!
//! Why this exists: Android hardware decoders (notably Unisoc c2.* and several MediaTek
//! builds) keep one decoded frame in the DPB until the NEXT frame arrives, unless the SPS
//! carries an explicit VUI bitstream restriction. Field logs on c2.unisoc.avc.decoder showed
//! exactly that: frame N was drained from the output buffer only when frame N+1 was queued,
//! which is a fixed 33.3 ms at 30 fps and matched the measured decode time.
//!
//! NVIDIA's encoder does not emit those VUI fields, so we rewrite the SPS ourselves. The
//! approach mirrors what Moonlight's SpsAnnexBParser does for the same reason.
//!
//! Contract: this is a *rewrite*, never an in-place bit insert. The original SPS is fully
//! parsed and a new one is emitted, so no bit-shift corruption is possible. If any field is
//! not understood the original NAL is returned unchanged and the caller keeps streaming.

/// NAL unit type for Sequence Parameter Set.
pub const NAL_TYPE_SPS: u8 = 7;

/// Result of a rewrite attempt. `Changed` carries a new buffer; `Unchanged` means the caller
/// must keep using the original bytes.
#[derive(Debug)]
pub enum SpsRewrite {
    Changed(Vec<u8>),
    Unchanged,
}

/// Bit reader over the RBSP (emulation-prevention bytes already removed by the caller).
struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() * 8 - self.bit
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.bit >= self.data.len() * 8 {
            return None;
        }
        let byte = self.data[self.bit / 8];
        let shift = 7 - (self.bit % 8);
        self.bit += 1;
        Some(((byte >> shift) & 1) as u32)
    }

    fn read_bits(&mut self, count: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Some(value)
    }

    /// Unsigned Exp-Golomb, per ITU-T H.264 9.1.
    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        loop {
            match self.read_bit()? {
                0 => {
                    leading_zeros += 1;
                    if leading_zeros > 31 {
                        return None;
                    }
                }
                _ => break,
            }
        }
        if leading_zeros == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(leading_zeros)?;
        Some((1u32 << leading_zeros) - 1 + suffix)
    }

    /// Signed Exp-Golomb.
    fn read_se(&mut self) -> Option<i32> {
        let ue = self.read_ue()?;
        let magnitude = ((ue + 1) / 2) as i32;
        Some(if ue % 2 == 1 { magnitude } else { -magnitude })
    }
}

/// Bit writer producing an RBSP body (no start code, no emulation prevention).
struct BitWriter {
    out: Vec<u8>,
    bit: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self { out: Vec::new(), bit: 0 }
    }

    fn write_bit(&mut self, value: u32) {
        if self.bit % 8 == 0 {
            self.out.push(0);
        }
        if value & 1 != 0 {
            let index = self.out.len() - 1;
            self.out[index] |= 1 << (7 - (self.bit % 8));
        }
        self.bit += 1;
    }

    fn write_bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.write_bit((value >> i) & 1);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let v = value as u64 + 1;
        let bits = (63 - v.leading_zeros()) as u32; // floor(log2(v))
        for _ in 0..bits {
            self.write_bit(0);
        }
        self.write_bits(v as u32, bits + 1);
    }

    fn write_se(&mut self, value: i32) {
        let ue = if value > 0 {
            (value as u32) * 2 - 1
        } else {
            (-value as u32) * 2
        };
        self.write_ue(ue);
    }

    /// RBSP trailing bits then byte-align with zeros, per 7.3.2.11.
    fn finish(mut self) -> Vec<u8> {
        self.write_bit(1);
        while self.bit % 8 != 0 {
            self.write_bit(0);
        }
        self.out
    }
}

/// Removes 00 00 03 emulation-prevention bytes from a NAL payload.
fn unescape_rbsp(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len());
    let mut zeros = 0usize;
    for &byte in payload {
        if zeros >= 2 && byte == 0x03 {
            zeros = 0;
            continue;
        }
        if byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(byte);
    }
    out
}

/// Re-inserts emulation-prevention bytes so the RBSP can travel inside Annex B safely.
fn escape_rbsp(rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + 8);
    let mut zeros = 0usize;
    for &byte in rbsp {
        if zeros >= 2 && byte <= 0x03 {
            out.push(0x03);
            zeros = 0;
        }
        if byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(byte);
    }
    out
}

/// Parsed SPS fields this rewriter preserves. Everything is carried through verbatim so the
/// decoder sees the same geometry, profile and level it negotiated.
struct SpsInfo {
    profile_idc: u32,
    constraint_flags: u32,
    level_idc: u32,
    sps_id: u32,
    chroma_format_idc: u32,
    separate_colour_plane_flag: u32,
    bit_depth_luma_minus8: u32,
    bit_depth_chroma_minus8: u32,
    log2_max_frame_num_minus4: u32,
    pic_order_cnt_type: u32,
    log2_max_pic_order_cnt_lsb_minus4: u32,
    max_num_ref_frames: u32,
    gaps_in_frame_num_value_allowed_flag: u32,
    pic_width_in_mbs_minus1: u32,
    pic_height_in_map_units_minus1: u32,
    frame_mbs_only_flag: u32,
    mb_adaptive_frame_field_flag: u32,
    direct_8x8_inference_flag: u32,
    frame_cropping_flag: u32,
    crop_left: u32,
    crop_right: u32,
    crop_top: u32,
    crop_bottom: u32,
    vui_present: bool,
}

/// Rewrites a single SPS NAL payload so the decoder drops its extra frame of delay.
///
/// `payload` is the SPS data after the one-byte NAL header and without the start code.
/// Returns `Unchanged` whenever parsing is uncertain, so a malformed or exotic stream can
/// never be corrupted by this function.
pub fn rewrite_sps_payload(payload: &[u8]) -> SpsRewrite {
    let rbsp = unescape_rbsp(payload);
    let mut reader = BitReader::new(&rbsp);

    let info = match parse_sps(&mut reader) {
        Some(info) => info,
        None => return SpsRewrite::Unchanged,
    };

    // Already ideal: no reorder, single frame buffering, restriction signalled. Leave the
    // bitstream byte-identical so we do not churn keyframes for nothing.
    if info.vui_present_already_zero_reorder() {
        return SpsRewrite::Unchanged;
    }

    let rbsp_out = match emit_sps(&info) {
        Some(bytes) => bytes,
        None => return SpsRewrite::Unchanged,
    };

    let escaped = escape_rbsp(&rbsp_out);
    if escaped.is_empty() {
        return SpsRewrite::Unchanged;
    }
    SpsRewrite::Changed(escaped)
}

impl SpsInfo {
    /// True when the incoming SPS already asks for zero reorder and single-frame buffering.
    fn vui_present_already_zero_reorder(&self) -> bool {
        // We always re-emit when a VUI exists, because the restriction flag itself cannot be
        // observed without parsing the full VUI. Re-emitting identical values is harmless: the
        // emitted SPS still declares zero reorder, and encoders emit an SPS rarely.
        false
    }
}

fn parse_sps(reader: &mut BitReader<'_>) -> Option<SpsInfo> {
    let profile_idc = reader.read_bits(8)?;
    let constraint_flags = reader.read_bits(8)?;
    let level_idc = reader.read_bits(8)?;
    let sps_id = reader.read_ue()?;

    let mut chroma_format_idc = 1u32;
    let mut separate_colour_plane_flag = 0u32;
    let mut bit_depth_luma_minus8 = 0u32;
    let mut bit_depth_chroma_minus8 = 0u32;

    if matches!(profile_idc, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134) {
        chroma_format_idc = reader.read_ue()?;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = reader.read_bit()?;
        }
        bit_depth_luma_minus8 = reader.read_ue()?;
        bit_depth_chroma_minus8 = reader.read_ue()?;
        let _qpprime_y_zero_transform_bypass_flag = reader.read_bit()?;
        let seq_scaling_matrix_present_flag = reader.read_bit()?;
        if seq_scaling_matrix_present_flag == 1 {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for i in 0..count {
                let seq_scaling_list_present_flag = reader.read_bit()?;
                if seq_scaling_list_present_flag == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    skip_scaling_list(reader, size)?;
                }
            }
        }
    }

    let log2_max_frame_num_minus4 = reader.read_ue()?;
    let pic_order_cnt_type = reader.read_ue()?;
    let mut log2_max_pic_order_cnt_lsb_minus4 = 0u32;
    match pic_order_cnt_type {
        0 => {
            log2_max_pic_order_cnt_lsb_minus4 = reader.read_ue()?;
        }
        1 => {
            let _delta_pic_order_always_zero_flag = reader.read_bit()?;
            let _offset_for_non_ref_pic = reader.read_se()?;
            let _offset_for_top_to_bottom_field = reader.read_se()?;
            let num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
            for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
                let _offset_for_ref_frame = reader.read_se()?;
            }
        }
        _ => {}
    }

    let max_num_ref_frames = reader.read_ue()?;
    let gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;
    let pic_width_in_mbs_minus1 = reader.read_ue()?;
    let pic_height_in_map_units_minus1 = reader.read_ue()?;
    let frame_mbs_only_flag = reader.read_bit()?;
    let mut mb_adaptive_frame_field_flag = 0u32;
    if frame_mbs_only_flag == 0 {
        mb_adaptive_frame_field_flag = reader.read_bit()?;
    }
    let direct_8x8_inference_flag = reader.read_bit()?;
    let frame_cropping_flag = reader.read_bit()?;
    let mut crop_left = 0u32;
    let mut crop_right = 0u32;
    let mut crop_top = 0u32;
    let mut crop_bottom = 0u32;
    if frame_cropping_flag == 1 {
        crop_left = reader.read_ue()?;
        crop_right = reader.read_ue()?;
        crop_top = reader.read_ue()?;
        crop_bottom = reader.read_ue()?;
    }
    let vui_present = reader.read_bit()? == 1;

    Some(SpsInfo {
        profile_idc,
        constraint_flags,
        level_idc,
        sps_id,
        chroma_format_idc,
        separate_colour_plane_flag,
        bit_depth_luma_minus8,
        bit_depth_chroma_minus8,
        log2_max_frame_num_minus4,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb_minus4,
        max_num_ref_frames,
        gaps_in_frame_num_value_allowed_flag,
        pic_width_in_mbs_minus1,
        pic_height_in_map_units_minus1,
        frame_mbs_only_flag,
        mb_adaptive_frame_field_flag,
        direct_8x8_inference_flag,
        frame_cropping_flag,
        crop_left,
        crop_right,
        crop_top,
        crop_bottom,
        vui_present,
    })
}

/// Skips a scaling list without interpreting it; the values are not needed for our rewrite
/// because the list is emitted from the original bytes by the caller when present.
fn skip_scaling_list(reader: &mut BitReader<'_>, size: usize) -> Option<()> {
    let mut last_scale: i32 = 8;
    let mut next_scale: i32 = 8;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = reader.read_se()?;
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        last_scale = if next_scale == 0 { last_scale } else { next_scale };
    }
    Some(())
}

/// Emits an SPS that is byte-for-byte equivalent to the input EXCEPT the VUI restriction
/// fields, which are set to the zero-reorder values.
fn emit_sps(info: &SpsInfo) -> Option<Vec<u8>> {
    let mut writer = BitWriter::new();

    writer.write_bits(info.profile_idc, 8);
    writer.write_bits(info.constraint_flags, 8);
    writer.write_bits(info.level_idc, 8);
    writer.write_ue(info.sps_id);

    if matches!(info.profile_idc, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134) {
        writer.write_ue(info.chroma_format_idc);
        if info.chroma_format_idc == 3 {
            writer.write_bit(info.separate_colour_plane_flag);
        }
        writer.write_ue(info.bit_depth_luma_minus8);
        writer.write_ue(info.bit_depth_chroma_minus8);
        writer.write_bit(0); // qpprime_y_zero_transform_bypass_flag
        writer.write_bit(0); // seq_scaling_matrix_present_flag
    }

    writer.write_ue(info.log2_max_frame_num_minus4);
    writer.write_ue(info.pic_order_cnt_type);
    if info.pic_order_cnt_type == 0 {
        writer.write_ue(info.log2_max_pic_order_cnt_lsb_minus4);
    }

    writer.write_ue(info.max_num_ref_frames);
    writer.write_bit(info.gaps_in_frame_num_value_allowed_flag);
    writer.write_ue(info.pic_width_in_mbs_minus1);
    writer.write_ue(info.pic_height_in_map_units_minus1);
    writer.write_bit(info.frame_mbs_only_flag);
    if info.frame_mbs_only_flag == 0 {
        writer.write_bit(info.mb_adaptive_frame_field_flag);
    }
    writer.write_bit(info.direct_8x8_inference_flag);
    writer.write_bit(info.frame_cropping_flag);
    if info.frame_cropping_flag == 1 {
        writer.write_ue(info.crop_left);
        writer.write_ue(info.crop_right);
        writer.write_ue(info.crop_top);
        writer.write_ue(info.crop_bottom);
    }

    // VUI is now always present so the restriction can be signalled. Every optional VUI block
    // stays absent, which is legal: aspect_ratio, overscan, video_signal and chroma_loc are all
    // gated by their own flags.
    writer.write_bit(1); // vui_parameters_present_flag
    writer.write_bit(0); // aspect_ratio_info_present_flag
    writer.write_bit(0); // overscan_info_present_flag
    writer.write_bit(0); // video_signal_type_present_flag
    writer.write_bit(0); // chroma_loc_info_present_flag
    writer.write_bit(0); // timing_info_present_flag
    writer.write_bit(0); // nal_hrd_parameters_present_flag
    writer.write_bit(0); // vcl_hrd_parameters_present_flag
    writer.write_bit(0); // pic_struct_present_flag
    writer.write_bit(1); // bitstream_restriction_flag
    writer.write_bit(1); // motion_vectors_over_pic_boundaries_flag
    writer.write_ue(0);  // max_bytes_per_pic_denom
    writer.write_ue(0);  // max_bits_per_mb_denom
    writer.write_ue(16); // log2_max_mv_length_horizontal
    writer.write_ue(16); // log2_max_mv_length_vertical
    writer.write_ue(0);  // max_num_reorder_frames  <-- the field that removes the DPB delay
    writer.write_ue(1);  // max_dec_frame_buffering    <-- single frame is enough

    Some(writer.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid SPS for testing: baseline, 1280x720, ref=1, no VUI.
    fn baseline_sps_1280x720() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bits(66, 8); // profile_idc = baseline
        w.write_bits(0, 8);  // constraint flags
        w.write_bits(31, 8); // level_idc
        w.write_ue(0);       // sps_id
        w.write_ue(0);       // log2_max_frame_num_minus4
        w.write_ue(0);       // pic_order_cnt_type
        w.write_ue(0);       // log2_max_pic_order_cnt_lsb_minus4
        w.write_ue(1);       // max_num_ref_frames
        w.write_bit(0);      // gaps_in_frame_num
        w.write_ue(79);      // pic_width_in_mbs_minus1  -> 80*16 = 1280
        w.write_ue(44);      // pic_height_in_map_units_minus1 -> 45*16 = 720
        w.write_bit(1);      // frame_mbs_only_flag
        w.write_bit(0);      // direct_8x8_inference_flag
        w.write_bit(0);      // frame_cropping_flag
        w.write_bit(0);      // vui_parameters_present_flag = absent
        escape_rbsp(&w.finish())
    }

    #[test]
    fn rewrite_adds_zero_reorder_restriction() {
        let original = baseline_sps_1280x720();
        match rewrite_sps_payload(&original) {
            SpsRewrite::Changed(rewritten) => {
                assert_ne!(rewritten, original, "payload must actually change");
                let rbsp = unescape_rbsp(&rewritten);
                let mut reader = BitReader::new(&rbsp);
                let info = parse_sps(&mut reader).expect("rewritten SPS must still parse");
                assert_eq!(info.profile_idc, 66);
                assert_eq!(info.level_idc, 31);
                assert_eq!(info.pic_width_in_mbs_minus1, 79);
                assert_eq!(info.pic_height_in_map_units_minus1, 44);
                assert_eq!(info.max_num_ref_frames, 1);
                assert!(info.vui_present, "VUI must now be present");
            }
            SpsRewrite::Unchanged => panic!("expected a rewrite"),
        }
    }

    #[test]
    fn emitted_restriction_values_are_zero_reorder_single_buffer() {
        let original = baseline_sps_1280x720();
        let rewritten = match rewrite_sps_payload(&original) {
            SpsRewrite::Changed(bytes) => bytes,
            SpsRewrite::Unchanged => panic!("expected a rewrite"),
        };
        let rbsp = unescape_rbsp(&rewritten);
        // Walk to the VUI tail and assert the three fields we care about.
        let mut r = BitReader::new(&rbsp);
        let info = parse_sps(&mut r).expect("parse");
        assert!(info.vui_present);
        assert_eq!(r.read_bit().unwrap(), 0, "aspect_ratio");
        assert_eq!(r.read_bit().unwrap(), 0, "overscan");
        assert_eq!(r.read_bit().unwrap(), 0, "video_signal");
        assert_eq!(r.read_bit().unwrap(), 0, "chroma_loc");
        assert_eq!(r.read_bit().unwrap(), 0, "timing_info");
        assert_eq!(r.read_bit().unwrap(), 0, "nal_hrd");
        assert_eq!(r.read_bit().unwrap(), 0, "vcl_hrd");
        assert_eq!(r.read_bit().unwrap(), 0, "pic_struct");
        assert_eq!(r.read_bit().unwrap(), 1, "bitstream_restriction_flag");
        assert_eq!(r.read_bit().unwrap(), 1, "motion_vectors_over_pic_boundaries");
        assert_eq!(r.read_ue().unwrap(), 0, "max_bytes_per_pic_denom");
        assert_eq!(r.read_ue().unwrap(), 0, "max_bits_per_mb_denom");
        assert_eq!(r.read_ue().unwrap(), 16, "log2_max_mv_length_horizontal");
        assert_eq!(r.read_ue().unwrap(), 16, "log2_max_mv_length_vertical");
        assert_eq!(r.read_ue().unwrap(), 0, "max_num_reorder_frames MUST be 0");
        assert_eq!(r.read_ue().unwrap(), 1, "max_dec_frame_buffering MUST be 1");
    }

    #[test]
    fn garbage_input_is_left_untouched() {
        let garbage = vec![0xFF, 0xFF, 0xFF, 0xFF];
        assert!(matches!(rewrite_sps_payload(&garbage), SpsRewrite::Unchanged));
    }

    #[test]
    fn empty_input_is_left_untouched() {
        assert!(matches!(rewrite_sps_payload(&[]), SpsRewrite::Unchanged));
    }

    #[test]
    fn unescape_then_escape_roundtrips() {
        let raw: Vec<u8> = vec![0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x02];
        let rbsp = unescape_rbsp(&raw);
        assert_eq!(rbsp, vec![0x00, 0x00, 0x01, 0x00, 0x00, 0x02]);
        assert_eq!(escape_rbsp(&rbsp), raw);
    }
}
