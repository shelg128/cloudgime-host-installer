use std::{
    collections::VecDeque,
    env,
    sync::{Arc, Mutex},
};

use cpal::{
    BufferSize, Device, FromSample, I24, Sample, SampleFormat, SampleRate, Stream, StreamConfig,
    SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use log::{info, warn};
use opus::{Channels, Decoder as OpusDecoder};

const OPUS_SAMPLE_RATE: u32 = 48_000;
const LOW_LATENCY_OUTPUT_BUFFER_FRAMES: u32 = 480;
const TARGET_QUEUE_MS: usize = 60;
const MAX_QUEUE_MS: usize = 140;

#[cfg(target_os = "windows")]
mod default_capture_endpoint {
    use std::ffi::c_void;

    use windows::{
        Win32::{
            Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
            Media::Audio::{
                DEVICE_STATE_ACTIVE, ERole, Endpoints::IAudioEndpointVolume, IMMDevice,
                IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications, eConsole,
                eMultimedia, eRender,
            },
            System::{
                Com::{
                    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                    CoTaskMemFree, CoUninitialize, STGM_READ,
                },
                Variant::VT_LPWSTR,
            },
        },
        core::{GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, PCWSTR},
    };

    const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
    const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    #[repr(transparent)]
    #[derive(Clone, PartialEq, Eq)]
    struct IPolicyConfig(IUnknown);

    unsafe impl Interface for IPolicyConfig {
        type Vtable = IPolicyConfigVtbl;
        const IID: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct IPolicyConfigVtbl {
        base__: IUnknown_Vtbl,
        GetMixFormat: unsafe extern "system" fn(*mut c_void, PCWSTR, *mut *mut c_void) -> HRESULT,
        GetDeviceFormat:
            unsafe extern "system" fn(*mut c_void, PCWSTR, i32, *mut *mut c_void) -> HRESULT,
        ResetDeviceFormat: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
        SetDeviceFormat:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *mut c_void, *mut c_void) -> HRESULT,
        GetProcessingPeriod:
            unsafe extern "system" fn(*mut c_void, PCWSTR, i32, *mut i64, *mut i64) -> HRESULT,
        SetProcessingPeriod: unsafe extern "system" fn(*mut c_void, PCWSTR, *mut i64) -> HRESULT,
        GetShareMode: unsafe extern "system" fn(*mut c_void, PCWSTR, *mut c_void) -> HRESULT,
        SetShareMode: unsafe extern "system" fn(*mut c_void, PCWSTR, *mut c_void) -> HRESULT,
        GetPropertyValue:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *const c_void, *mut c_void) -> HRESULT,
        SetPropertyValue:
            unsafe extern "system" fn(*mut c_void, PCWSTR, *const c_void, *const c_void) -> HRESULT,
        SetDefaultEndpoint: unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT,
        SetEndpointVisibility: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> HRESULT,
    }

    impl IPolicyConfig {
        fn set_default_endpoint(&self, device_id: &str, role: ERole) -> Result<(), String> {
            let device_id = wide_null(device_id);
            let result = unsafe {
                (Interface::vtable(self).SetDefaultEndpoint)(
                    Interface::as_raw(self),
                    PCWSTR(device_id.as_ptr()),
                    role,
                )
            };
            result
                .ok()
                .map_err(|err| format!("Windows rejected default microphone update: {err:?}"))
        }
    }

    struct ComScope {
        uninitialize: bool,
    }

    impl ComScope {
        fn new() -> Result<Self, String> {
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if result.is_ok() {
                return Ok(Self { uninitialize: true });
            }
            if result == RPC_E_CHANGED_MODE {
                return Ok(Self {
                    uninitialize: false,
                });
            }

            Err(format!("Windows audio COM init failed: {result:?}"))
        }
    }

    impl Drop for ComScope {
        fn drop(&mut self) {
            if self.uninitialize {
                unsafe { CoUninitialize() };
            }
        }
    }

    #[derive(Clone)]
    struct CaptureEndpoint {
        id: String,
        name: String,
    }

    pub struct HostDefaultMicrophoneGuard {
        target_id: String,
        previous_defaults: Vec<(ERole, String)>,
        pub capture_name: String,
    }

    impl Drop for HostDefaultMicrophoneGuard {
        fn drop(&mut self) {
            let Ok(_com) = ComScope::new() else {
                return;
            };
            let Ok(enumerator) = audio_enumerator() else {
                return;
            };
            let Ok(policy) = policy_config() else {
                return;
            };

            for (role, previous_id) in self.previous_defaults.iter().rev() {
                let current_id = default_capture_id(&enumerator, *role).ok();
                if current_id.as_deref() == Some(self.target_id.as_str()) {
                    let _ = policy.set_default_endpoint(previous_id, *role);
                }
            }
        }
    }

    pub fn promote_virtual_capture_for_session(
        output_device_name: &str,
        capture_hint: Option<&str>,
    ) -> Result<HostDefaultMicrophoneGuard, String> {
        let _com = ComScope::new()?;
        let enumerator = audio_enumerator()?;
        let target = find_capture_endpoint(&enumerator, output_device_name, capture_hint)?
            .ok_or_else(|| {
                format!(
                    "paired virtual microphone input was not found for '{}'",
                    output_device_name
                )
            })?;
        let policy = policy_config()?;
        let roles = [eConsole, eMultimedia, eCommunications];
        let previous_defaults = roles
            .iter()
            .filter_map(|role| {
                default_capture_id(&enumerator, *role)
                    .ok()
                    .map(|id| (*role, id))
            })
            .collect::<Vec<_>>();

        let mut applied_roles = Vec::new();
        for role in roles {
            if let Err(err) = policy.set_default_endpoint(&target.id, role) {
                for applied_role in applied_roles.iter().rev() {
                    if let Some((_, previous_id)) = previous_defaults
                        .iter()
                        .find(|(previous_role, _)| previous_role == applied_role)
                    {
                        let _ = policy.set_default_endpoint(previous_id, *applied_role);
                    }
                }
                return Err(err);
            }
            applied_roles.push(role);
        }

        Ok(HostDefaultMicrophoneGuard {
            target_id: target.id,
            previous_defaults,
            capture_name: target.name,
        })
    }

    pub fn prepare_virtual_render_for_session(output_device_name: &str) -> Result<(), String> {
        let _com = ComScope::new()?;
        let enumerator = audio_enumerator()?;
        let (device, name) =
            find_render_endpoint(&enumerator, output_device_name)?.ok_or_else(|| {
                format!(
                    "virtual microphone output endpoint was not found for '{output_device_name}'"
                )
            })?;
        let volume = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.map_err(
            |err| format!("Windows audio endpoint volume lookup failed for '{name}': {err:?}"),
        )?;

        let muted = unsafe { volume.GetMute() }.map_err(|err| {
            format!("Windows audio mute state lookup failed for '{name}': {err:?}")
        })?;
        if muted.as_bool() {
            unsafe { volume.SetMute(false, std::ptr::null()) }
                .map_err(|err| format!("Windows audio unmute failed for '{name}': {err:?}"))?;
        }

        let scalar = unsafe { volume.GetMasterVolumeLevelScalar() }.unwrap_or(1.0);
        if scalar < 0.99 {
            unsafe { volume.SetMasterVolumeLevelScalar(1.0, std::ptr::null()) }.map_err(|err| {
                format!("Windows audio volume restore failed for '{name}': {err:?}")
            })?;
        }

        Ok(())
    }

    fn audio_enumerator() -> Result<IMMDeviceEnumerator, String> {
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|err| format!("Windows audio endpoint enumerator failed: {err:?}"))
    }

    fn policy_config() -> Result<IPolicyConfig, String> {
        unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
            .map_err(|err| format!("Windows audio policy config failed: {err:?}"))
    }

    fn default_capture_id(enumerator: &IMMDeviceEnumerator, role: ERole) -> Result<String, String> {
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, role) }
            .map_err(|err| format!("Windows default capture endpoint lookup failed: {err:?}"))?;
        device_id(&device)
    }

    fn find_capture_endpoint(
        enumerator: &IMMDeviceEnumerator,
        output_device_name: &str,
        capture_hint: Option<&str>,
    ) -> Result<Option<CaptureEndpoint>, String> {
        let patterns =
            super::capture_match_patterns_for_output_device(output_device_name, capture_hint);
        if patterns.is_empty() {
            return Ok(None);
        }

        let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
            .map_err(|err| format!("Windows capture endpoint enumeration failed: {err:?}"))?;
        let count = unsafe { collection.GetCount() }
            .map_err(|err| format!("Windows capture endpoint count failed: {err:?}"))?;
        let mut endpoints = Vec::<CaptureEndpoint>::new();

        for index in 0..count {
            let Ok(device) = (unsafe { collection.Item(index) }) else {
                continue;
            };
            let Ok(id) = device_id(&device) else {
                continue;
            };
            let Some(name) = friendly_name(&device) else {
                continue;
            };
            endpoints.push(CaptureEndpoint { id, name });
        }

        for pattern in &patterns {
            if let Some(endpoint) = endpoints.iter().find(|endpoint| {
                super::normalized_contains(&endpoint.name, pattern)
                    || super::normalized_contains(pattern, &endpoint.name)
            }) {
                return Ok(Some(endpoint.clone()));
            }
        }

        Ok(None)
    }

    fn find_render_endpoint(
        enumerator: &IMMDeviceEnumerator,
        output_device_name: &str,
    ) -> Result<Option<(IMMDevice, String)>, String> {
        let collection = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
            .map_err(|err| format!("Windows render endpoint enumeration failed: {err:?}"))?;
        let count = unsafe { collection.GetCount() }
            .map_err(|err| format!("Windows render endpoint count failed: {err:?}"))?;

        for index in 0..count {
            let Ok(device) = (unsafe { collection.Item(index) }) else {
                continue;
            };
            let Some(name) = friendly_name(&device) else {
                continue;
            };
            if name.eq_ignore_ascii_case(output_device_name)
                || super::normalized_contains(&name, output_device_name)
                || super::normalized_contains(output_device_name, &name)
            {
                return Ok(Some((device, name)));
            }
        }

        Ok(None)
    }

    fn device_id(device: &IMMDevice) -> Result<String, String> {
        let id = unsafe { device.GetId() }
            .map_err(|err| format!("Windows audio endpoint id lookup failed: {err:?}"))?;
        let value = unsafe { id.to_string() }
            .map_err(|err| format!("Windows audio endpoint id decode failed: {err}"))?;
        unsafe { CoTaskMemFree(Some(id.0.cast())) };
        Ok(value)
    }

    fn friendly_name(device: &IMMDevice) -> Option<String> {
        let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok()?;
        let property = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.ok()?;
        let raw = property.as_raw();
        let variant = unsafe { raw.Anonymous.Anonymous };
        if variant.vt != VT_LPWSTR.0 {
            return None;
        }

        let value = unsafe { variant.Anonymous.pwszVal };
        if value.is_null() {
            return None;
        }

        Some(unsafe { raw_wide_null_to_string(value) })
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn raw_wide_null_to_string(value: *const u16) -> String {
        let mut len = 0usize;
        while unsafe { *value.add(len) } != 0 {
            len += 1;
        }
        let slice = unsafe { std::slice::from_raw_parts(value, len) };
        String::from_utf16_lossy(slice)
    }
}

