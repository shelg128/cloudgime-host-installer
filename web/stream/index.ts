import { Api } from "../api.js"
import { App, ConnectionStatus, DisplayModePhase, GeneralClientMessage, GeneralServerMessage, HostMouseEmulationMode, StreamCapabilities, StreamClientMessage, StreamServerMessage, TransportChannelId, VideoFlowPhase } from "../api_bindings.js"
import { showErrorPopup } from "../component/error.js"
import { Component } from "../component/index.js"
import { Settings, WEBSOCKET_RELAY_ENABLED } from "../component/settings_menu.js"
import { AudioPlayer } from "./audio/index.js"
import { buildAudioPipeline } from "./audio/pipeline.js"
import { BIG_BUFFER, ByteBuffer } from "./buffer.js"
import { defaultStreamInputConfig, StreamInput } from "./input.js"
import { Logger, LogMessageInfo } from "./log.js"
import { gatherPipeInfo, getPipe } from "./pipeline/index.js"
import { StreamStats } from "./stats.js"
import {
    Transport,
    TransportInboundVideoActivitySnapshot,
    TransportMicrophoneDevice,
    TransportMicrophoneDeviceResult,
    TransportMicrophoneDiagnostics,
    TransportMicrophoneSetResult,
    TransportMicrophoneState,
    TransportShutdown,
    VideoTrackTransportChannel
} from "./transport/index.js"
import { WebSocketTransport } from "./transport/web_socket.js"
import { WebRTCTransport } from "./transport/webrtc.js"
import { allVideoCodecs, andVideoCodecs, createSupportedVideoFormatsBits, emptyVideoCodecs, getSelectedVideoCodec, hasAnyCodec, VideoCodecSupport } from "./video.js"
import { TrackVideoRenderer, VideoRenderer } from "./video/index.js"
import { buildVideoPipeline, VideoPipelineOptions } from "./video/pipeline.js"

export type ExecutionEnvironment = {
    main: boolean
    worker: boolean
}

export type InfoEvent = CustomEvent<
    { type: "app", app: App } |
    { type: "serverMessage", message: string } |
    { type: "connectionComplete", capabilities: StreamCapabilities } |
    { type: "displayModeApplied", phase: DisplayModePhase, width: number, height: number, fps: number, changed: boolean, skipped: boolean } |
    { type: "videoFlowReady", phase: VideoFlowPhase, width: number, height: number, fps: number } |
    { type: "videoReconfigured", width: number, height: number, fps: number } |
    { type: "startupInterrupted", reason: string } |
    { type: "runtimeResizeTimeout", width: number, height: number, fps: number } |
    { type: "startupTimeout" } |
    { type: "connectionStatus", status: ConnectionStatus } |
    { type: "addDebugLine", line: string, additional?: LogMessageInfo }
>
export type InfoEventListener = (event: InfoEvent) => void

const DYNAMIC_DEVICE_MATCH_FLAG_KEY = "ML_DYNAMIC_DEVICE_MATCH"
const NATIVE_ANDROID_BRIDGE_SMART_CAPPING_STORAGE_KEY = "mlNativeAndroidBridgeSmartCappingV1"
type ResponsiveStreamTierId = "540p" | "720p" | "1080p" | "1440p" | "4k"
const RESPONSIVE_STREAM_TIER_SHORT_EDGE: Record<ResponsiveStreamTierId, number> = {
    "540p": 540,
    "720p": 720,
    "1080p": 1080,
    "1440p": 1440,
    "4k": 2160
}

function isDynamicDeviceMatchFlagEnabled(): boolean {
    try {
        return window.localStorage.getItem(DYNAMIC_DEVICE_MATCH_FLAG_KEY) != "0"
    } catch {
        return true
    }
}

export function shouldUseDynamicDeviceMatch(settings: Settings): boolean {
    return (settings.videoSize == "native" || isResponsiveStreamTier(settings.videoSize)) && isDynamicDeviceMatchFlagEnabled()
}

function detectStreamModuleBuildLabel(): string {
    const extractVersion = (source: string): string | null => {
        const match = source.match(/(?:\d{8})?(q\d+)/i) ?? source.match(/[?&]v=([^&]+)/i)
        return match?.[1]?.toLowerCase() ?? null
    }

    const versionSources: string[] = []

    if (typeof import.meta?.url == "string" && import.meta.url.length > 0) {
        versionSources.push(import.meta.url)
    }

    if (typeof performance != "undefined" && typeof performance.getEntriesByType == "function") {
        const resourceEntries = performance.getEntriesByType("resource")
        for (const entry of resourceEntries) {
            if ("name" in entry && typeof entry.name == "string" && /\/stream(\/|\.js)/i.test(entry.name)) {
                versionSources.push(entry.name)
            }
        }
    }

    for (const script of Array.from(document.scripts)) {
        if (script.src) {
            versionSources.push(script.src)
        }
    }

    for (const source of versionSources) {
        const version = extractVersion(source)
        if (version) {
            return version
        }
    }

    return "unknown"
}

function roundStreamDimension(value: number, minimum: number): number {
    const min = Math.ceil(minimum / 8) * 8
    return Math.max(min, Math.round(value / 8) * 8)
}

function isSmartStreamTierCappingEnabled(): boolean {
    try {
        return window.localStorage.getItem(NATIVE_ANDROID_BRIDGE_SMART_CAPPING_STORAGE_KEY) != "0"
    } catch {
        return true
    }
}

export function isResponsiveStreamTier(videoSize: Settings["videoSize"]): boolean {
    return typeof RESPONSIVE_STREAM_TIER_SHORT_EDGE[videoSize as ResponsiveStreamTierId] == "number"
}

function sizesEqual(a: [number, number], b: [number, number]): boolean {
    return a[0] == b[0] && a[1] == b[1]
}

function isTouchMobileLikeClient(): boolean {
    const touchPoints = Number(navigator.maxTouchPoints || 0)
    const userAgent = String(navigator.userAgent || "")
    return touchPoints > 0 && /(Android|iPhone|iPad|Mobile)/i.test(userAgent)
}

function shouldPreferCanvasRendererOnTouchMobile(): boolean {
    if (!isTouchMobileLikeClient()) {
        return false
    }

    try {
        const override = window.localStorage.getItem("ML_ANDROID_SCREENSHOT_CANVAS")
        if (override == "0") {
            return false
        }
        if (override == "1") {
            return true
        }
    } catch {
        // Ignore storage failures and fall back to the mobile default.
    }

    return false
}

function shouldPreferStableVideoElementRendererOnTouchMobile(): boolean {
    if (!isTouchMobileLikeClient()) {
        return false
    }

    try {
        const override = window.localStorage.getItem("ML_TOUCH_STABLE_VIDEO_ELEMENT")
        if (override == "0") {
            return false
        }
        if (override == "1") {
            return true
        }
    } catch {
        // Ignore storage failures and fall back to the mobile default.
    }

    return true
}

function shouldAutoWakeHostMouse(): boolean {
    try {
        const override = window.localStorage.getItem("ML_AUTO_WAKE_MOUSE")
        if (override == "0") {
            return false
        }
        if (override == "1") {
            return true
        }
    } catch {
        // Ignore storage failures and use the default heuristic.
    }

    return isTouchMobileLikeClient()
}

function shouldUseMouseWakeDummyClick(): boolean {
    try {
        const override = window.localStorage.getItem("ML_AUTO_WAKE_MOUSE_CLICK")
        if (override == "0") {
            return false
        }
        if (override == "1") {
            return true
        }
    } catch {
        // Ignore storage failures and use the safer default.
    }

    return false
}

export function getDeviceMatchStreamerSize(
    viewerScreenSize: [number, number],
    devicePixelRatio: number = window.devicePixelRatio || 1
): [number, number] {
    const cssWidth = Math.max(1, viewerScreenSize[0])
    const cssHeight = Math.max(1, viewerScreenSize[1])

    if (isTouchMobileLikeClient()) {
        let width = cssWidth
        let height = cssHeight
        const dpr = Math.min(1.5, Math.max(1, Number.isFinite(devicePixelRatio) ? devicePixelRatio : 1))

        // Touch/mobile clients need a denser target than raw CSS pixels;
        // otherwise the stream looks soft on tablets/phones even when the
        // aspect ratio is correct. Use a moderated DPR scale so the host gets
        // a near-native shape without jumping to unstable 4k-class targets.
        width *= dpr
        height *= dpr

        const longEdge = Math.max(width, height)
        const shortEdge = Math.max(1, Math.min(width, height))
        const scale = Math.min(1, 2400 / longEdge, 1440 / shortEdge)
        width *= scale
        height *= scale

        return [
            roundStreamDimension(width, 320),
            roundStreamDimension(height, 180)
        ]
    }

    const dpr = Math.min(4, Math.max(1, Number.isFinite(devicePixelRatio) ? devicePixelRatio : 1))
    let width = cssWidth * dpr
    let height = cssHeight * dpr

    const longEdge = Math.max(width, height, cssWidth, cssHeight)
    const shortEdge = Math.max(1, Math.min(width, height))
    const scale = Math.min(1, 3840 / longEdge, 2160 / shortEdge)

    width *= scale
    height *= scale

    return [
        roundStreamDimension(width, 320),
        roundStreamDimension(height, 180)
    ]
}

