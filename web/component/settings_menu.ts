import { ControllerConfig } from "../stream/gamepad.js";
import { MouseScrollMode } from "../stream/input.js";
import { PageStyle } from "../styles/index.js";
import { Component, ComponentEvent } from "./index.js";
import { InputComponent, SelectComponent, setHelpModeActive, registerHelpCheckbox } from "./input.js";
import { SidebarEdge } from "./sidebar/index.js";

export type UiLanguage = "en" | "id"

export type Settings = {
    sidebarEdge: SidebarEdge,
    uiLanguage: UiLanguage
    bitrate: number
    adaptiveBitrate: boolean
    packetSize: number
    videoFrameQueueSize: number
    videoSize: "720p" | "1080p" | "1440p" | "4k" | "native" | "custom"
    videoSizeCustom: {
        width: number
        height: number
    },
    fps: number
    videoCodec: StreamCodec,
    forceVideoElementRenderer: boolean
    canvasRenderer: boolean
    canvasVsync: boolean
    playAudioLocal: boolean
    audioSampleQueueSize: number
    mouseScrollMode: MouseScrollMode
    controllerConfig: ControllerConfig
    dataTransport: TransportType
    toggleFullscreenWithKeybind: boolean
    pageStyle: PageStyle
    hdr: boolean
    useSelectElementPolyfill: boolean
}

export type StreamCodec = "h264" | "auto" | "h265" | "av1"
export type TransportType = "auto" | "webrtc" | "websocket"
export const WEBSOCKET_RELAY_ENABLED = false

import DEFAULT_SETTINGS from "../default_settings.js"

function normalizeTransportType(transport: TransportType | null | undefined): TransportType {
    if (transport == "auto") {
        return "webrtc"
    }
    if (!WEBSOCKET_RELAY_ENABLED && transport == "websocket") {
        return "webrtc"
    }

    return transport ?? "webrtc"
}

export function defaultSettings(): Settings {
    // We are deep cloning this
    let settings: Settings
    if ("structuredClone" in window) {
        settings = structuredClone(DEFAULT_SETTINGS)
    } else {
        settings = JSON.parse(JSON.stringify(DEFAULT_SETTINGS))
    }

    settings.dataTransport = normalizeTransportType(settings.dataTransport)
    return settings
}

export function getLocalStreamSettings(): Settings | null {
    let settings = null
    let migratedLegacyDeviceMatchDefault = false
    try {
        const settingsLoadedJson = localStorage.getItem("mlSettings")
        if (settingsLoadedJson == null) {
            return null
        }

        const settingsLoaded = JSON.parse(settingsLoadedJson)

        settings = defaultSettings()
        Object.assign(settings, settingsLoaded)
    } catch (e) {
        localStorage.removeItem("mlSettings")
    }

    // Migration
    if (settings?.pageStyle == "old") {
        settings.pageStyle = "moonlight"
    }
    if (settings) {
        if (
            settings.videoSize == "custom"
            && settings.videoSizeCustom.width == 1920
            && settings.videoSizeCustom.height == 1080
        ) {
            settings.videoSize = "native"
            migratedLegacyDeviceMatchDefault = true
        }
        if (
            settings.packetSize == 1400
            && settings.videoFrameQueueSize == 4
            && settings.audioSampleQueueSize == 24
        ) {
            settings.packetSize = 1200
            settings.videoFrameQueueSize = 2
            settings.audioSampleQueueSize = 12
            if (settings.bitrate == 10000) {
                settings.bitrate = 8000
            }
            setLocalStreamSettings(settings)
        }
        if (
            settings.packetSize == 2048
            && settings.videoFrameQueueSize == 3
            && settings.audioSampleQueueSize == 20
        ) {
            settings.packetSize = 1200
            settings.videoFrameQueueSize = 2
            settings.audioSampleQueueSize = 12
            if (settings.bitrate == 10000) {
                settings.bitrate = 8000
            }
            setLocalStreamSettings(settings)
        }
        if (
            settings.packetSize == 1200
            && settings.videoFrameQueueSize == 3
            && settings.audioSampleQueueSize == 20
        ) {
            settings.videoFrameQueueSize = 2
            settings.audioSampleQueueSize = 12
            if (settings.bitrate == 10000) {
                settings.bitrate = 8000
            }
            setLocalStreamSettings(settings)
        }
        if (
            settings.packetSize == 1200
            && settings.videoFrameQueueSize == 3
            && settings.audioSampleQueueSize == 16
        ) {
            settings.packetSize = 1000
            settings.videoFrameQueueSize = 2
            settings.audioSampleQueueSize = 10
            if (settings.bitrate == 10000) {
                settings.bitrate = 8000
            }
            setLocalStreamSettings(settings)
        }
        if (
            settings.packetSize == 1000
            && settings.videoFrameQueueSize == 2
            && settings.audioSampleQueueSize == 12
            && settings.bitrate == 10000
        ) {
            settings.bitrate = 8000
            setLocalStreamSettings(settings)
        }
        settings.dataTransport = normalizeTransportType(settings.dataTransport)
        if (migratedLegacyDeviceMatchDefault) {
            setLocalStreamSettings(settings)
        }
    }

    return settings
}
export function setLocalStreamSettings(settings?: Settings) {
    localStorage.setItem("mlSettings", JSON.stringify(settings))
}