#[cfg(not(target_os = "windows"))]
mod default_capture_endpoint {
    pub struct HostDefaultMicrophoneGuard {
        pub capture_name: String,
    }

    pub fn promote_virtual_capture_for_session(
        _output_device_name: &str,
        _capture_hint: Option<&str>,
    ) -> Result<HostDefaultMicrophoneGuard, String> {
        Err("automatic default microphone selection is only available on Windows".to_owned())
    }

    pub fn prepare_virtual_render_for_session(_output_device_name: &str) -> Result<(), String> {
        Ok(())
    }
}

fn preferred_output_patterns() -> &'static [&'static str] {
    &[
        "symo virtual audio output",
        "symo virtual audio",
        "virtual audio driver output",
        "virtual audio driver by mtt",
        "virtual audio driver",
        "cable input (vb-audio virtual cable)",
        "cable-a input",
        "cable-b input",
        "cable input",
        "virtual speakers",
        "vb-audio",
        "audiorelay",
    ]
}

fn capture_hint_for_output_device(name: &str) -> Option<&'static str> {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("symo virtual audio output") || normalized.contains("symo virtual audio")
    {
        return Some("SYMO Virtual Audio Input");
    }
    if normalized.contains("virtual audio driver output") {
        return Some("Virtual Audio Driver Input");
    }
    if normalized.contains("virtual audio driver by mtt")
        || normalized.contains("virtual audio driver")
    {
        return Some("Virtual Mic Driver by MTT");
    }
    if normalized.contains("cable-a input") {
        return Some("CABLE-A Output (VB-Audio Cable A)");
    }
    if normalized.contains("cable-b input") {
        return Some("CABLE-B Output (VB-Audio Cable B)");
    }
    if normalized.contains("cable input") || normalized.contains("vb-audio") {
        return Some("CABLE Output (VB-Audio Virtual Cable)");
    }
    if normalized.contains("virtual speakers") || normalized.contains("audiorelay") {
        return Some("Virtual Mic (Virtual Mic for AudioRelay)");
    }

    None
}