export function getSafeInitialDeviceMatchStreamerSize(viewerScreenSize: [number, number]): [number, number] {
    const cssWidth = Math.max(1, viewerScreenSize[0])
    const cssHeight = Math.max(1, viewerScreenSize[1])
    const cssShortEdge = Math.min(cssWidth, cssHeight)

    // Native Moonlight and Sunshine keep the startup path conservative and only
    // promote presentation once the session is alive. Mirror that here with a
    // stable bootstrap mode that preserves aspect ratio but avoids large, highly
    // device-specific viewport targets during initial connect.
    let width = cssWidth
    let height = cssHeight
    const bootstrapShortEdge = Math.min(720, Math.max(360, cssShortEdge))
    const bootstrapScale = bootstrapShortEdge / cssShortEdge
    width *= bootstrapScale
    height *= bootstrapScale

    const maxLongEdge = 1280
    const clampScale = Math.min(1, maxLongEdge / Math.max(width, height))
    width *= clampScale
    height *= clampScale

    return [
        roundStreamDimension(width, 320),
        roundStreamDimension(height, 180)
    ]
}

export function getResponsiveStreamTierSize(
    videoSize: Settings["videoSize"] | "540p",
    viewerScreenSize: [number, number],
    smartCapping: boolean = isSmartStreamTierCappingEnabled()
): [number, number] | null {
    const requestedShortEdge = RESPONSIVE_STREAM_TIER_SHORT_EDGE[videoSize as ResponsiveStreamTierId]
    if (!requestedShortEdge) {
        return null
    }

    const viewportWidth = Math.max(1, viewerScreenSize[0])
    const viewportHeight = Math.max(1, viewerScreenSize[1])
    const viewportShortEdge = Math.min(viewportWidth, viewportHeight)
    const viewportLongEdge = Math.max(viewportWidth, viewportHeight)
    const aspectRatio = Math.max(1, viewportLongEdge / Math.max(1, viewportShortEdge))
    const deviceMatchSize = getDeviceMatchStreamerSize(viewerScreenSize)
    let smartCapShortEdge = Math.max(1, Math.min(deviceMatchSize[0], deviceMatchSize[1]))
    if (isTouchMobileLikeClient()) {
        const screenWidth = Math.max(0, Number(window.screen?.width || 0))
        const screenHeight = Math.max(0, Number(window.screen?.height || 0))
        const dpr = Math.max(1, Number.isFinite(window.devicePixelRatio) ? window.devicePixelRatio : 1)
        const physicalShortEdge = Math.min(screenWidth, screenHeight) * dpr
        if (Number.isFinite(physicalShortEdge) && physicalShortEdge > 0) {
            smartCapShortEdge = Math.max(smartCapShortEdge, Math.min(physicalShortEdge, 1440))
        }
    }
    const effectiveShortEdge = smartCapping
        ? Math.min(requestedShortEdge, smartCapShortEdge)
        : requestedShortEdge
    const isLandscape = viewportWidth >= viewportHeight
    const width = isLandscape
        ? effectiveShortEdge * aspectRatio
        : effectiveShortEdge
    const height = isLandscape
        ? effectiveShortEdge
        : effectiveShortEdge * aspectRatio

    return [
        roundStreamDimension(width, 320),
        roundStreamDimension(height, 180)
    ]
}

export function getStreamerSize(settings: Settings, viewerScreenSize: [number, number]): [number, number] {
    let width, height
    const responsiveTierSize = getResponsiveStreamTierSize(settings.videoSize, viewerScreenSize)
    if (responsiveTierSize) {
        [width, height] = responsiveTierSize
    } else if (settings.videoSize == "custom") {
        width = settings.videoSizeCustom.width
        height = settings.videoSizeCustom.height
    } else { // native
        [width, height] = getDeviceMatchStreamerSize(viewerScreenSize)
    }
    return [width, height]
}

function getVideoCodecHint(settings: Settings): VideoCodecSupport {
    let videoCodecHint = emptyVideoCodecs()
    if (settings.videoCodec == "h264") {
        videoCodecHint.H264 = true
        videoCodecHint.H264_HIGH8_444 = true
    } else if (settings.videoCodec == "h265") {
        videoCodecHint.H265 = true
        videoCodecHint.H265_MAIN10 = true
        videoCodecHint.H265_REXT8_444 = true
        videoCodecHint.H265_REXT10_444 = true
    } else if (settings.videoCodec == "av1") {
        videoCodecHint.AV1 = true
        videoCodecHint.AV1_MAIN8 = true
        videoCodecHint.AV1_MAIN10 = true
        videoCodecHint.AV1_REXT8_444 = true
        videoCodecHint.AV1_REXT10_444 = true
    } else if (settings.videoCodec == "auto") {
        videoCodecHint = allVideoCodecs()
    }
    return videoCodecHint
}

function shouldUseSafeInitialDeviceMatchStartup(settings: Settings, viewerScreenSize: [number, number]): boolean {
    if (!shouldUseDynamicDeviceMatch(settings)) {
        return false
    }

    try {
        const override = localStorage.getItem("ML_SAFE_INITIAL_DEVICE_MATCH")
        if (override == "0") {
            return false
        }
        if (override == "1") {
            return true
        }
    } catch {
        // Ignore local storage failures and fall back to automatic detection.
    }

    // Default to pre-stream negotiation using the viewport-sized StartStream
    // target. Keep the old conservative bootstrap only as an explicit escape
    // hatch for debugging problematic hosts.
    return false
}

type DeviceMatchStartupPlan = {
    startupSize: [number, number]
    promotedSize: [number, number] | null
    usedBootstrap: boolean
}

function buildDeviceMatchStartupPlan(settings: Settings, viewerScreenSize: [number, number]): DeviceMatchStartupPlan {
    const startupSize = shouldUseSafeInitialDeviceMatchStartup(settings, viewerScreenSize)
        ? getSafeInitialDeviceMatchStreamerSize(viewerScreenSize)
        : getStreamerSize(settings, viewerScreenSize)

    if (!shouldUseDynamicDeviceMatch(settings)) {
        return {
            startupSize,
            promotedSize: null,
            usedBootstrap: false
        }
    }

    const promotedSize = getStreamerSize(settings, viewerScreenSize)
    const usedBootstrap = !sizesEqual(startupSize, promotedSize)

    return {
        startupSize,
        promotedSize: usedBootstrap ? promotedSize : null,
        usedBootstrap
    }
}

function getSafeStartPacketSize(settings: Settings): number {
    const requestedPacketSize = Math.max(576, Math.round(Number(settings.packetSize || 0) || 1200))
    if (isTouchMobileLikeClient()) {
        return Math.min(requestedPacketSize, 1024)
    }

    return Math.min(requestedPacketSize, 1200)
}

export class Stream implements Component {
    private static nextInstanceId = 1

    private logger: Logger = new Logger()
    private readonly buildLabel = detectStreamModuleBuildLabel()
    private readonly instanceLabel = `stream#${Stream.nextInstanceId++}`

    private api: Api

    private hostId: number
    private appId: number

    private settings: Settings

    private divElement = document.createElement("div")
    private eventTarget = new EventTarget()

    private ws: WebSocket
    private iceServers: Array<RTCIceServer> | null = null

    private videoRenderer: VideoRenderer | null = null
    private audioPlayer: AudioPlayer | null = null
    private videoTrackChannel: VideoTrackTransportChannel | null = null
    private videoTrackListener: ((track: MediaStreamTrack) => void) | null = null
    private lastVideoSetup: { formatRaw: number, width: number, height: number, fps: number } | null = null

    private input: StreamInput
    private stats: StreamStats

    private streamerSize: [number, number]
    private readonly preferredRuntimeStreamerSize: [number, number] | null
    private readonly usedBootstrapDeviceMatchStartup: boolean
    private readonly dynamicDisplayMatchInitEnabled: boolean
    private capabilities: StreamCapabilities | null = null
    private runtimeResizeInFlight = false
    private queuedRuntimeResize: { width: number, height: number, fps: number } | null = null
    private runtimeResizeAckTimer: number | null = null
    private runtimeResizeRequestedSize: { width: number, height: number, fps: number } | null = null
    private startStreamGuardTimer: number | null = null
    private connectionCompleted = false
    private activeHostMouseEmulationMode: HostMouseEmulationMode | null = null
    private negotiatedVideoCodecSupport: VideoCodecSupport = emptyVideoCodecs()
    private pendingWsCloseReason: string | null = null
    private wsHeartbeatTimer: number | null = null
    private mouseWakeSent = false
    private mouseWakeTimer: number | null = null
    private readonly startupGuardTimeoutMs: number