export type StreamSettingsChangeListener = (event: ComponentEvent<StreamSettingsComponent>) => void

export class StreamSettingsComponent implements Component {

    private divElement: HTMLDivElement = document.createElement("div")

    private sidebarHeader: HTMLHeadingElement = document.createElement("h2")
    private sidebarEdge: SelectComponent

    private streamHeader: HTMLHeadingElement = document.createElement("h2")
    private bitrate: InputComponent
    private adaptiveBitrate: InputComponent
    private packetSize: InputComponent
    private fps: InputComponent
    private videoCodec: SelectComponent
    private forceVideoElementRenderer: InputComponent
    private canvasRenderer: InputComponent
    private canvasVsync: InputComponent
    private hdr: InputComponent

    private videoSize: SelectComponent
    private videoSizeWidth: InputComponent
    private videoSizeHeight: InputComponent

    private videoSampleQueueSize: InputComponent

    private audioHeader: HTMLHeadingElement = document.createElement("h2")
    private playAudioLocal: InputComponent
    private audioSampleQueueSize: InputComponent

    private mouseHeader: HTMLHeadingElement = document.createElement("h2")
    private mouseScrollMode: SelectComponent

    private controllerHeader: HTMLHeadingElement = document.createElement("h2")
    private controllerInvertAB: InputComponent
    private controllerInvertXY: InputComponent
    private controllerSendIntervalOverride: InputComponent

    private otherHeader: HTMLHeadingElement = document.createElement("h2")
    private uiLanguage: SelectComponent
    private dataTransport: SelectComponent
    private toggleFullscreenWithKeybind: InputComponent

    private pageStyle: SelectComponent

    private useSelectElementPolyfill: InputComponent

    private currentSettings: Settings
    private translate(en: string, id: string): string {
        return this.currentSettings.uiLanguage === "id" ? id : en
    }