fn normalized_contains(haystack: &str, needle: &str) -> bool {
    haystack
        .trim()
        .to_ascii_lowercase()
        .contains(&needle.trim().to_ascii_lowercase())
}

fn capture_match_patterns_for_output_device(
    output_device_name: &str,
    capture_hint: Option<&str>,
) -> Vec<String> {
    let normalized_output = output_device_name.to_ascii_lowercase();
    let mut patterns = Vec::<String>::new();
    let mut push_pattern = |pattern: &str| {
        let normalized = pattern.to_ascii_lowercase();
        if !patterns.iter().any(|existing| existing == &normalized) {
            patterns.push(normalized);
        }
    };

    if let Some(capture_hint) = capture_hint {
        push_pattern(capture_hint);
    }

    if normalized_output.contains("symo virtual audio") {
        push_pattern("symo virtual audio input");
    }
    if normalized_output.contains("virtual audio driver by mtt")
        || normalized_output.contains("virtual audio driver")
    {
        push_pattern("virtual mic driver by mtt");
        push_pattern("virtual audio driver input");
    }
    if normalized_output.contains("cable-a input") {
        push_pattern("cable-a output");
    }
    if normalized_output.contains("cable-b input") {
        push_pattern("cable-b output");
    }
    if normalized_output.contains("cable input") || normalized_output.contains("vb-audio") {
        push_pattern("cable output");
    }
    if normalized_output.contains("virtual speakers") || normalized_output.contains("audiorelay") {
        push_pattern("virtual mic");
        push_pattern("virtual mic for audiorelay");
    }
    patterns
}

