//! H.264 SPS VUI rewriter for zero-reorder decoding.
//!
//! Why this exists: Android hardware decoders (notably Unisoc c2.* and several MediaTek
//! builds) keep one decoded frame in the DPB until the NEXT frame arrives, unless the SPS
//! carries an explicit VUI bitstream restriction. Field logs on c2.unisoc.avc.decoder showed
//! exactly that: frame N left the output buffer only when frame N+1 was queued, a fixed
//! 33.3 ms at 30 fps that matched the measured decode time of 42 ms.
//!
//! NVIDIA's encoder does not emit those VUI fields, so the SPS is rewritten here. The
//! approach mirrors Moonlight's SpsAnnexBParser for the same reason.
//!
//! Two rewrite paths exist:
//!   * splice — when the encoder already emitted a VUI (NVENC does, it carries timing_info).
//!     Every original bit up to `bitstream_restriction_flag` is copied verbatim, then only the
//!     restriction fields are appended. This preserves timing, HRD and colour description
//!     byte-exact, which a whole-SPS re-emit cannot do.
//!   * rebuild — only for a stream with no VUI at all.
//!
//! Safety: any uncertainty, any geometry change on re-parse, or a shorter payload than the
//! source makes the caller keep the original NAL. Corruption is never preferred over latency.

pub const NAL_TYPE_SPS: u8 = 7;

#[derive(Debug)]
pub enum SpsRewrite {
    Changed(Vec<u8>),
    Unchanged,
}

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
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
        if count > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.read_bit()?;
        }
        Some(value)
    }

    /// Unsigned Exp-Golomb, ITU-T H.264 9.1.
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
        let bits = (63 - v.leading_zeros()) as u32;
        for _ in 0..bits {
            self.write_bit(0);
        }
        self.write_bits(v as u32, bits + 1);
    }

    /// RBSP trailing bits then byte alignment, per H.264 7.3.2.11.
    fn finish(mut self) -> Vec<u8> {
        self.write_bit(1);
        while self.bit % 8 != 0 {
            self.write_bit(0);
        }
        self.out
    }
}

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

/// Rewrites an SPS payload (without start code and without the NAL header byte) so the
/// decoder drops its extra frame of delay.
pub fn rewrite_sps_payload(payload: &[u8]) -> SpsRewrite {
    let rbsp = unescape_rbsp(payload);
    let mut reader = BitReader::new(&rbsp);

    let info = match parse_sps(&mut reader) {
        Some(info) => info,
        None => return SpsRewrite::Unchanged,
    };

    if info.vui_present {
        // `reader` now sits right after vui_parameters_present_flag.
        return splice_vui_restriction(&rbsp, &info, reader.bit);
    }

    rebuild_with_vui(&rbsp, &info)
}

/// Copies original bits up to `bitstream_restriction_flag`, then appends the restriction.
/// Every VUI field the encoder emitted stays byte-exact.
fn splice_vui_restriction(rbsp: &[u8], info: &SpsInfo, vui_start_bit: usize) -> SpsRewrite {
    let mut reader = BitReader::new(rbsp);
    reader.bit = vui_start_bit;

    let restriction = match locate_vui_restriction(&mut reader) {
        Some(value) => value,
        None => return SpsRewrite::Unchanged,
    };
    if restriction.already_zero_reorder {
        return SpsRewrite::Unchanged;
    }

    let mut writer = BitWriter::new();
    for index in 0..restriction.flag_bit_index {
        let byte = rbsp[index / 8];
        let shift = 7 - (index % 8);
        writer.write_bit(((byte >> shift) & 1) as u32);
    }

    writer.write_bit(1); // bitstream_restriction_flag
    writer.write_bit(1); // motion_vectors_over_pic_boundaries_flag: no boundary crossing
    writer.write_ue(0);  // max_bytes_per_pic_denom
    writer.write_ue(0);  // max_bits_per_mb_denom
    writer.write_ue(16); // log2_max_mv_length_horizontal
    writer.write_ue(16); // log2_max_mv_length_vertical
    writer.write_ue(0);  // max_num_reorder_frames  <-- removes the DPB delay
    writer.write_ue(1);  // max_dec_frame_buffering

    let escaped = escape_rbsp(&writer.finish());
    if escaped.is_empty() {
        return SpsRewrite::Unchanged;
    }
    if !self_consistent(&escaped, info) {
        return SpsRewrite::Unchanged;
    }
    SpsRewrite::Changed(escaped)
}