    constructor(
        api: Api,
        hostId: number,
        appId: number,
        settings: Settings,
        viewerScreenSize: [number, number],
        startupGuardTimeoutMs?: number,
    ) {
        this.logger.addInfoListener((info, type) => {
            this.debugLog(info, { type: type ?? undefined })
        })

        this.api = api

        this.hostId = hostId
        this.appId = appId

        this.settings = settings

        const startupPlan = buildDeviceMatchStartupPlan(settings, viewerScreenSize)
        // Let hosts that support it align the virtual display with native/device-sized streams.
        this.dynamicDisplayMatchInitEnabled = shouldUseDynamicDeviceMatch(settings)
        this.usedBootstrapDeviceMatchStartup = startupPlan.usedBootstrap
        this.preferredRuntimeStreamerSize = startupPlan.promotedSize
        this.streamerSize = startupPlan.startupSize
        this.startupGuardTimeoutMs = Math.max(
            8000,
            Math.round(
                Number.isFinite(startupGuardTimeoutMs ?? NaN)
                    ? Number(startupGuardTimeoutMs)
                    : (isTouchMobileLikeClient() ? 14000 : 11000)
            )
        )
        this.logClientEvent(
            `created build=${this.buildLabel} initDynamic=${this.dynamicDisplayMatchInitEnabled ? "yes" : "no"} startup=${this.streamerSize[0]}x${this.streamerSize[1]} bootstrap=${this.usedBootstrapDeviceMatchStartup ? "yes" : "no"} promoted=${this.preferredRuntimeStreamerSize ? `${this.preferredRuntimeStreamerSize[0]}x${this.preferredRuntimeStreamerSize[1]}` : "none"} startupGuard=${this.startupGuardTimeoutMs}ms`
        )

        // Configure web socket
        const wsApiHost = api.host_url.replace(/^http(s)?:/, "ws$1:")
        this.ws = new WebSocket(`${wsApiHost}/host/stream`)
        this.ws.addEventListener("error", this.onError.bind(this))
        this.ws.addEventListener("open", this.onWsOpen.bind(this))
        this.ws.addEventListener("close", this.onWsClose.bind(this))
        this.ws.addEventListener("message", this.onRawWsMessage.bind(this))

        this.sendWsMessage({
            Init: {
                host_id: this.hostId,
                app_id: this.appId,
                video_frame_queue_size: this.settings.videoFrameQueueSize,
                audio_sample_queue_size: this.settings.audioSampleQueueSize,
                client_build: this.buildLabel,
                dynamic_display_match: this.dynamicDisplayMatchInitEnabled,
            }
        })

        // Stream Input
        const streamInputConfig = defaultStreamInputConfig()
        Object.assign(streamInputConfig, {
            mouseScrollMode: this.settings.mouseScrollMode,
            controllerConfig: this.settings.controllerConfig
        })
        this.input = new StreamInput(streamInputConfig)

        // Stream Stats
        this.stats = new StreamStats()
    }