fn choose_output_device() -> Result<Device, String> {
    let host = cpal::default_host();
    let override_name = env::var("MOONLIGHT_UPLINK_AUDIO_DEVICE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let allow_default_output = env::var("MOONLIGHT_UPLINK_ALLOW_DEFAULT_OUTPUT")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !matches!(normalized.as_str(), "" | "0" | "false" | "off" | "no")
        })
        .unwrap_or(false);

    let mut fallback_default = host.default_output_device();
    let devices = host
        .output_devices()
        .map_err(|err| format!("failed to enumerate output devices: {err}"))?;

    let mut named_devices = Vec::<(String, Device)>::new();
    for device in devices {
        let Ok(name) = device.name() else {
            continue;
        };
        named_devices.push((name, device));
    }

    if let Some(override_name) = override_name {
        if let Some((_, device)) = named_devices
            .into_iter()
            .find(|(name, _)| name.to_ascii_lowercase().contains(&override_name))
        {
            return Ok(device);
        }

        return Err(format!(
            "requested output device '{override_name}' was not found"
        ));
    }

    for pattern in preferred_output_patterns() {
        if let Some((_, device)) = named_devices
            .iter()
            .find(|(name, _)| name.to_ascii_lowercase().contains(pattern))
        {
            return Ok(device.clone());
        }
    }

    if !allow_default_output {
        return Err(
            "no virtual microphone sink device was found (install/configure SYMO Virtual Audio Output/Input, Virtual Audio Driver by MTT, VB-CABLE, or SYMO Audio Driver (AudioRelay), or set MOONLIGHT_UPLINK_AUDIO_DEVICE)"
                .to_owned(),
        );
    }

    fallback_default
        .take()
        .ok_or_else(|| "no output device available for microphone uplink".to_owned())
}