/// Whole-SPS emission, used only when the source stream has no VUI at all.
fn rebuild_with_vui(rbsp: &[u8], info: &SpsInfo) -> SpsRewrite {
    let _ = rbsp;
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
    writer.write_ue(0);  // max_num_reorder_frames
    writer.write_ue(1);  // max_dec_frame_buffering

    let escaped = escape_rbsp(&writer.finish());
    if escaped.is_empty() {
        return SpsRewrite::Unchanged;
    }
    if !self_consistent(&escaped, info) {
        return SpsRewrite::Unchanged;
    }
    SpsRewrite::Changed(escaped)
}

/// Re-parses the candidate and refuses it unless the stream identity is unchanged and the
/// VUI restriction is present. This is the guard that caught an earlier whole-SPS emitter
/// dropping ten bytes of NVENC VUI data.
fn self_consistent(candidate: &[u8], info: &SpsInfo) -> bool {
    let rbsp = unescape_rbsp(candidate);
    let mut reader = BitReader::new(&rbsp);
    let Some(verify) = parse_sps(&mut reader) else {
        return false;
    };
    if verify.profile_idc != info.profile_idc
        || verify.level_idc != info.level_idc
        || verify.pic_width_in_mbs_minus1 != info.pic_width_in_mbs_minus1
        || verify.pic_height_in_map_units_minus1 != info.pic_height_in_map_units_minus1
        || verify.frame_mbs_only_flag != info.frame_mbs_only_flag
        || verify.max_num_ref_frames != info.max_num_ref_frames
        || verify.frame_cropping_flag != info.frame_cropping_flag
        || !verify.vui_present
    {
        return false;
    }
    let Some(restriction) = locate_vui_restriction(&mut reader) else {
        return false;
    };
    restriction.already_zero_reorder
}

struct VuiRestriction {
    flag_bit_index: usize,
    already_zero_reorder: bool,
}

/// Walks the VUI up to (not including) `bitstream_restriction_flag`.
fn locate_vui_restriction(reader: &mut BitReader<'_>) -> Option<VuiRestriction> {
    if reader.read_bit()? == 1 {
        let aspect_ratio_idc = reader.read_bits(8)?;
        if aspect_ratio_idc == 255 {
            reader.read_bits(16)?; // sar_width
            reader.read_bits(16)?; // sar_height
        }
    }
    if reader.read_bit()? == 1 {
        reader.read_bit()?; // overscan_appropriate_flag
    }
    if reader.read_bit()? == 1 {
        reader.read_bits(3)?; // video_format
        reader.read_bit()?;   // video_full_range_flag
        if reader.read_bit()? == 1 {
            reader.read_bits(8)?; // colour_primaries
            reader.read_bits(8)?; // transfer_characteristics
            reader.read_bits(8)?; // matrix_coefficients
        }
    }
    if reader.read_bit()? == 1 {
        reader.read_ue()?; // chroma_sample_loc_type_top_field
        reader.read_ue()?; // chroma_sample_loc_type_bottom_field
    }
    if reader.read_bit()? == 1 {
        reader.read_bits(32)?; // num_units_in_tick
        reader.read_bits(32)?; // time_scale
        reader.read_bit()?;    // fixed_frame_rate_flag
    }
    let nal_hrd = reader.read_bit()? == 1;
    if nal_hrd {
        skip_hrd_parameters(reader)?;
    }
    let vcl_hrd = reader.read_bit()? == 1;
    if vcl_hrd {
        skip_hrd_parameters(reader)?;
    }
    if nal_hrd || vcl_hrd {
        reader.read_bit()?; // low_delay_hrd_flag
    }
    reader.read_bit()?; // pic_struct_present_flag

    let flag_bit_index = reader.bit;
    if reader.read_bit()? == 1 {
        reader.read_bit()?;  // motion_vectors_over_pic_boundaries_flag
        reader.read_ue()?;    // max_bytes_per_pic_denom
        reader.read_ue()?;    // max_bits_per_mb_denom
        reader.read_ue()?;    // log2_max_mv_length_horizontal
        reader.read_ue()?;    // log2_max_mv_length_vertical
        let reorder = reader.read_ue()?;
        let _buffering = reader.read_ue()?;
        return Some(VuiRestriction { flag_bit_index, already_zero_reorder: reorder == 0 });
    }
    Some(VuiRestriction { flag_bit_index, already_zero_reorder: false })
}