    constructor(settings?: Settings) {
        const defaultSettings_ = defaultSettings()
        this.currentSettings = settings || defaultSettings_

        // Root div
        this.divElement.classList.add("settings")

        // Help Mode toggle
        const helpContainer = document.createElement("div")
        helpContainer.classList.add("help-toggle-container")

        const helpLabel = document.createElement("label")
        helpLabel.classList.add("help-toggle-label")
        helpLabel.innerText = this.translate("💡 Help Mode", "💡 Mode Panduan")

        const helpCheckbox = document.createElement("input")
        helpCheckbox.type = "checkbox"
        const savedHelpState = localStorage.getItem("helpModeActive") === "true"
        helpCheckbox.checked = savedHelpState
        setHelpModeActive(savedHelpState);
        registerHelpCheckbox(helpCheckbox);

        helpCheckbox.addEventListener("change", () => {
            const active = helpCheckbox.checked;
            localStorage.setItem("helpModeActive", String(active));
            setHelpModeActive(active);
        });

        helpLabel.appendChild(helpCheckbox)
        helpContainer.appendChild(helpLabel)
        this.divElement.appendChild(helpContainer)

        // Sidebar
        this.sidebarHeader.innerText = "Sidebar"
        this.divElement.appendChild(this.sidebarHeader)

        this.sidebarEdge = new SelectComponent("sidebarEdge", [
            { value: "left", name: "Left" },
            { value: "right", name: "Right" },
            { value: "up", name: "Up" },
            { value: "down", name: "Down" },
        ], {
            displayName: this.translate("Sidebar Position", "Posisi Menu Samping"),
            preSelectedOption: settings?.sidebarEdge ?? defaultSettings_.sidebarEdge,
        })
        this.sidebarEdge.setHelpText(
            "Which edge of the screen the sidebar toggle is located on (left, right, top, or bottom).",
            "Sisi layar tempat tombol menu samping berada (kiri, kanan, atas, atau bawah)."
        )
        this.sidebarEdge.addChangeListener(this.onSettingsChange.bind(this))
        this.sidebarEdge.mount(this.divElement)

        // Video
        this.streamHeader.innerText = "Video"
        this.divElement.appendChild(this.streamHeader)

        // Bitrate
        this.bitrate = new InputComponent("bitrate", "number", this.translate("Image Quality / Bandwidth Limit (Bitrate)", "Kualitas Gambar / Batas Kuota (Bitrate)"), {
            defaultValue: defaultSettings_.bitrate.toString(),
            value: settings?.bitrate?.toString(),
            step: "100",
            numberSlider: {
                range_min: 1000,
                range_max: 30000,
            }
        })
        this.bitrate.setHelpText(
            "Adjusts image clarity. Higher values make the stream sharper but require a faster and more stable internet connection.",
            "Mengatur ketajaman gambar. Semakin tinggi nilainya, gambar semakin jernih tetapi membutuhkan koneksi internet yang lebih kencang."
        )
        this.bitrate.addChangeListener(this.onSettingsChange.bind(this))
        this.bitrate.mount(this.divElement)

        // Adaptive Quality
        this.adaptiveBitrate = new InputComponent("adaptiveBitrate", "checkbox", this.translate("Auto-Adjust Stream Quality", "Penyesuaian Kualitas Otomatis"), {
            checked: settings?.adaptiveBitrate ?? defaultSettings_.adaptiveBitrate
        })
        this.adaptiveBitrate.setHelpText(
            "Automatically lowers stream quality dynamically when the internet slows down to prevent stuttering or freezes.",
            "Otomatis menurunkan kualitas visual secara dinamis saat internet melambat agar aliran tetap lancar tanpa macet."
        )
        this.adaptiveBitrate.addChangeListener(this.onSettingsChange.bind(this))
        this.adaptiveBitrate.mount(this.divElement)

        // Packet Size
        this.packetSize = new InputComponent("packetSize", "number", this.translate("Network Packet Size (Stability)", "Ukuran Paket Jaringan (Stabilitas)"), {
            defaultValue: defaultSettings_.packetSize.toString(),
            value: settings?.packetSize?.toString(),
            step: "100"
        })
        this.packetSize.setHelpText(
            "Use smaller sizes (1000-1200) for unstable connections like Wi-Fi/Mobile. Large sizes (1400+) are suitable for stable cable LAN/Fiber.",
            "Gunakan ukuran kecil (1000-1200) untuk Wi-Fi/Seluler agar koneksi lebih stabil. Ukuran besar (1400+) cocok untuk kabel LAN/Fiber."
        )
        this.packetSize.addChangeListener(this.onSettingsChange.bind(this))
        this.packetSize.mount(this.divElement)

        // Fps
        this.fps = new InputComponent("fps", "number", this.translate("Frame Rate (FPS)", "Kecepatan Gambar (FPS)"), {
            defaultValue: defaultSettings_.fps.toString(),
            value: settings?.fps?.toString(),
            step: "100"
        })
        this.fps.setHelpText(
            "Higher frame rates (e.g., 60 FPS) provide smoother gameplay, while lower frame rates (30 FPS) save bandwidth and reduce lag.",
            "Frame rate tinggi (60 FPS) memberikan gerakan yang lebih mulus, sedangkan frame rate rendah (30 FPS) menghemat kuota dan mengurangi lag."
        )
        this.fps.addChangeListener(this.onSettingsChange.bind(this))
        this.fps.mount(this.divElement)

        // Video Size
        this.videoSize = new SelectComponent("videoSize",
            [
                { value: "720p", name: "720p" },
                { value: "1080p", name: "1080p" },
                { value: "1440p", name: "1440p" },
                { value: "4k", name: "4k" },
                { value: "native", name: this.translate("device match", "sesuai layar perangkat") },
                { value: "custom", name: this.translate("custom", "kustom") }
            ],
            {
                displayName: this.translate("Video Resolution", "Resolusi Video"),
                preSelectedOption: settings?.videoSize || defaultSettings_.videoSize
            }
        )
        this.videoSize.setHelpText(
            "Controls the resolution of the video stream. Matching device resolution is recommended.",
            "Mengatur ketajaman piksel video. Direkomendasikan menyamakan dengan resolusi layar asli perangkatmu."
        )
        this.videoSize.addChangeListener(this.onSettingsChange.bind(this))
        this.videoSize.mount(this.divElement)

        this.videoSizeWidth = new InputComponent("videoSizeWidth", "number", this.translate("Video Width", "Lebar Video"), {
            defaultValue: defaultSettings_.videoSizeCustom.width.toString(),
            value: settings?.videoSizeCustom.width.toString()
        })
        this.videoSizeWidth.setHelpText(
            "Manual width in pixels for custom resolution.",
            "Lebar piksel manual untuk resolusi kustom."
        )
        this.videoSizeWidth.addChangeListener(this.onSettingsChange.bind(this))
        this.videoSizeWidth.mount(this.divElement)

        this.videoSizeHeight = new InputComponent("videoSizeHeight", "number", this.translate("Video Height", "Tinggi Video"), {
            defaultValue: defaultSettings_.videoSizeCustom.height.toString(),
            value: settings?.videoSizeCustom.height.toString()
        })
        this.videoSizeHeight.setHelpText(
            "Manual height in pixels for custom resolution.",
            "Tinggi piksel manual untuk resolusi kustom."
        )
        this.videoSizeHeight.addChangeListener(this.onSettingsChange.bind(this))
        this.videoSizeHeight.mount(this.divElement)

        // Video Sample Queue Size
        this.videoSampleQueueSize = new InputComponent("videoFrameQueueSize", "number", this.translate("Video Smoothness Buffer", "Buffer Penstabil Video"), {
            defaultValue: defaultSettings_.videoFrameQueueSize.toString(),
            value: settings?.videoFrameQueueSize?.toString()
        })
        this.videoSampleQueueSize.setHelpText(
            "Low values (1-2) reduce input lag but might stutter. High values (3-4) increase smoothness but add slight input delay.",
            "Nilai rendah (1-2) mengurangi jeda kontrol tapi rentan patah. Nilai tinggi (3-4) lebih mulus tapi menambah sedikit jeda input."
        )
        this.videoSampleQueueSize.addChangeListener(this.onSettingsChange.bind(this))
        this.videoSampleQueueSize.mount(this.divElement)

        // Codec
        this.videoCodec = new SelectComponent("videoCodec", [
            { value: "h264", name: "H264" },
            { value: "auto", name: this.translate("Auto (Experimental)", "Otomatis (Eksperimental)") },
            { value: "h265", name: "H265" },
            { value: "av1", name: this.translate("AV1 (Experimental)", "AV1 (Eksperimental)") },
        ], {
            displayName: this.translate("Video Compression Format (Codec)", "Format Kompresi Video (Codec)"),
            preSelectedOption: settings?.videoCodec ?? defaultSettings_.videoCodec
        })
        this.videoCodec.setHelpText(
            "Encoding format (H264 for compatibility, H265/AV1 for better quality and lower data usage on modern devices).",
            "Metode kompresi gambar (H264 untuk PC lama, H265 untuk PC baru, AV1 untuk PC modern agar hemat data dengan gambar tajam)."
        )
        this.videoCodec.addChangeListener(this.onSettingsChange.bind(this))
        this.videoCodec.mount(this.divElement)

        // Force Video Element renderer
        this.forceVideoElementRenderer = new InputComponent("forceVideoElementRenderer", "checkbox", this.translate("Classic Player Mode (Compatibility)", "Mode Pemutar Klasik (Kompatibilitas)"), {
            checked: settings?.forceVideoElementRenderer ?? defaultSettings_.forceVideoElementRenderer
        })
        this.forceVideoElementRenderer.setHelpText(
            "Enable this troubleshooting option only if the screen remains black or video fails to display during streaming.",
            "Aktifkan opsi pemecahan masalah ini hanya jika layar tetap hitam atau video gagal muncul saat mulai streaming."
        )
        this.forceVideoElementRenderer.addChangeListener(this.onSettingsChange.bind(this))
        this.forceVideoElementRenderer.mount(this.divElement)

        // Use Canvas Renderer
        this.canvasRenderer = new InputComponent("canvasRenderer", "checkbox", this.translate("High-Response Screen Mode (Canvas)", "Mode Layar Ultra-Responsif (Canvas)"), {
            defaultValue: defaultSettings_.canvasRenderer.toString(),
            checked: settings === null || settings === void 0 ? void 0 : settings.canvasRenderer
        })
        this.canvasRenderer.setHelpText(
            "Uses HTML5 Canvas to render video. Provides extremely low latency but may lack some hardware optimizations.",
            "Menggunakan teknologi HTML5 Canvas untuk merender video. Memberikan jeda input terkecil namun memakan lebih banyak daya CPU."
        )
        this.canvasRenderer.addChangeListener(this.onSettingsChange.bind(this))
        this.canvasRenderer.mount(this.divElement)

        // Canvas VSync (Canvas only: sync draw to display refresh to reduce tearing; off = lower latency)
        this.canvasVsync = new InputComponent("canvasVsync", "checkbox", this.translate("Smooth Screen Sync (VSync)", "Sinkronisasi Layar Halus (VSync)"), {
            checked: settings?.canvasVsync ?? defaultSettings_.canvasVsync
        })
        this.canvasVsync.setHelpText(
            "Prevents horizontal screen tearing during fast camera movement. Turning VSync OFF can reduce input lag slightly.",
            "Mencegah gambar patah/robek secara horizontal saat kamera berputar cepat. Mematikan ini memberikan respons kontrol sedikit lebih cepat."
        )
        this.canvasVsync.addChangeListener(this.onSettingsChange.bind(this))
        this.canvasVsync.mount(this.divElement)

        // HDR
        this.hdr = new InputComponent("hdr", "checkbox", this.translate("Enable HDR (High Dynamic Range)", "Aktifkan HDR (High Dynamic Range)"), {
            checked: settings?.hdr ?? defaultSettings_.hdr
        })
        this.hdr.setHelpText(
            "Provides richer colors and higher contrast. Requires an HDR-compatible display and H265/AV1 codec support.",
            "Memberikan warna yang lebih hidup dan kontras lebih tinggi. Membutuhkan layar yang mendukung HDR dan kompresi H265/AV1."
        )
        this.hdr.addChangeListener(this.onSettingsChange.bind(this))
        this.hdr.mount(this.divElement)

        // Audio local
        this.audioHeader.innerText = "Audio"
        this.divElement.appendChild(this.audioHeader)

        this.playAudioLocal = new InputComponent("playAudioLocal", "checkbox", this.translate("Play Audio on Host PC", "Putar Suara di PC Host"), {
            checked: settings?.playAudioLocal
        })
        this.playAudioLocal.setHelpText(
            "If checked, game audio will play on the host computer's speakers as well as streaming to your local device.",
            "Jika dicentang, suara game akan diputar di speaker komputer host di samping dikirim ke perangkat lokal kamu."
        )
        this.playAudioLocal.addChangeListener(this.onSettingsChange.bind(this))
        this.playAudioLocal.mount(this.divElement)

        // Audio Sample Queue Size
        this.audioSampleQueueSize = new InputComponent("audioSampleQueueSize", "number", this.translate("Audio Smoothness Buffer", "Buffer Suara (Pencegah Suara Pecah)"), {
            defaultValue: defaultSettings_.audioSampleQueueSize.toString(),
            value: settings?.audioSampleQueueSize?.toString()
        })
        this.audioSampleQueueSize.setHelpText(
            "Adjusts audio buffering. Increase this value if you hear sound stuts, cracks, or static noise due to connection drops.",
            "Mengatur antrean suara. Naikkan nilai jika suara terdengar pecah, tersendat, atau kresek-kresek akibat internet tidak stabil."
        )
        this.audioSampleQueueSize.addChangeListener(this.onSettingsChange.bind(this))
        this.audioSampleQueueSize.mount(this.divElement)

        // Mouse
        this.mouseHeader.innerText = this.translate("Mouse", "Mouse / Kursor")
        this.divElement.appendChild(this.mouseHeader)

        this.mouseScrollMode = new SelectComponent("mouseScrollMode",
            [
                { value: "highres", name: this.translate("High Resolution (Smooth)", "Resolusi Tinggi (Halus)") },
                { value: "normal", name: this.translate("Normal", "Normal / Biasa") }
            ],
            {
                displayName: this.translate("Mouse Scroll Sensitivity", "Sensitivitas Scroll Mouse"),
                preSelectedOption: settings?.mouseScrollMode || defaultSettings_.mouseScrollMode
            }
        )
        this.mouseScrollMode.setHelpText(
            "High Resolution scroll mode provides smooth scrolling, while Normal scroll matches standard wheel increments.",
            "Mode High Res memberikan guliran halaman yang halus, sedangkan Normal menyamakan dengan putaran roda mouse standar."
        )
        this.mouseScrollMode.addChangeListener(this.onSettingsChange.bind(this))
        this.mouseScrollMode.mount(this.divElement)

        // Controller
        if (window.isSecureContext) {
            this.controllerHeader.innerText = this.translate("Gamepad / Controller", "Gamepad / Kontroler")
        } else {
            this.controllerHeader.innerText = this.translate("Gamepad / Controller (Disabled: Secure Context Required)", "Gamepad / Kontroler (Nonaktif: Butuh HTTPS/Aman)")
        }
        this.divElement.appendChild(this.controllerHeader)

        this.controllerInvertAB = new InputComponent("controllerInvertAB", "checkbox", this.translate("Invert A and B Buttons", "Tukar Tombol A dan B"), {
            checked: settings?.controllerConfig.invertAB
        })
        this.controllerInvertAB.setHelpText(
            "Swaps the actions of controller buttons A and B (Nintendo style layout).",
            "Menukar fungsi tombol A dan B pada gamepad (gaya layout Nintendo)."
        )
        this.controllerInvertAB.addChangeListener(this.onSettingsChange.bind(this))
        this.controllerInvertAB.mount(this.divElement)

        this.controllerInvertXY = new InputComponent("controllerInvertXY", "checkbox", this.translate("Invert X and Y Buttons", "Tukar Tombol X dan Y"), {
            checked: settings?.controllerConfig.invertXY
        })
        this.controllerInvertXY.setHelpText(
            "Swaps the actions of controller buttons X and Y (Nintendo style layout).",
            "Menukar fungsi tombol X dan Y pada gamepad (gaya layout Nintendo)."
        )
        this.controllerInvertXY.addChangeListener(this.onSettingsChange.bind(this))
        this.controllerInvertXY.mount(this.divElement)

        // Controller Send Interval
        this.controllerSendIntervalOverride = new InputComponent("controllerSendIntervalOverride", "number", this.translate("Override Controller Update Interval", "Sesuaikan Interval Update Kontroler"), {
            hasEnableCheckbox: true,
            defaultValue: "20",
            value: settings?.controllerConfig.sendIntervalOverride?.toString(),
            numberSlider: {
                range_min: 10,
                range_max: 120
            }
        })
        this.controllerSendIntervalOverride.setHelpText(
            "Sets custom update rates in milliseconds for controller state reports. Recommended: 20ms.",
            "Mengatur interval waktu pembaruan status gamepad dalam milidetik. Rekomendasi: 20ms."
        )
        this.controllerSendIntervalOverride.setEnabled(settings?.controllerConfig.sendIntervalOverride != null)
        this.controllerSendIntervalOverride.addChangeListener(this.onSettingsChange.bind(this))
        this.controllerSendIntervalOverride.mount(this.divElement)

        if (!window.isSecureContext) {
            this.controllerInvertAB.setEnabled(false)
            this.controllerInvertXY.setEnabled(false)
        }

        // Other
        this.otherHeader.innerText = this.translate("Other", "Lain-lain")
        this.divElement.appendChild(this.otherHeader)

        this.uiLanguage = new SelectComponent("uiLanguage", [
            { value: "en", name: "English" },
            { value: "id", name: "Bahasa Indonesia" },
        ], {
            displayName: this.translate("UI Language", "Bahasa Tampilan (UI)"),
            preSelectedOption: settings?.uiLanguage ?? defaultSettings_.uiLanguage
        })
        this.uiLanguage.setHelpText(
            "Changes the app's display language.",
            "Mengubah bahasa tampilan aplikasi."
        )
        this.uiLanguage.addChangeListener(this.onSettingsChange.bind(this))
        this.uiLanguage.mount(this.divElement)

        const dataTransportOptions = [
            { value: "webrtc", name: WEBSOCKET_RELAY_ENABLED ? this.translate("WebRTC (Recommended)", "WebRTC (Rekomendasi)") : this.translate("WebRTC (P2P Only)", "WebRTC (Hanya P2P)") },
        ]
        if (WEBSOCKET_RELAY_ENABLED) {
            dataTransportOptions.push({ value: "websocket", name: this.translate("Web Socket Relay (Experimental)", "Relay Web Socket (Eksperimental)") })
        }

        this.dataTransport = new SelectComponent("transport", dataTransportOptions, {
            displayName: this.translate("Primary Connection Route", "Jalur Koneksi Utama"),
            preSelectedOption: settings?.dataTransport ?? defaultSettings_.dataTransport
        })
        this.dataTransport.setHelpText(
            "Selects WebRTC (P2P direct route, lowest lag) or WebSocket Relay (goes through intermediate server if P2P is blocked).",
            "Memilih WebRTC (koneksi langsung P2P tanpa perantara, paling rendah jeda) atau WebSocket Relay (melalui server perantara)."
        )
        this.dataTransport.addChangeListener(this.onSettingsChange.bind(this))
        this.dataTransport.mount(this.divElement)

        this.toggleFullscreenWithKeybind = new InputComponent("toggleFullscreenWithKeybind", "checkbox", this.translate("Enable Fullscreen Hotkey", "Aktifkan Hotkey Layar Penuh"), {
            checked: settings?.toggleFullscreenWithKeybind
        })
        this.toggleFullscreenWithKeybind.setHelpText(
            "Allows toggling fullscreen mode and locking the mouse pointer using the Ctrl + Shift + I keyboard shortcut.",
            "Mengizinkan masuk/keluar mode layar penuh dan mengunci kursor mouse menggunakan pintasan keyboard Ctrl + Shift + I."
        )
        this.toggleFullscreenWithKeybind.addChangeListener(this.onSettingsChange.bind(this))
        this.toggleFullscreenWithKeybind.mount(this.divElement)

        this.pageStyle = new SelectComponent("pageStyle", [
            { value: "standard", name: this.translate("Standard", "Standar") },
            { value: "moonlight", name: this.translate("Cloudgime Classic", "Cloudgime Klasik") },
        ], {
            displayName: this.translate("Lobby Theme Style", "Gaya Tema Lobby"),
            preSelectedOption: settings?.pageStyle ?? defaultSettings_.pageStyle
        })
        this.pageStyle.setHelpText(
            "Select the visual style for the dashboard lobby (Standard or Cloudgime Classic).",
            "Pilih gaya tampilan visual untuk halaman menu utama lobby (Standar atau Cloudgime Klasik)."
        )
        this.pageStyle.addChangeListener(this.onSettingsChange.bind(this))
        this.pageStyle.mount(this.divElement)

        this.useSelectElementPolyfill = new InputComponent("useSelectElementPolyfill", "checkbox", this.translate("Enable Modern UI Style", "Aktifkan Gaya Tampilan Modern"), {
            checked: settings?.useSelectElementPolyfill ?? defaultSettings_.useSelectElementPolyfill
        })
        this.useSelectElementPolyfill.setHelpText(
            "Replaces standard browser dropdown boxes with customized, styled UI elements.",
            "Mengganti kotak pilihan menu dropdown standar browser dengan elemen antarmuka yang didekorasi lebih modern."
        )
        this.useSelectElementPolyfill.addChangeListener(this.onSettingsChange.bind(this))
        this.useSelectElementPolyfill.mount(this.divElement)

        this.onSettingsChange()
    }