fn choose_stream_config(device: &Device) -> Result<SupportedStreamConfig, String> {
    let supported_configs = device
        .supported_output_configs()
        .map_err(|err| format!("failed to enumerate output configs: {err}"))?;

    let mut matching_configs = Vec::<SupportedStreamConfigRange>::new();
    for config in supported_configs {
        if config.min_sample_rate() <= SampleRate(OPUS_SAMPLE_RATE)
            && config.max_sample_rate() >= SampleRate(OPUS_SAMPLE_RATE)
        {
            matching_configs.push(config);
        }
    }

    if let Some(best_config) = matching_configs
        .into_iter()
        .max_by_key(|config| output_sample_format_score(config.sample_format()))
    {
        return Ok(best_config.with_sample_rate(SampleRate(OPUS_SAMPLE_RATE)));
    }

    let default = device
        .default_output_config()
        .map_err(|err| format!("failed to read default output config: {err}"))?;

    if default.sample_rate().0 != OPUS_SAMPLE_RATE {
        return Err(format!(
            "output device '{}' does not expose a 48 kHz config",
            device.name().unwrap_or_else(|_| "unknown".to_owned())
        ));
    }

    Ok(default)
}

fn output_sample_format_score(sample_format: SampleFormat) -> u8 {
    match sample_format {
        SampleFormat::F32 => 100,
        SampleFormat::I16 => 95,
        SampleFormat::U16 => 90,
        SampleFormat::F64 => 85,
        SampleFormat::I32 => 80,
        SampleFormat::U32 => 75,
        SampleFormat::I24 => 70,
        SampleFormat::I8 => 65,
        SampleFormat::U8 => 60,
        SampleFormat::I64 => 55,
        SampleFormat::U64 => 50,
        _ => 0,
    }
}

fn pop_sample(queue: &Arc<Mutex<VecDeque<f32>>>) -> f32 {
    let Ok(mut guard) = queue.lock() else {
        return 0.0;
    };
    guard.pop_front().unwrap_or(0.0)
}

fn write_output_samples<T>(target: &mut [T], queue: &Arc<Mutex<VecDeque<f32>>>)
where
    T: Sample + FromSample<f32>,
{
    for sample in target.iter_mut() {
        let value = pop_sample(queue).clamp(-1.0, 1.0);
        *sample = T::from_sample(value);
    }
}

fn build_output_stream_for_format<T>(
    device: &Device,
    stream_config: &StreamConfig,
    queue_for_callback: Arc<Mutex<VecDeque<f32>>>,
    error_device_name: String,
) -> Result<Stream, String>
where
    T: cpal::SizedSample + FromSample<f32>,
{
    let mut low_latency_config = stream_config.clone();
    low_latency_config.buffer_size = BufferSize::Fixed(LOW_LATENCY_OUTPUT_BUFFER_FRAMES);
    match build_output_stream_for_format_with_config::<T>(
        device,
        &low_latency_config,
        queue_for_callback.clone(),
        error_device_name.clone(),
    ) {
        Ok(stream) => Ok(stream),
        Err(low_latency_error) => {
            warn!(
                "[WebRTC] Microphone uplink low-latency output buffer unavailable ({error_device_name}): {low_latency_error}; using driver default"
            );
            build_output_stream_for_format_with_config::<T>(
                device,
                stream_config,
                queue_for_callback,
                error_device_name,
            )
        }
    }
}