fn skip_hrd_parameters(reader: &mut BitReader<'_>) -> Option<()> {
    let cpb_cnt_minus1 = reader.read_ue()?;
    if cpb_cnt_minus1 > 31 {
        return None;
    }
    reader.read_bits(4)?; // bit_rate_scale
    reader.read_bits(4)?; // cpb_size_scale
    for _ in 0..=cpb_cnt_minus1 {
        reader.read_ue()?; // bit_rate_value_minus1
        reader.read_ue()?; // cpb_size_value_minus1
        reader.read_bit()?; // cbr_flag
    }
    reader.read_bits(5)?; // initial_cpb_removal_delay_length_minus1
    reader.read_bits(5)?; // cpb_removal_delay_length_minus1
    reader.read_bits(5)?; // dpb_output_delay_length_minus1
    reader.read_bits(5)?; // time_offset_length
    Some(())
}

/// Parses up to and including vui_parameters_present_flag; the reader is left right after it.
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
        let _qpprime = reader.read_bit()?;
        let scaling_present = reader.read_bit()?;
        if scaling_present == 1 {
            // Scaling lists are not modelled; refuse rather than guess.
            return None;
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
            let _offset_non_ref = reader.read_se()?;
            let _offset_top_bottom = reader.read_se()?;
            let cycle = reader.read_ue()?;
            if cycle > 255 {
                return None;
            }
            for _ in 0..cycle {
                let _offset_ref = reader.read_se()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim SPS captured from the production host (NVENC, 720x1568, High profile 100).
    /// NAL header byte 0x67 is stripped; this is the payload only.
    const NVENC_SPS_720x1568: [u8; 24] = [
        0x64, 0x00, 0x20, 0xac, 0x2b, 0x40, 0x5a, 0x06,
        0x2d, 0x80, 0xb5, 0x01, 0x01, 0x01, 0xf0, 0x00,
        0x00, 0x3e, 0x80, 0x00, 0x0e, 0xa6, 0x08, 0x40,
    ];

    fn baseline_sps_no_vui() -> Vec<u8> {
        let mut w = BitWriter::new();
        w.write_bits(66, 8);
        w.write_bits(0, 8);
        w.write_bits(31, 8);
        w.write_ue(0);
        w.write_ue(0);
        w.write_ue(0);
        w.write_ue(0);
        w.write_ue(1);
        w.write_bit(0);
        w.write_ue(79);
        w.write_ue(44);
        w.write_bit(1);
        w.write_bit(0);
        w.write_bit(0);
        w.write_bit(0); // vui absent
        escape_rbsp(&w.finish())
    }

    #[test]
    fn parses_real_nvenc_sps_geometry() {
        let rbsp = unescape_rbsp(&NVENC_SPS_720x1568);
        let mut reader = BitReader::new(&rbsp);
        let info = parse_sps(&mut reader).expect("nvenc sps must parse");
        assert_eq!(info.profile_idc, 100, "High profile");
        assert_eq!(info.level_idc, 32);
        assert_eq!(info.pic_width_in_mbs_minus1, 44, "720 px wide");
        assert_eq!(info.pic_height_in_map_units_minus1, 97, "1568 px tall");
        assert_eq!(info.max_num_ref_frames, 1);
        assert!(info.vui_present, "NVENC emits a VUI");
    }

    #[test]
    fn real_nvenc_sps_is_rewritten_without_losing_bytes() {
        let original = NVENC_SPS_720x1568.to_vec();
        match rewrite_sps_payload(&original) {
            SpsRewrite::Changed(rewritten) => {
                // The splice keeps every original bit and only appends the restriction, so the
                // result cannot be shorter than the source.
                assert!(
                    rewritten.len() >= original.len(),
                    "rewrite must not shrink the SPS: {} -> {}",
                    original.len(),
                    rewritten.len()
                );
                let rbsp = unescape_rbsp(&rewritten);
                let mut reader = BitReader::new(&rbsp);
                let info = parse_sps(&mut reader).expect("rewritten sps must parse");
                assert_eq!(info.profile_idc, 100);
                assert_eq!(info.pic_width_in_mbs_minus1, 44);
                assert_eq!(info.pic_height_in_map_units_minus1, 97);
                assert_eq!(info.max_num_ref_frames, 1);
                assert!(info.vui_present);
                let restriction = locate_vui_restriction(&mut reader)
                    .expect("restriction must be locatable after rewrite");
                assert!(restriction.already_zero_reorder, "max_num_reorder_frames must be 0");
            }
            SpsRewrite::Unchanged => panic!("real NVENC SPS must be rewritten"),
        }
    }

    #[test]
    fn rebuilt_sps_for_stream_without_vui() {
        let original = baseline_sps_no_vui();
        match rewrite_sps_payload(&original) {
            SpsRewrite::Changed(rewritten) => {
                let rbsp = unescape_rbsp(&rewritten);
                let mut reader = BitReader::new(&rbsp);
                let info = parse_sps(&mut reader).expect("rebuilt sps must parse");
                assert_eq!(info.profile_idc, 66);
                assert_eq!(info.pic_width_in_mbs_minus1, 79);
                assert_eq!(info.pic_height_in_map_units_minus1, 44);
                assert!(info.vui_present);
                let restriction = locate_vui_restriction(&mut reader).expect("restriction");
                assert!(restriction.already_zero_reorder);
            }
            SpsRewrite::Unchanged => panic!("a VUI-less SPS must be rebuilt"),
        }
    }

    #[test]
    fn already_zero_reorder_is_left_untouched() {
        let original = NVENC_SPS_720x1568.to_vec();
        let once = match rewrite_sps_payload(&original) {
            SpsRewrite::Changed(bytes) => bytes,
            SpsRewrite::Unchanged => panic!("first rewrite must change"),
        };
        // Second pass must be a no-op: the stream is already correct.
        assert!(matches!(rewrite_sps_payload(&once), SpsRewrite::Unchanged));
    }

    #[test]
    fn garbage_and_empty_are_untouched() {
        assert!(matches!(rewrite_sps_payload(&[0xFF, 0xFF, 0xFF, 0xFF]), SpsRewrite::Unchanged));
        assert!(matches!(rewrite_sps_payload(&[]), SpsRewrite::Unchanged));
    }

    #[test]
    fn unescape_escape_roundtrip() {
        let raw: Vec<u8> = vec![0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x02];
        let rbsp = unescape_rbsp(&raw);
        assert_eq!(rbsp, vec![0x00, 0x00, 0x01, 0x00, 0x00, 0x02]);
        assert_eq!(escape_rbsp(&rbsp), raw);
    }
}