    private onSettingsChange() {
        if (this.videoSize.getValue() == "custom") {
            this.videoSizeWidth.setEnabled(true)
            this.videoSizeHeight.setEnabled(true)
        } else {
            this.videoSizeWidth.setEnabled(false)
            this.videoSizeHeight.setEnabled(false)
        }

        this.divElement.dispatchEvent(new ComponentEvent("ml-settingschange", this))
    }

    addChangeListener(listener: StreamSettingsChangeListener) {
        this.divElement.addEventListener("ml-settingschange", listener as any)
    }
    removeChangeListener(listener: StreamSettingsChangeListener) {
        this.divElement.removeEventListener("ml-settingschange", listener as any)
    }

    getStreamSettings(): Settings {
        const settings = defaultSettings()

        settings.sidebarEdge = this.sidebarEdge.getValue() as any
        settings.bitrate = parseInt(this.bitrate.getValue())
        settings.adaptiveBitrate = this.adaptiveBitrate.isChecked()
        settings.packetSize = parseInt(this.packetSize.getValue())
        settings.fps = parseInt(this.fps.getValue())
        settings.videoSize = this.videoSize.getValue() as any
        settings.videoSizeCustom = {
            width: parseInt(this.videoSizeWidth.getValue()),
            height: parseInt(this.videoSizeHeight.getValue())
        }
        settings.videoFrameQueueSize = parseInt(this.videoSampleQueueSize.getValue())
        settings.videoCodec = this.videoCodec.getValue() as any
        settings.forceVideoElementRenderer = this.forceVideoElementRenderer.isChecked()
        settings.canvasRenderer = this.canvasRenderer.isChecked()
        settings.canvasVsync = this.canvasVsync.isChecked()

        settings.playAudioLocal = this.playAudioLocal.isChecked()
        settings.audioSampleQueueSize = parseInt(this.audioSampleQueueSize.getValue())

        settings.mouseScrollMode = this.mouseScrollMode.getValue() as any

        settings.controllerConfig.invertAB = this.controllerInvertAB.isChecked()
        settings.controllerConfig.invertXY = this.controllerInvertXY.isChecked()
        if (this.controllerSendIntervalOverride.isEnabled()) {
            settings.controllerConfig.sendIntervalOverride = parseInt(this.controllerSendIntervalOverride.getValue())
        } else {
            settings.controllerConfig.sendIntervalOverride = null
        }

        settings.uiLanguage = this.uiLanguage.getValue() as UiLanguage
        settings.dataTransport = normalizeTransportType(this.dataTransport.getValue() as TransportType)

        settings.toggleFullscreenWithKeybind = this.toggleFullscreenWithKeybind.isChecked()

        settings.pageStyle = this.pageStyle.getValue() as any

        settings.hdr = this.hdr.isChecked()

        settings.useSelectElementPolyfill = this.useSelectElementPolyfill.isChecked()

        return settings
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.divElement)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.divElement)
    }
}