fn build_output_stream_for_format_with_config<T>(
    device: &Device,
    stream_config: &StreamConfig,
    queue_for_callback: Arc<Mutex<VecDeque<f32>>>,
    error_device_name: String,
) -> Result<Stream, String>
where
    T: cpal::SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            stream_config,
            move |data: &mut [T], _| write_output_samples(data, &queue_for_callback),
            move |err| {
                warn!("[WebRTC] Microphone uplink output stream error ({error_device_name}): {err}")
            },
            None,
        )
        .map_err(|err| format!("failed to build output stream: {err}"))
}

fn queue_len_for_ms(channels: usize, duration_ms: usize) -> usize {
    ((OPUS_SAMPLE_RATE as usize * channels.max(1) * duration_ms) / 1_000).max(channels.max(1))
}

pub struct HostMicrophoneLoopback {
    _stream: Stream,
    default_mic_guard: Option<default_capture_endpoint::HostDefaultMicrophoneGuard>,
    queue: Arc<Mutex<VecDeque<f32>>>,
    decoder: OpusDecoder,
    decoded_channels: usize,
    output_channels: usize,
    output_device_name: String,
    capture_hint: Option<&'static str>,
}

impl HostMicrophoneLoopback {
    pub fn new(preferred_channels: usize) -> Result<Self, String> {
        let device = choose_output_device()?;
        let output_device_name = device.name().unwrap_or_else(|_| "unknown".to_owned());
        let capture_hint = capture_hint_for_output_device(&output_device_name);
        if let Err(err) =
            default_capture_endpoint::prepare_virtual_render_for_session(&output_device_name)
        {
            warn!("[WebRTC] Could not prepare microphone uplink output endpoint: {err}");
        }
        let supported_config = choose_stream_config(&device)?;
        let output_channels = usize::from(supported_config.channels());
        let sample_format = supported_config.sample_format();
        let stream_config: StreamConfig = supported_config.into();

        let max_queue_len = queue_len_for_ms(output_channels, MAX_QUEUE_MS);
        let queue = Arc::new(Mutex::new(VecDeque::<f32>::with_capacity(max_queue_len)));
        let queue_for_callback = queue.clone();
        let queue_for_error = queue.clone();

        let error_device_name = output_device_name.clone();
        let stream = match sample_format {
            SampleFormat::F32 => build_output_stream_for_format::<f32>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::I8 => build_output_stream_for_format::<i8>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::I16 => build_output_stream_for_format::<i16>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::I24 => build_output_stream_for_format::<I24>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::I32 => build_output_stream_for_format::<i32>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::I64 => build_output_stream_for_format::<i64>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::U8 => build_output_stream_for_format::<u8>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::U16 => build_output_stream_for_format::<u16>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::U32 => build_output_stream_for_format::<u32>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::U64 => build_output_stream_for_format::<u64>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            SampleFormat::F64 => build_output_stream_for_format::<f64>(
                &device,
                &stream_config,
                queue_for_callback,
                error_device_name,
            )?,
            other => return Err(format!("unsupported output sample format: {other:?}")),
        };

        stream
            .play()
            .map_err(|err| format!("failed to start output stream: {err}"))?;

        let default_mic_guard = match default_capture_endpoint::promote_virtual_capture_for_session(
            &output_device_name,
            capture_hint,
        ) {
            Ok(guard) => {
                info!(
                    "[WebRTC] Host default microphone temporarily routed to '{}'",
                    guard.capture_name
                );
                Some(guard)
            }
            Err(err) => {
                warn!("[WebRTC] Could not auto-select host default microphone: {err}");
                None
            }
        };

        let decoded_channels = if preferred_channels >= 2 { 2 } else { 1 };
        let decoder = OpusDecoder::new(
            OPUS_SAMPLE_RATE,
            if decoded_channels >= 2 {
                Channels::Stereo
            } else {
                Channels::Mono
            },
        )
        .map_err(|err| format!("failed to create opus decoder: {err}"))?;

        info!(
            "[WebRTC] Remote microphone uplink routed to output device '{}' ({} ch @ {} Hz)",
            output_device_name, output_channels, OPUS_SAMPLE_RATE
        );
        if let Some(capture_hint) = capture_hint {
            info!(
                "[WebRTC] Use host capture endpoint '{}' in the game/app to receive browser mic uplink",
                capture_hint
            );
        } else {
            warn!(
                "[WebRTC] No capture endpoint hint is known for output device '{}'",
                output_device_name
            );
        }

        let _ = queue_for_error;

        Ok(Self {
            _stream: stream,
            default_mic_guard,
            queue,
            decoder,
            decoded_channels,
            output_channels,
            output_device_name,
            capture_hint,
        })
    }