    private debugLog(message: string, additional?: LogMessageInfo) {
        for (const line of message.split("\n")) {
            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "addDebugLine", line, additional }
            })

            this.eventTarget.dispatchEvent(event)
        }
    }

    logClientEvent(message: string, additional?: LogMessageInfo) {
        this.debugLog(`[${this.instanceLabel}] ${message}`, additional)
    }

    private getLifecycleSnapshot(): string {
        return `readyState=${this.ws.readyState} transport=${this.transport?.implementationName ?? "none"} streamer=${this.streamerSize[0]}x${this.streamerSize[1]}`
    }

    private formatCloseReason(reason: string): string {
        const trimmed = String(reason ?? "")
            .replace(/[\r\n\t]+/g, " ")
            .replace(/\s+/g, " ")
            .trim()
        const normalized = trimmed.length > 0 ? trimmed : "unspecified"
        return normalized.slice(0, 120)
    }

    private clearRuntimeResizeAckTimer() {
        if (this.runtimeResizeAckTimer != null) {
            window.clearTimeout(this.runtimeResizeAckTimer)
            this.runtimeResizeAckTimer = null
        }
    }

    private clearStartStreamGuardTimer() {
        if (this.startStreamGuardTimer != null) {
            window.clearTimeout(this.startStreamGuardTimer)
            this.startStreamGuardTimer = null
        }
    }

    private armStartStreamGuardTimer() {
        this.clearStartStreamGuardTimer()
        const timeoutMs = this.startupGuardTimeoutMs
        this.startStreamGuardTimer = window.setTimeout(() => {
            this.startStreamGuardTimer = null
            if (this.connectionCompleted) {
                return
            }

            this.logClientEvent(`StartStream timed out before ConnectionComplete after ${timeoutMs}ms (${this.getLifecycleSnapshot()})`)
            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "startupTimeout" }
            })
            this.eventTarget.dispatchEvent(event)
        }, timeoutMs)
    }

    private flushQueuedRuntimeResize() {
        if (!this.queuedRuntimeResize) {
            return
        }

        const queued = this.queuedRuntimeResize
        this.queuedRuntimeResize = null
        void this.requestRuntimeResize([queued.width, queued.height], queued.fps)
    }

    private isSameRuntimeResizeTarget(
        left: { width: number, height: number, fps: number } | null,
        right: { width: number, height: number, fps: number } | null
    ) {
        return !!left && !!right
            && left.width == right.width
            && left.height == right.height
            && left.fps == right.fps
    }

    private clearWsHeartbeatTimer() {
        if (this.wsHeartbeatTimer != null) {
            window.clearInterval(this.wsHeartbeatTimer)
            this.wsHeartbeatTimer = null
        }
    }

    private clearMouseWakeTimer() {
        if (this.mouseWakeTimer != null) {
            window.clearTimeout(this.mouseWakeTimer)
            this.mouseWakeTimer = null
        }
    }

    noteUserInteraction() {
        if (this.mouseWakeSent) {
            return
        }

        if (this.mouseWakeTimer != null) {
            this.clearMouseWakeTimer()
            this.logClientEvent("Cancelling pending mouse wake pulse after user interaction")
        }

        // Once the user is actively driving input, a delayed wake pulse is more
        // harmful than helpful because it can snap the host cursor back toward
        // the center just after the first touch/move.
        this.mouseWakeSent = true
    }

    private triggerMouseWakePulse(reason: string) {
        const hostMouseEmulation = this.activeHostMouseEmulationMode ?? this.getHostMouseEmulationMode()
        if (this.mouseWakeSent || !shouldAutoWakeHostMouse() || hostMouseEmulation != "absoluteFollow") {
            return
        }

        const withDummyClick = shouldUseMouseWakeDummyClick()
        const sent = this.input.sendMouseWakePulse(withDummyClick)
        this.logClientEvent(`Mouse wake pulse reason=${reason} sent=${sent ? "yes" : "no"} dummyClick=${withDummyClick ? "yes" : "no"}`)
        if (sent) {
            this.mouseWakeSent = true
        }
    }

    private scheduleMouseWakePulse(reason: string, delayMs: number = 140) {
        const hostMouseEmulation = this.activeHostMouseEmulationMode ?? this.getHostMouseEmulationMode()
        if (this.mouseWakeSent || !shouldAutoWakeHostMouse() || hostMouseEmulation != "absoluteFollow") {
            return
        }

        this.clearMouseWakeTimer()
        this.mouseWakeTimer = window.setTimeout(() => {
            this.mouseWakeTimer = null
            this.triggerMouseWakePulse(reason)
        }, delayMs)
    }

    private sendWsHeartbeat() {
        if (this.ws.readyState != WebSocket.OPEN) {
            return
        }

        this.ws.send(JSON.stringify({
            Heartbeat: {
                ts_ms: Date.now(),
            }
        } satisfies StreamClientMessage))
    }

    private startWsHeartbeat() {
        this.clearWsHeartbeatTimer()
        this.sendWsHeartbeat()
        this.wsHeartbeatTimer = window.setInterval(() => {
            this.sendWsHeartbeat()
        }, 4000)
    }

    private clearVideoTrackBinding() {
        if (this.videoTrackChannel && this.videoTrackListener) {
            this.videoTrackChannel.removeTrackListener(this.videoTrackListener)
        }
        this.videoTrackChannel = null
        this.videoTrackListener = null
    }

    private bindTrackVideoRenderer(channel: VideoTrackTransportChannel, renderer: TrackVideoRenderer & VideoRenderer) {
        this.clearVideoTrackBinding()
        const listener = (track: MediaStreamTrack) => {
            renderer.setTrack(track)
        }
        this.videoTrackChannel = channel
        this.videoTrackListener = listener
        channel.addTrackListener(listener)
    }

    private getVideoPipelineOptions(
        transportVideoType: "videotrack" | "data",
        transportCodecSupport: VideoCodecSupport
    ): VideoPipelineOptions {
        const codecHint = getVideoCodecHint(this.settings)
        this.debugLog(`Codec Hint by the user: ${JSON.stringify(codecHint)}`)

        const preferStableVideoElementRenderer = shouldPreferStableVideoElementRendererOnTouchMobile()
        const preferCanvasRenderer = !this.settings.forceVideoElementRenderer
            && !preferStableVideoElementRenderer
            && shouldPreferCanvasRendererOnTouchMobile()
        if (preferCanvasRenderer && !this.settings.canvasRenderer) {
            this.debugLog("Touch/mobile client detected. Canvas renderer override enabled for screenshot capture.")
        }
        if (preferStableVideoElementRenderer && !this.settings.forceVideoElementRenderer) {
            this.debugLog("Touch/mobile client detected. Stable video element renderer override enabled.")
        }

        const userRequestedWebRtcCanvasRenderer = transportVideoType == "videotrack"
            && this.settings.canvasRenderer
            && !this.settings.forceVideoElementRenderer
        const autoForceVideoElementRenderer = transportVideoType == "videotrack" && !userRequestedWebRtcCanvasRenderer
        if (autoForceVideoElementRenderer) {
            this.debugLog("WebRTC video track detected. Forcing stable video element renderer.")
        } else if (userRequestedWebRtcCanvasRenderer) {
            this.debugLog("Experimental WebRTC canvas renderer enabled by user setting.")
        }

        return {
            supportedVideoCodecs: andVideoCodecs(codecHint, transportCodecSupport),
            canvasRenderer: this.settings.canvasRenderer || preferCanvasRenderer,
            forceVideoElementRenderer: this.settings.forceVideoElementRenderer || autoForceVideoElementRenderer,
            canvasVsync: this.settings.canvasVsync
        }
    }

    private async setupVideoRenderer(
        renderer: VideoRenderer,
        formatRaw: number,
        width: number,
        height: number,
        fps: number
    ) {
        const format = getSelectedVideoCodec(formatRaw)
        if (format == null) {
            this.debugLog(`Video Format ${formatRaw} was not found! Couldn't update stream video setup!`, { type: "fatal" })
            return false
        }

        this.lastVideoSetup = {
            formatRaw,
            width,
            height,
            fps
        }

        this.streamerSize = [width, height]
        this.stats.setVideoInfo(String(format ?? "Unknown"), width, height, fps)
        await renderer.setup({
            codec: format,
            fps,
            width,
            height,
        })
        return true
    }

    private async applyVideoSetup(
        formatRaw: number,
        width: number,
        height: number,
        fps: number,
        capabilities?: StreamCapabilities
    ) {
        if (capabilities) {
            this.capabilities = capabilities
            this.input.onStreamStart(capabilities, [width, height])
        } else {
            this.input.updateStreamerSize([width, height])
        }

        if (!this.videoRenderer) {
            throw "Video renderer not initialized!"
        }

        await this.setupVideoRenderer(this.videoRenderer, formatRaw, width, height, fps)
    }

    private async onMessage(message: StreamServerMessage) {
        if ("DebugLog" in message) {
            const debugLog = message.DebugLog

            this.debugLog(debugLog.message, {
                type: debugLog.ty ?? undefined
            })
        } else if ("UpdateApp" in message) {
            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "app", app: message.UpdateApp.app }
            })

            this.eventTarget.dispatchEvent(event)
        } else if ("ConnectionComplete" in message) {
            const capabilities = message.ConnectionComplete.capabilities
            const formatRaw = message.ConnectionComplete.format
            const width = message.ConnectionComplete.width
            const height = message.ConnectionComplete.height
            const fps = message.ConnectionComplete.fps
            this.connectionCompleted = true
            this.clearStartStreamGuardTimer()

            const audioSampleRate = message.ConnectionComplete.audio_sample_rate
            const audioChannelCount = message.ConnectionComplete.audio_channel_count
            const audioStreams = message.ConnectionComplete.audio_streams
            const audioCoupledStreams = message.ConnectionComplete.audio_coupled_streams
            const audioSamplesPerFrame = message.ConnectionComplete.audio_samples_per_frame
            const audioMapping = message.ConnectionComplete.audio_mapping

            await this.applyVideoSetup(formatRaw, width, height, fps, capabilities)

            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "connectionComplete", capabilities }
            })

            this.eventTarget.dispatchEvent(event)
            this.scheduleMouseWakePulse("connection-complete", 420)

            // HDR state will be set when server sends HdrModeUpdate message
            // Don't initialize from settings.hdr because that's just the user's preference,
            // not the actual HDR state (which depends on host support, display, and codec)
            if (this.settings.hdr) {
                this.debugLog("HDR requested by user, waiting for host confirmation...")
            }

            // we should allow streaming without audio
            if (!this.audioPlayer) {
                showErrorPopup("Failed to find supported audio player -> audio is missing.")
            }

            if (!this.videoRenderer || !this.audioPlayer) {
                throw "Video renderer or audio player not initialized!"
            }

            await this.audioPlayer.setup({
                sampleRate: audioSampleRate,
                channels: audioChannelCount,
                streams: audioStreams,
                coupledStreams: audioCoupledStreams,
                samplesPerFrame: audioSamplesPerFrame,
                mapping: audioMapping,
            })
        } else if ("ConnectionTerminated" in message) {
            const code = message.ConnectionTerminated.error_code

            this.debugLog(`ConnectionTerminated with code ${code}`, { type: "fatalDescription" })
        } else if ("DisplayModeApplied" in message) {
            const displayModeApplied = message.DisplayModeApplied
            if (displayModeApplied) {
                const { phase, width, height, fps, changed, skipped } = displayModeApplied
                this.debugLog(`Host display ${phase} applied ${width}x${height}@${fps} changed=${changed} skipped=${skipped}`)
                const event: InfoEvent = new CustomEvent("stream-info", {
                    detail: { type: "displayModeApplied", phase, width, height, fps, changed, skipped }
                })
                this.eventTarget.dispatchEvent(event)
            }
        } else if ("VideoFlowReady" in message) {
            const videoFlowReady = message.VideoFlowReady
            if (videoFlowReady) {
                const { phase, width, height, fps } = videoFlowReady
                this.debugLog(`Video flow ${phase} ready ${width}x${height}@${fps}`)
                if (phase == "start") {
                    this.scheduleMouseWakePulse("video-flow-ready", 140)
                }
                const event: InfoEvent = new CustomEvent("stream-info", {
                    detail: { type: "videoFlowReady", phase, width, height, fps }
                })
                this.eventTarget.dispatchEvent(event)
            }
        }
        // -- WebRTC Config
        else if ("Setup" in message) {
            const iceServers = message.Setup.ice_servers

            this.iceServers = iceServers
            const iceServerUrls = iceServers
                .map((server: { urls: string[] }) => server.urls)
                .reduce((list: string[], urls: string[]) => list.concat(urls), [])

            this.debugLog(`window.isSecureContext: ${window.isSecureContext}`)
            this.debugLog(`Using WebRTC Ice Servers: ${createPrettyList(iceServerUrls)}`)

            await this.startConnection()
        }
        // -- WebRTC
        else if ("WebRtc" in message) {
            const webrtcMessage = message.WebRtc
            if (this.transport instanceof WebRTCTransport) {
                this.transport.onReceiveMessage(webrtcMessage)
            } else {
                this.debugLog(`Received WebRTC message but transport is currently ${this.transport?.implementationName}`)
            }
        }
    }

    async startConnection() {
        const configuredTransport = this.getConfiguredTransport()
        this.debugLog(`Using transport: ${configuredTransport}`)

        const shutdownReason = configuredTransport == "websocket"
            ? await this.tryWebSocketTransport()
            : await this.tryConfiguredWebRTCTransport()

        if (!this.connectionCompleted && (shutdownReason == "failed" || shutdownReason == "failednoconnect")) {
            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "startupInterrupted", reason: shutdownReason }
            })
            this.eventTarget.dispatchEvent(event)
        }

        if (shutdownReason == "failednoconnect" && configuredTransport == "webrtc") {
            const icePlan = this.iceServers ? this.createWebRtcIcePlan(this.iceServers) : null
            const failedMessage = icePlan?.hasTurnServers
                ? "Failed to establish a WebRTC connection. Direct path was retried 3 times and TURN relay fallback also failed."
                : WEBSOCKET_RELAY_ENABLED
                    ? "Failed to establish a direct WebRTC connection. WebSocket relay fallback is disabled unless you explicitly choose it in the settings."
                    : "Failed to establish a direct WebRTC connection. This build is locked to WebRTC-only, and no TURN relay server is available for fallback."

            this.debugLog(failedMessage, { type: "fatalDescription" })
        } else if (shutdownReason == "failed" && configuredTransport == "webrtc") {
            this.debugLog("The active WebRTC connection was lost.", { type: "ifErrorDescription" })
        } else if (shutdownReason == "failed" && configuredTransport == "websocket") {
            this.debugLog("The active WebSocket relay connection was lost.", { type: "ifErrorDescription" })
        }
    }

    private getConfiguredTransport(): "webrtc" | "websocket" {
        if (!WEBSOCKET_RELAY_ENABLED && this.settings.dataTransport == "websocket") {
            this.debugLog("WebSocket relay setting detected, but this build is locked to WebRTC-only. Forcing WebRTC.")
            return "webrtc"
        }

        if (this.settings.dataTransport == "websocket") {
            return "websocket"
        }

        if (this.settings.dataTransport == "auto") {
            this.debugLog("Legacy transport setting \"auto\" detected. Using WebRTC-only to keep stream traffic off the web server path.")
        }

        return "webrtc"
    }

    private transport: Transport | null = null

    private setTransport(transport: Transport) {
        if (this.transport) {
            this.debugLog(`Replacing transport ${this.transport.implementationName} -> ${transport.implementationName}`)
            this.transport.close()
        }

        this.transport = transport

        this.input.setTransport(this.transport)
        this.stats.setTransport(this.transport)

        const rtt = this.transport.getChannel(TransportChannelId.RTT)
        if (rtt.type == "data") {
            rtt.addReceiveListener((data) => {
                const buffer = new ByteBuffer(data.byteLength)
                buffer.putU8Array(new Uint8Array(data))
                buffer.flip()

                const ty = buffer.getU8()
                if (ty == 0) {
                    rtt.send(data)
                }
            })
        } else {
            this.debugLog("Failed to get rtt as data transport channel. Cannot respond to rtt packets")
        }

        // Setup GENERAL channel listener for HDR mode updates
        const generalChannel = this.transport.getChannel(TransportChannelId.GENERAL)
        this.debugLog(`[GENERAL] Setting up GENERAL channel listener, type=${generalChannel.type}`)
        if (generalChannel.type === "data") {
            generalChannel.addReceiveListener((data: ArrayBuffer) => {
                this.onGeneralChannelMessage(data)
            })
            this.debugLog(`[GENERAL] GENERAL channel listener registered`)
        } else {
            this.debugLog(`[GENERAL] Cannot register listener, channel type is not 'data'`)
        }
    }

    private onGeneralChannelMessage(data: ArrayBuffer) {
        this.debugLog(`[GENERAL] Received message on GENERAL channel, size=${data.byteLength}`)
        const buffer = new Uint8Array(data)
        if (buffer.length < 2) {
            this.debugLog(`[GENERAL] Message too short: ${buffer.length} bytes`)
            return
        }

        const textLength = (buffer[0] << 8) | buffer[1]
        if (buffer.length < 2 + textLength) {
            this.debugLog(`[GENERAL] Message incomplete: expected ${2 + textLength} bytes, got ${buffer.length}`)
            return
        }

        const text = new TextDecoder().decode(buffer.slice(2, 2 + textLength))
        this.debugLog(`[GENERAL] Parsed message: ${text}`)
        try {
            const message: GeneralServerMessage = JSON.parse(text)
            this.handleGeneralMessage(message)
        } catch (err) {
            this.debugLog(`Failed to parse general message: ${err}`)
        }
    }

    private handleGeneralMessage(message: GeneralServerMessage) {
        if ("HdrModeUpdate" in message) {
            const hdrUpdate = message.HdrModeUpdate
            if (hdrUpdate) {
                const enabled = hdrUpdate.enabled
                this.debugLog(`HDR mode ${enabled ? "enabled" : "disabled"}`)
                this.setHdrMode(enabled)
            }
        } else if ("ClipboardData" in message) {
            const clipboard = message.ClipboardData?.payload
            if (clipboard?.text && navigator.clipboard && typeof navigator.clipboard.writeText == "function") {
                navigator.clipboard.writeText(clipboard.text).catch(err => {
                    this.debugLog(`[GENERAL] Failed to write host clipboard text to browser clipboard: ${err}`)
                })
            }
        } else if ("ClipboardText" in message) {
            const clipboardText = message.ClipboardText
            if (clipboardText?.text && navigator.clipboard && typeof navigator.clipboard.writeText == "function") {
                navigator.clipboard.writeText(clipboardText.text).catch(err => {
                    this.debugLog(`[GENERAL] Failed to write host clipboard text to browser clipboard: ${err}`)
                })
            }
        } else if ("VideoReconfigured" in message) {
            const videoReconfigured = message.VideoReconfigured
            if (videoReconfigured) {
                const { format, width, height, fps } = videoReconfigured
                this.debugLog(`Video reconfigured to ${width}x${height}@${fps}`)
                void this.applyVideoSetup(format, width, height, fps)
                this.clearRuntimeResizeAckTimer()
                this.runtimeResizeInFlight = false
                this.runtimeResizeRequestedSize = null
                const event: InfoEvent = new CustomEvent("stream-info", {
                    detail: { type: "videoReconfigured", width, height, fps }
                })
                this.eventTarget.dispatchEvent(event)
                this.flushQueuedRuntimeResize()
            }
        } else if ("ConnectionStatusUpdate" in message) {
            const statusUpdate = message.ConnectionStatusUpdate
            if (statusUpdate) {
                const status = statusUpdate.status
                const event: InfoEvent = new CustomEvent("stream-info", {
                    detail: { type: "connectionStatus", status }
                })
                this.eventTarget.dispatchEvent(event)
            }
        }
    }

    private setHdrMode(enabled: boolean) {
        this.stats.setHdrEnabled(enabled)
        if (this.videoRenderer) {
            if ("setHdrMode" in this.videoRenderer && typeof this.videoRenderer.setHdrMode === "function") {
                this.videoRenderer.setHdrMode(enabled)
            }
        }
    }

    private sendGeneralMessage(message: GeneralClientMessage): boolean {
        const general = this.transport?.getChannel(TransportChannelId.GENERAL)

        if (!general || general.type != "data") {
            return false
        }

        const text = JSON.stringify(message)

        const buffer = BIG_BUFFER
        buffer.reset()
        buffer.putU16(text.length)
        buffer.putUtf8Raw(text)
        buffer.flip()

        general.send(buffer.getRemainingBuffer().buffer)

        return true
    }

    private getIceServerUrls(server: RTCIceServer): Array<string> {
        if (Array.isArray(server.urls)) {
            return server.urls
        }
        if (typeof server.urls == "string") {
            return [server.urls]
        }

        return []
    }

    private isTurnIceUrl(url: string): boolean {
        return /^turns?:/i.test(url)
    }

    private createWebRtcIcePlan(iceServers: Array<RTCIceServer>): {
        directIceServers: Array<RTCIceServer>
        fullIceServers: Array<RTCIceServer>
        hasTurnServers: boolean
    } {
        const directIceServers: Array<RTCIceServer> = []
        let hasTurnServers = false

        for (const server of iceServers) {
            const urls = this.getIceServerUrls(server)
            if (urls.length == 0) {
                continue
            }

            const directUrls = urls.filter((url) => !this.isTurnIceUrl(url))
            if (directUrls.length != urls.length) {
                hasTurnServers = true
            }
            if (directUrls.length == 0) {
                continue
            }

            directIceServers.push({
                ...server,
                urls: directUrls
            })
        }

        return {
            directIceServers,
            fullIceServers: iceServers,
            hasTurnServers
        }
    }

    private async tryConfiguredWebRTCTransport(): Promise<TransportShutdown> {
        if (!this.iceServers) {
            this.debugLog(`Failed to try WebRTC Transport: no ice servers available`)
            return "failednoconnect"
        }

        const icePlan = this.createWebRtcIcePlan(this.iceServers)
        const maxDirectAttempts = icePlan.directIceServers.length > 0 ? 3 : 0

        if (maxDirectAttempts > 0) {
            const directUrls = icePlan.directIceServers
                .map((server) => this.getIceServerUrls(server))
                .reduce((list: Array<string>, urls) => list.concat(urls), [])

            this.debugLog(`WebRTC connection plan: try direct path up to ${maxDirectAttempts} times${icePlan.hasTurnServers ? ", then allow TURN relay fallback." : "."}`)
            this.debugLog(`Direct-only ICE servers for initial attempts: ${createPrettyList(directUrls)}`)
        }

        for (let attempt = 1; attempt <= maxDirectAttempts; attempt++) {
            this.debugLog(`${attempt == 1 ? "Trying" : "Retrying"} direct WebRTC path (attempt ${attempt}/${maxDirectAttempts})`)

            const result = await this.tryWebRTCTransport(icePlan.directIceServers)
            if (result != "failednoconnect") {
                return result
            }
        }

        if (icePlan.hasTurnServers) {
            this.debugLog(`Retrying WebRTC with TURN relay fallback after ${maxDirectAttempts} failed direct attempts.`)
            return await this.tryWebRTCTransport(icePlan.fullIceServers)
        }

        if (maxDirectAttempts == 0) {
            this.debugLog("No direct-only ICE servers were available. Trying the configured WebRTC ICE server list as-is.")
            return await this.tryWebRTCTransport(icePlan.fullIceServers)
        }

        return "failednoconnect"
    }

    private async tryWebRTCTransport(iceServers: Array<RTCIceServer>): Promise<TransportShutdown> {
        this.debugLog("Trying WebRTC transport")

        this.sendWsMessage({
            SetTransport: "WebRTC"
        })

        if (!iceServers || iceServers.length == 0) {
            this.debugLog(`Failed to try WebRTC Transport: no ice servers available`)
            return "failednoconnect"
        }

        const transport = new WebRTCTransport(this.logger)
        transport.onsendmessage = (message) => this.sendWsMessage({ WebRtc: message })

        await transport.initPeer({
            iceServers
        })
        this.setTransport(transport)

        // Wait for negotiation
        const result = await (new Promise<true | TransportShutdown>((resolve, _reject) => {
            transport.onconnect = () => resolve(true)
            transport.onclose = (shutdown) => {
                this.debugLog(`WebRTC transport closed during negotiation: ${shutdown}`)
                resolve(shutdown)
            }
        }))
        this.debugLog(`WebRTC negotiation result: ${result}`)

        if (result !== true) {
            return result
        }

        this.debugLog("Beginning pipeline creation after WebRTC connection")

        const pipesInfo = await gatherPipeInfo()
        this.debugLog(`Pipe inventory ready (${pipesInfo.size})`)

        const videoCodecSupport = await this.createPipelines()
        if (!videoCodecSupport) {
            this.debugLog("No video pipeline was found for the codec that was specified. If you're unsure which codecs are supported use H264.", { type: "fatalDescription" })

            this.debugLog("Closing WebRTC transport because video pipeline creation failed")
            await transport.close()
            return "failednoconnect"
        }

        await this.startStream(videoCodecSupport)

        return new Promise((resolve, reject) => {
            transport.onclose = (shutdown) => {
                this.debugLog(`WebRTC transport closed during active stream phase: ${shutdown}`)
                resolve(shutdown)
            }
        })
    }
    private async tryWebSocketTransport(): Promise<TransportShutdown> {
        this.debugLog("Trying Web Socket transport")

        this.sendWsMessage({
            SetTransport: "WebSocket"
        })

        const transport = new WebSocketTransport(this.ws, BIG_BUFFER, this.logger)

        this.setTransport(transport)

        const videoCodecSupport = await this.createPipelines()
        if (!videoCodecSupport) {
            this.debugLog("Failed to start stream because no video pipeline with support for the specified codec was found!", { type: "fatalDescription" })
            return "failednoconnect"
        }

        await this.startStream(videoCodecSupport)

        return new Promise((resolve, reject) => {
            transport.onclose = (shutdown) => {
                resolve(shutdown)
            }
        })
    }

    private async createPipelines(): Promise<VideoCodecSupport | null> {
        this.debugLog("Creating video and audio pipelines")
        const pipesInfo = await gatherPipeInfo()
        this.debugLog(`Pipeline capability snapshot ready (${pipesInfo.size})`)

        // Create pipelines
        const [supportedVideoCodecs] = await Promise.all([this.createVideoRenderer(), this.createAudioPlayer()])

        const videoPipelineName = `${this.transport?.getChannel(TransportChannelId.HOST_VIDEO).type} (transport) -> ${this.videoRenderer?.implementationName} (renderer)`
        this.debugLog(`Using video pipeline: ${videoPipelineName}`)

        const audioPipelineName = `${this.transport?.getChannel(TransportChannelId.HOST_AUDIO).type} (transport) -> ${this.audioPlayer?.implementationName} (player)`
        this.debugLog(`Using audio pipeline: ${audioPipelineName}`)

        this.stats.setVideoPipeline(videoPipelineName, this.videoRenderer)
        this.stats.setAudioPipeline(audioPipelineName, this.audioPlayer)

        this.debugLog("Video and audio pipelines are ready")

        return supportedVideoCodecs
    }
    private async createVideoRenderer(): Promise<VideoCodecSupport | null> {
        if (this.videoRenderer) {
            this.debugLog("Found an old video renderer -> cleaning it up")

            this.clearVideoTrackBinding()
            this.videoRenderer.unmount(this.divElement)
            this.videoRenderer.cleanup()
            this.videoRenderer = null
        }
        if (!this.transport) {
            this.debugLog("Failed to setup video without transport")
            return null
        }

        const codecHint = getVideoCodecHint(this.settings)
        if (!hasAnyCodec(codecHint)) {
            this.debugLog("Couldn't find any supported video format. Change the codec option to H264 in the settings if you're unsure which codecs are supported.", { type: "fatalDescription" })
            return null
        }

        const transportCodecSupport = await this.transport.setupHostVideo({
            type: ["videotrack", "data"]
        })
        this.debugLog(`Transport supports these video codecs: ${JSON.stringify(transportCodecSupport)}`)

        const video = this.transport.getChannel(TransportChannelId.HOST_VIDEO)
        if (video.type != "videotrack" && video.type != "data") {
            this.debugLog(`Failed to create video pipeline with transport channel of type ${video.type} (${this.transport.implementationName})`)
            return null
        }
        const videoSettings = this.getVideoPipelineOptions(video.type, transportCodecSupport)

        let pipelineCodecSupport: VideoCodecSupport | null = null
        if (video.type == "videotrack") {
            const { videoRenderer, supportedCodecs, error } = await buildVideoPipeline("videotrack", videoSettings, this.logger)

            if (error) {
                return null
            }
            pipelineCodecSupport = supportedCodecs

            videoRenderer.mount(this.divElement)
            this.bindTrackVideoRenderer(video, videoRenderer)

            this.videoRenderer = videoRenderer
        } else if (video.type == "data") {
            const { videoRenderer, supportedCodecs, error } = await buildVideoPipeline("data", videoSettings, this.logger)

            if (error) {
                return null
            }
            pipelineCodecSupport = supportedCodecs

            videoRenderer.mount(this.divElement)

            video.addReceiveListener((data) => {
                videoRenderer.submitPacket(data)

                // data pipeline support requesting idrs over video channel
                if (videoRenderer.pollRequestIdr()) {
                    const buffer = new ByteBuffer(1)

                    buffer.putU8(0)

                    buffer.flip()

                    video.send(buffer.getRemainingBuffer().buffer)
                }
            })

            this.videoRenderer = videoRenderer
        }

        if (!pipelineCodecSupport) {
            return null
        }
        this.negotiatedVideoCodecSupport = pipelineCodecSupport
        this.debugLog(`Video pipeline ready via ${video.type}`)
        return pipelineCodecSupport
    }
    private async createAudioPlayer(): Promise<boolean> {
        if (this.audioPlayer) {
            this.debugLog("Found an old audio player -> cleaning it up")

            this.audioPlayer.unmount(this.divElement)
            this.audioPlayer.cleanup()
            this.audioPlayer = null
        }
        if (!this.transport) {
            this.debugLog("Failed to setup audio without transport")
            return false
        }

        this.transport.setupHostAudio({
            type: ["audiotrack", "data"]
        })

        const audio = this.transport?.getChannel(TransportChannelId.HOST_AUDIO)
        if (audio.type == "audiotrack") {
            const { audioPlayer, error } = await buildAudioPipeline("audiotrack", this.settings, this.logger)

            if (error) {
                return false
            }

            audioPlayer.mount(this.divElement)

            audio.addTrackListener((track) => audioPlayer.setTrack(track))

            this.audioPlayer = audioPlayer
        } else if (audio.type == "data") {
            const { audioPlayer, error } = await buildAudioPipeline("data", this.settings, this.logger)

            if (error) {
                return false
            }

            audioPlayer.mount(this.divElement)

            audio.addReceiveListener((data) => {
                audioPlayer.submitPacket(data)
            })

            this.audioPlayer = audioPlayer
        } else {
            this.debugLog(`Cannot find audio pipeline for transport type "${audio.type}"`)
            return false
        }

        this.debugLog(`Audio pipeline ready via ${audio.type}`)
        return true
    }
    private buildStartStreamMessage(videoCodecSupport: VideoCodecSupport, settings: Settings = this.settings): StreamClientMessage {
        const packetSize = getSafeStartPacketSize(settings)
        const hostMouseEmulation = this.getHostMouseEmulationMode()
        return {
            StartStream: {
                bitrate: settings.bitrate,
                packet_size: packetSize,
                fps: settings.fps,
                width: this.streamerSize[0],
                height: this.streamerSize[1],
                adaptive_bitrate: settings.adaptiveBitrate !== false,
                adaptive_fps: settings.adaptiveBitrate !== false,
                host_mouse_emulation: hostMouseEmulation,
                play_audio_local: settings.playAudioLocal,
                video_supported_formats: createSupportedVideoFormatsBits(videoCodecSupport),
                video_colorspace: "Rec709",
                video_color_range_full: false,
                hdr: settings.hdr ?? false,
            }
        }
    }

    private sendStartStreamRequest(videoCodecSupport: VideoCodecSupport, reason: string, settings: Settings = this.settings) {
        const packetSize = getSafeStartPacketSize(settings)
        const hostMouseEmulation = this.getHostMouseEmulationMode()
        const message = this.buildStartStreamMessage(videoCodecSupport, settings)
        if (packetSize != settings.packetSize) {
            this.logClientEvent(`Capped packet size ${settings.packetSize} -> ${packetSize} for safer WebRTC streaming`)
        }
        this.logClientEvent(`${reason} with info: ${JSON.stringify(message)}`)
        this.logClientEvent(`Stream video codec info: ${JSON.stringify(videoCodecSupport)}`)

        // Log HDR requirements if HDR is requested
        if (settings.hdr) {
            const hasHdrCodec = videoCodecSupport.H265_MAIN10 || videoCodecSupport.AV1_MAIN10
            if (!hasHdrCodec) {
                this.logClientEvent(`Warning: HDR requested but no 10-bit codec available. HDR requires H265_MAIN10 or AV1_MAIN10 support.`)
            } else {
                this.logClientEvent(`HDR codec available: H265_MAIN10=${videoCodecSupport.H265_MAIN10}, AV1_MAIN10=${videoCodecSupport.AV1_MAIN10}`)
            }
        }

        this.logClientEvent(`Sending StartStream over websocket (${this.getLifecycleSnapshot()})`)
        this.connectionCompleted = false
        this.activeHostMouseEmulationMode = hostMouseEmulation
        this.armStartStreamGuardTimer()
        this.sendWsMessage(message)
    }

    private async startStream(videoCodecSupport: VideoCodecSupport): Promise<void> {
        this.negotiatedVideoCodecSupport = videoCodecSupport
        this.sendStartStreamRequest(videoCodecSupport, "Starting stream", this.settings)
    }

    async restartHostStreamInPlace(nextSettings: Partial<Settings>, reason: string): Promise<boolean> {
        if (this.ws.readyState != WebSocket.OPEN) {
            this.logClientEvent(`Skipping in-place host stream restart because websocket is not open (${this.getLifecycleSnapshot()})`)
            return false
        }

        const negotiatedVideoCodecSupport = this.negotiatedVideoCodecSupport
        if (!hasAnyCodec(negotiatedVideoCodecSupport)) {
            this.logClientEvent(`Skipping in-place host stream restart because negotiated codec support is unavailable (${this.getLifecycleSnapshot()})`)
            return false
        }

        Object.assign(this.settings, nextSettings)
        this.clearRuntimeResizeAckTimer()
        this.runtimeResizeInFlight = false
        this.runtimeResizeRequestedSize = null
        this.queuedRuntimeResize = null
        this.logClientEvent(`Restarting host stream in place reason=${reason} bitrate=${this.settings.bitrate} fps=${this.settings.fps} size=${this.streamerSize[0]}x${this.streamerSize[1]}`)
        this.sendStartStreamRequest(negotiatedVideoCodecSupport, "Restarting host stream in place", this.settings)
        return true
    }

    private getHostMouseEmulationMode(): HostMouseEmulationMode {
        const inputConfig = this.input.getConfig()
        const isRelativeMouse = inputConfig.mouseMode == "relative"

        return isRelativeMouse ? "relativeNative" : "absoluteFollow"
    }

    syncHostMouseEmulationMode(force: boolean = false) {
        if (!this.ws || this.ws.readyState != WebSocket.OPEN) {
            return
        }

        const nextMode = this.getHostMouseEmulationMode()
        if (!force && this.activeHostMouseEmulationMode == nextMode) {
            return
        }

        this.activeHostMouseEmulationMode = nextMode
        this.logClientEvent(`Updating host mouse emulation to ${nextMode} (${this.getLifecycleSnapshot()})`)
        const message: StreamClientMessage = {
            SetHostMouseEmulation: {
                host_mouse_emulation: nextMode,
            }
        }
        this.sendWsMessage(message)
    }

    async requestRuntimeResize(viewerScreenSize: [number, number], fps: number = this.settings.fps): Promise<boolean> {
        if (!this.capabilities) {
            return false
        }

        const [width, height] = viewerScreenSize
        if (width <= 0 || height <= 0) {
            return false
        }

        const requested = { width, height, fps }

        if (this.streamerSize[0] == width && this.streamerSize[1] == height) {
            return true
        }

        if (this.runtimeResizeInFlight) {
            if (this.isSameRuntimeResizeTarget(this.runtimeResizeRequestedSize, requested)
                || this.isSameRuntimeResizeTarget(this.queuedRuntimeResize, requested)) {
                this.logClientEvent(`Ignoring duplicate runtime resize request for ${width}x${height}@${fps}`)
                return true
            }

            this.queuedRuntimeResize = requested
            return true
        }

        this.runtimeResizeInFlight = true
        this.runtimeResizeRequestedSize = requested
        this.queuedRuntimeResize = null
        this.clearRuntimeResizeAckTimer()
        this.runtimeResizeAckTimer = window.setTimeout(() => {
            this.runtimeResizeAckTimer = null
            this.runtimeResizeInFlight = false
            this.runtimeResizeRequestedSize = null
            this.logClientEvent(`Runtime resize ack timed out for ${width}x${height}@${fps}`)
            if (this.queuedRuntimeResize) {
                this.flushQueuedRuntimeResize()
                return
            }

            const event: InfoEvent = new CustomEvent("stream-info", {
                detail: { type: "runtimeResizeTimeout", width, height, fps }
            })
            this.eventTarget.dispatchEvent(event)
        }, 12000)

        this.logClientEvent(`Requesting runtime resize to ${width}x${height}@${fps}`)
        this.sendWsMessage({
            ResizeStream: {
                width,
                height,
                fps,
            }
        })

        return true
    }

    async requestHostOnlyDisplayResize(viewerScreenSize: [number, number], fps: number = this.settings.fps): Promise<boolean> {
        if (!this.capabilities) {
            return false
        }

        const [width, height] = viewerScreenSize
        if (width <= 0 || height <= 0) {
            return false
        }

        this.logClientEvent(`Requesting host-only display resize to ${width}x${height}@${fps}`)
        this.sendWsMessage({
            ResizeStream: {
                width,
                height,
                fps,
            }
        })

        return true
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.divElement)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.divElement)
    }

    getVideoRenderer(): VideoRenderer | null {
        return this.videoRenderer
    }
    getAudioPlayer(): AudioPlayer | null {
        return this.audioPlayer
    }

    async setRuntimeMicrophoneEnabled(enabled: boolean): Promise<TransportMicrophoneSetResult> {
        if (!this.transport) {
            return {
                supported: !!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function"),
                enabled: false,
                reason: "no_transport"
            }
        }

        return await this.transport.setMicrophoneEnabled(enabled)
    }

    getRuntimeMicrophoneState(): TransportMicrophoneState {
        if (!this.transport) {
            return {
                supported: !!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function"),
                enabled: false,
                attached: false,
                uplinkNegotiated: null,
                direction: "unknown",
                selectedDeviceId: "default",
                level: 0,
                outbound: {
                    supported: false,
                    timestampMs: Date.now(),
                    bitrateKbps: null,
                    packetsSent: null,
                    bytesSent: null
                }
            }
        }

        return this.transport.getMicrophoneState()
    }

    setRuntimeMicrophoneDevice(deviceId: string): TransportMicrophoneDeviceResult {
        if (!this.transport) {
            return {
                ok: false,
                reason: "no_transport"
            }
        }

        return this.transport.setMicrophoneDeviceId(deviceId)
    }

    getRuntimeMicrophoneDevice(): string {
        return this.transport?.getMicrophoneDeviceId() ?? "default"
    }

    async listRuntimeMicrophoneDevices(): Promise<Array<TransportMicrophoneDevice>> {
        if (!this.transport) {
            return []
        }

        return await this.transport.listMicrophoneDevices()
    }

    async getRuntimeMicrophoneDiagnostics(): Promise<TransportMicrophoneDiagnostics> {
        if (!this.transport) {
            return {
                selectedDeviceId: "default",
                level: 0,
                outbound: {
                    supported: false,
                    timestampMs: Date.now(),
                    bitrateKbps: null,
                    packetsSent: null,
                    bytesSent: null
                }
            }
        }

        return await this.transport.getMicrophoneDiagnostics()
    }

    // -- Raw Web Socket stuff
    private wsSendBuffer: Array<string> = []
    private lastRouteTelemetrySignature = ""

    private onWsOpen() {
        this.logClientEvent(`Web Socket Open (${this.getLifecycleSnapshot()})`)

        for (const raw of this.wsSendBuffer.splice(0)) {
            this.ws.send(raw)
        }

        this.startWsHeartbeat()
    }
    private onWsClose(event: CloseEvent) {
        this.logClientEvent(
            `Web Socket Closed code=${event.code} clean=${event.wasClean ? "yes" : "no"} reason=${event.reason || "none"} pending=${this.pendingWsCloseReason ?? "none"} (${this.getLifecycleSnapshot()})`
        )
        this.pendingWsCloseReason = null
        this.clearRuntimeResizeAckTimer()
        this.clearStartStreamGuardTimer()
        this.clearWsHeartbeatTimer()
        this.clearMouseWakeTimer()
        this.activeHostMouseEmulationMode = null
        this.runtimeResizeInFlight = false
        this.queuedRuntimeResize = null
    }
    private onError(event: Event) {
        this.logClientEvent(`Web Socket or WebRtcPeer Error (${this.getLifecycleSnapshot()})`)

        console.error(`Web Socket or WebRtcPeer Error`, event)
    }

    private sendWsMessage(message: StreamClientMessage) {
        const raw = JSON.stringify(message)
        const messageType = Object.keys(message)[0] ?? "unknown"
        if (this.ws.readyState == WebSocket.OPEN) {
            this.logClientEvent(`WS send ${messageType}`)
            this.ws.send(raw)
        } else {
            this.logClientEvent(`WS queue ${messageType} because readyState=${this.ws.readyState}`)
            this.wsSendBuffer.push(raw)
        }
    }

    reportRouteTelemetry(route: string, detail: string) {
        const normalizedRoute = String(route ?? "").trim() || "unknown"
        const normalizedDetail = String(detail ?? "").trim()
        const signature = `${normalizedRoute}::${normalizedDetail}`
        if (signature == this.lastRouteTelemetrySignature) {
            return
        }

        this.lastRouteTelemetrySignature = signature
        this.sendWsMessage({
            RouteTelemetry: {
                route: normalizedRoute,
                detail: normalizedDetail,
            }
        })
    }

    private onRawWsMessage(event: MessageEvent) {
        const message = event.data
        if (typeof message == "string") {
            const json = JSON.parse(message)

            this.onMessage(json)
        }
    }

    stop(): Promise<boolean> {
        this.clearRuntimeResizeAckTimer()
        this.clearStartStreamGuardTimer()
        this.runtimeResizeInFlight = false
        this.queuedRuntimeResize = null
        this.logClientEvent("Sending Stop over general channel")
        if (!this.sendGeneralMessage("Stop")) {
            this.logClientEvent("Stop request skipped because GENERAL channel is unavailable")
            return Promise.resolve(false)
        }

        // Wait for the message to get sent
        return new Promise((resolve, _reject) => {
            setTimeout(() => resolve(true), 100)
        })
    }

    requestIdrFrame(): boolean {
        this.logClientEvent("Requesting IDR frame over general channel")
        if (!this.sendGeneralMessage("RequestIdr")) {
            this.logClientEvent("IDR request skipped because GENERAL channel is unavailable")
            return false
        }

        return true
    }

    async recoverVideoRendererInPlace(reason: string): Promise<boolean> {
        if (!this.transport || !this.videoRenderer) {
            this.logClientEvent(`Skipping in-place video renderer recovery reason=${reason} missing=${!this.transport ? "transport" : "renderer"}`)
            return false
        }

        const videoChannel = this.transport.getChannel(TransportChannelId.HOST_VIDEO)
        if (videoChannel.type != "videotrack") {
            this.logClientEvent(`Skipping in-place video renderer recovery reason=${reason} channel=${videoChannel.type}`)
            return false
        }

        const lastVideoSetup = this.lastVideoSetup
        if (!lastVideoSetup) {
            this.logClientEvent(`Skipping in-place video renderer recovery reason=${reason} missing=video-setup`)
            return false
        }

        const recoveryCodecSupport = hasAnyCodec(this.negotiatedVideoCodecSupport)
            ? this.negotiatedVideoCodecSupport
            : await this.transport.setupHostVideo({ type: ["videotrack"] })
        const videoSettings = this.getVideoPipelineOptions("videotrack", recoveryCodecSupport)
        const { videoRenderer, supportedCodecs, error } = await buildVideoPipeline("videotrack", videoSettings, this.logger)
        if (error) {
            this.logClientEvent(`Video renderer in-place recovery failed reason=${reason} step=build`)
            return false
        }

        const previousRenderer = this.videoRenderer as TrackVideoRenderer & VideoRenderer
        try {
            videoRenderer.mount(this.divElement)
            const setupApplied = await this.setupVideoRenderer(
                videoRenderer,
                lastVideoSetup.formatRaw,
                lastVideoSetup.width,
                lastVideoSetup.height,
                lastVideoSetup.fps
            )
            if (!setupApplied) {
                throw new Error("video_setup_failed")
            }

            this.bindTrackVideoRenderer(videoChannel, videoRenderer)
            this.videoRenderer = videoRenderer
            this.negotiatedVideoCodecSupport = supportedCodecs
            const videoPipelineName = `${videoChannel.type} (transport) -> ${this.videoRenderer.implementationName} (renderer)`
            this.stats.setVideoPipeline(videoPipelineName, this.videoRenderer)
            this.requestIdrFrame()
            this.videoRenderer.onUserInteraction()
            this.logClientEvent(`Recovered video renderer in place reason=${reason} pipeline=${videoPipelineName}`)

            try {
                previousRenderer.unmount(this.divElement)
            } catch {
                // Ignore unmount failures after the replacement renderer is already live.
            }
            try {
                previousRenderer.cleanup()
            } catch {
                // Ignore cleanup failures after the replacement renderer is already live.
            }

            return true
        } catch (error) {
            try {
                videoRenderer.unmount(this.divElement)
            } catch {
                // ignore cleanup failures when renderer recovery fails
            }
            try {
                videoRenderer.cleanup()
            } catch {
                // ignore cleanup failures when renderer recovery fails
            }
            this.videoRenderer = previousRenderer
            this.bindTrackVideoRenderer(videoChannel, previousRenderer)
            this.logClientEvent(`Video renderer in-place recovery failed reason=${reason} step=apply err=${String(error)}`)
            return false
        }
    }

    async close(reason = "unspecified"): Promise<void> {
        const closeReason = this.formatCloseReason(reason)
        this.pendingWsCloseReason = closeReason
        this.input.onStreamStop()
        this.clearRuntimeResizeAckTimer()
        this.clearStartStreamGuardTimer()
        this.clearWsHeartbeatTimer()
        this.clearMouseWakeTimer()
        this.runtimeResizeInFlight = false
        this.queuedRuntimeResize = null

        this.logClientEvent(`Closing stream control channel reason=${closeReason} (${this.getLifecycleSnapshot()})`)

        if (this.transport) {
            try {
                await this.transport.close()
            } catch (err) {
                this.logClientEvent(`Transport close failed during stream close: ${String(err)}`)
            }
        }

        if (this.ws.readyState == WebSocket.OPEN || this.ws.readyState == WebSocket.CONNECTING) {
            try {
                this.ws.close(1000, closeReason)
            } catch (err) {
                this.logClientEvent(`Web Socket close failed: ${String(err)}`)
            }
        } else {
            this.logClientEvent(`Web Socket was already closed before explicit close (${this.getLifecycleSnapshot()})`)
            this.pendingWsCloseReason = null
        }
    }

    // -- Class Api
    addInfoListener(listener: InfoEventListener) {
        this.eventTarget.addEventListener("stream-info", listener as EventListenerOrEventListenerObject)
    }
    removeInfoListener(listener: InfoEventListener) {
        this.eventTarget.removeEventListener("stream-info", listener as EventListenerOrEventListenerObject)
    }

    getInput(): StreamInput {
        return this.input
    }
    getStats(): StreamStats {
        return this.stats
    }

    async captureInboundVideoActivitySnapshot(): Promise<TransportInboundVideoActivitySnapshot | null> {
        if (!this.transport) {
            return null
        }

        return await this.transport.captureInboundVideoActivitySnapshot()
    }

    async hasInboundVideoActivitySince(snapshot: TransportInboundVideoActivitySnapshot | null): Promise<boolean> {
        if (!this.transport) {
            return false
        }

        return await this.transport.hasInboundVideoActivitySince(snapshot)
    }

    getStreamerSize(): [number, number] {
        return this.streamerSize
    }

    getPreferredRuntimeStreamerSize(): [number, number] | null {
        return this.preferredRuntimeStreamerSize
    }

    usedBootstrapStartupProfile(): boolean {
        return this.usedBootstrapDeviceMatchStartup
    }

    isReadyForRuntimeResize(): boolean {
        return this.capabilities != null && this.connectionCompleted
    }
}

function createPrettyList(list: Array<string>): string {
    let isFirst = true
    let text = "["
    for (const item of list) {
        if (!isFirst) {
            text += ", "
        }
        isFirst = false

        text += item
    }
    text += "]"

    return text
}