    fn push_interleaved_i16(&self, pcm: &[i16]) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };

        let target_queue_len = queue_len_for_ms(self.output_channels, TARGET_QUEUE_MS);
        let max_queue_len = queue_len_for_ms(self.output_channels, MAX_QUEUE_MS);

        while queue.len() > target_queue_len {
            let _ = queue.pop_front();
        }

        if self.decoded_channels == 1 {
            for sample in pcm {
                let normalized = *sample as f32 / i16::MAX as f32;
                if self.output_channels <= 1 {
                    queue.push_back(normalized);
                } else {
                    for _ in 0..self.output_channels {
                        queue.push_back(normalized);
                    }
                }
            }
        } else {
            for frame in pcm.chunks_exact(self.decoded_channels) {
                let left = frame.first().copied().unwrap_or(0) as f32 / i16::MAX as f32;
                let right = frame
                    .get(1)
                    .copied()
                    .unwrap_or(frame.first().copied().unwrap_or(0))
                    as f32
                    / i16::MAX as f32;
                if self.output_channels <= 1 {
                    queue.push_back((left + right) * 0.5);
                } else {
                    queue.push_back(left);
                    queue.push_back(right);
                    for _ in 2..self.output_channels {
                        queue.push_back((left + right) * 0.5);
                    }
                }
            }
        }

        while queue.len() > max_queue_len {
            let _ = queue.pop_front();
        }
    }

    pub fn render_opus_payload(&mut self, payload: &[u8]) -> Result<(), String> {
        if payload.is_empty() {
            return Ok(());
        }

        let mut decode_buffer = vec![0_i16; 5_760 * self.decoded_channels.max(1)];
        let decoded_samples = match self.decoder.decode(payload, &mut decode_buffer, false) {
            Ok(samples) => samples,
            Err(err) if self.decoded_channels == 2 => {
                warn!(
                    "[WebRTC] Stereo opus decode failed on '{}', retrying mono: {err}",
                    self.output_device_name
                );
                self.decoder =
                    OpusDecoder::new(OPUS_SAMPLE_RATE, Channels::Mono).map_err(|create_err| {
                        format!("failed to re-create mono opus decoder: {create_err}")
                    })?;
                self.decoded_channels = 1;
                decode_buffer.resize(5_760, 0);
                self.decoder
                    .decode(payload, &mut decode_buffer, false)
                    .map_err(|retry_err| {
                        format!("failed to decode opus payload after mono fallback: {retry_err}")
                    })?
            }
            Err(err) => return Err(format!("failed to decode opus payload: {err}")),
        };

        let used_samples = decoded_samples.saturating_mul(self.decoded_channels);
        self.push_interleaved_i16(&decode_buffer[..used_samples]);
        Ok(())
    }

    pub fn capture_hint(&self) -> Option<&'static str> {
        self.capture_hint
    }

    pub fn default_capture_name(&self) -> Option<&str> {
        self.default_mic_guard
            .as_ref()
            .map(|guard| guard.capture_name.as_str())
    }
}
