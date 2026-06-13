import { LogMessageType, MicSidecarClientMessage, MicSidecarServerMessage, StreamSignalingMessage } from "../api_bindings.js"
import {
    TransportMicrophoneDevice,
    TransportMicrophoneDeviceResult,
    TransportMicrophoneDiagnostics,
    TransportMicrophoneOutboundStats,
    TransportMicrophoneRouteDiagnostics,
    TransportMicrophoneSetResult,
    TransportMicrophoneState
} from "./transport/index.js"

type MicSidecarLogListener = (message: string, type?: LogMessageType | null) => void

type RTCStatsLike = RTCStats & Record<string, unknown>

function getStatString(stat: RTCStatsLike | null | undefined, key: string): string | null {
    if (!stat || !(key in stat)) {
        return null
    }

    const value = stat[key]
    return typeof value == "string" ? value : null
}

function getStatNumber(stat: RTCStatsLike | null | undefined, key: string): number | null {
    if (!stat || !(key in stat)) {
        return null
    }

    const value = stat[key]
    return typeof value == "number" ? value : null
}

function getStatBool(stat: RTCStatsLike | null | undefined, key: string): boolean | null {
    if (!stat || !(key in stat)) {
        return null
    }

    const value = stat[key]
    return typeof value == "boolean" ? value : null
}

function inferCandidateAddressFamily(candidate: RTCStatsLike | null | undefined): string | null {
    const address = getStatString(candidate ?? null, "address") ?? getStatString(candidate ?? null, "ip")
    if (!address) {
        return null
    }

    if (address.includes(":")) {
        return "v6"
    }
    if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(address)) {
        return "v4"
    }

    return null
}

const DEFAULT_OUTBOUND: TransportMicrophoneOutboundStats = {
    supported: false,
    timestampMs: 0,
    bitrateKbps: null,
    packetsSent: null,
    bytesSent: null
}

const MIC_SIDECAR_TARGET_BITRATE_BPS = 16_000
export const MIC_SIDECAR_TARGET_BITRATE_KBPS = MIC_SIDECAR_TARGET_BITRATE_BPS / 1000
const MIC_SIDECAR_TARGET_SAMPLE_RATE = 16_000
const MIC_SIDECAR_OPUS_FMTP = [
    "ptime=40",
    "minptime=40",
    "useinbandfec=0",
    "stereo=0",
    "sprop-stereo=0",
    `maxaveragebitrate=${MIC_SIDECAR_TARGET_BITRATE_BPS}`,
    "usedtx=1"
].join(";")

function mergeFmtpLine(base: string | null | undefined, extras: Array<string>): string {
    const values = new Map<string, string>()
    for (const part of String(base || "").split(";")) {
        const trimmed = part.trim()
        if (!trimmed) {
            continue
        }
        const [keyRaw, valueRaw] = trimmed.split("=", 2)
        const key = String(keyRaw || "").trim().toLowerCase()
        if (!key) {
            continue
        }
        values.set(key, valueRaw == null ? "" : String(valueRaw).trim())
    }
    for (const part of extras) {
        const trimmed = part.trim()
        if (!trimmed) {
            continue
        }
        const [keyRaw, valueRaw] = trimmed.split("=", 2)
        const key = String(keyRaw || "").trim().toLowerCase()
        if (!key) {
            continue
        }
        values.set(key, valueRaw == null ? "" : String(valueRaw).trim())
    }
    return Array.from(values.entries())
        .map(([key, value]) => value ? `${key}=${value}` : key)
        .join(";")
}

export class MicSidecarClient {
    private readonly signalingUrl: string
    private readonly hostId: number
    private readonly onStateChange: (() => void) | null
    private readonly onLog: MicSidecarLogListener | null

    private ws: WebSocket | null = null
    private peer: RTCPeerConnection | null = null
    private localStream: MediaStream | null = null
    private localTrack: MediaStreamTrack | null = null
    private sender: RTCRtpSender | null = null
    private transceiver: RTCRtpTransceiver | null = null
    private pendingRemoteIceCandidates: RTCIceCandidateInit[] = []
    private selectedDeviceId = "default"
    private enabled = false
    private expectedEnabled = false
    private connecting = false
    private setupComplete = false
    private sessionSerial = 0
    private currentDirection = "unknown"
    private microphoneLevelValue = 0
    private microphoneLevelContext: AudioContext | null = null
    private microphoneLevelSource: MediaStreamAudioSourceNode | null = null
    private microphoneLevelAnalyser: AnalyserNode | null = null
    private microphoneLevelData: Uint8Array | null = null
    private lastOutboundSnapshot: { timestampMs: number, bytesSent: number | null } | null = null
    private lastOutboundStats: TransportMicrophoneOutboundStats = { ...DEFAULT_OUTBOUND }
    private lastRouteDiagnostics: TransportMicrophoneRouteDiagnostics | null = null
    private lastRouteLogSignature = ""

    constructor(apiHostUrl: string, hostId: number, onStateChange?: () => void, onLog?: MicSidecarLogListener) {
        this.signalingUrl = `${apiHostUrl.replace(/^http(s)?:/, "ws$1:")}/host/mic`
        this.hostId = hostId
        this.onStateChange = onStateChange ?? null
        this.onLog = onLog ?? null
    }

    private notifyStateChanged() {
        this.onStateChange?.()
    }

    private log(message: string, type?: LogMessageType | null) {
        this.onLog?.(message, type ?? null)
        console.info(`[Mic Sidecar] ${message}`)
    }

    private stopLevelMeter() {
        if (this.microphoneLevelSource) {
            try {
                this.microphoneLevelSource.disconnect()
            } catch {
                // ignore
            }
        }
        if (this.microphoneLevelAnalyser) {
            try {
                this.microphoneLevelAnalyser.disconnect()
            } catch {
                // ignore
            }
        }
        if (this.microphoneLevelContext) {
            try {
                void this.microphoneLevelContext.close()
            } catch {
                // ignore
            }
        }

        this.microphoneLevelSource = null
        this.microphoneLevelAnalyser = null
        this.microphoneLevelData = null
        this.microphoneLevelContext = null
        this.microphoneLevelValue = 0
    }

    private startLevelMeter(stream: MediaStream) {
        this.stopLevelMeter()

        try {
            const audioContextWindow = window as Window & { webkitAudioContext?: typeof AudioContext }
            const AudioContextClass = window.AudioContext ?? audioContextWindow.webkitAudioContext
            if (!AudioContextClass) {
                return
            }

            const context = new AudioContextClass()
            if (typeof context.resume == "function" && context.state == "suspended") {
                void context.resume().catch(() => undefined)
            }

            const source = context.createMediaStreamSource(stream)
            const analyser = context.createAnalyser()
            analyser.fftSize = 1024
            analyser.smoothingTimeConstant = 0.85
            source.connect(analyser)

            this.microphoneLevelContext = context
            this.microphoneLevelSource = source
            this.microphoneLevelAnalyser = analyser
            this.microphoneLevelData = new Uint8Array(analyser.fftSize)
        } catch {
            this.stopLevelMeter()
        }
    }

    private updateLevelEstimate(): number {
        if (!this.microphoneLevelAnalyser || !this.microphoneLevelData) {
            this.microphoneLevelValue = 0
            return 0
        }

        try {
            this.microphoneLevelAnalyser.getByteTimeDomainData(this.microphoneLevelData)
            let sum = 0
            for (let i = 0; i < this.microphoneLevelData.length; i += 1) {
                const normalized = (this.microphoneLevelData[i] - 128) / 128
                sum += normalized * normalized
            }

            const rms = Math.sqrt(sum / this.microphoneLevelData.length)
            this.microphoneLevelValue = Math.max(0, Math.min(1, rms / 0.35))
            return this.microphoneLevelValue
        } catch {
            this.microphoneLevelValue = 0
            return 0
        }
    }

    private releaseLocalTrack() {
        this.stopLevelMeter()

        if (this.localTrack) {
            try {
                this.localTrack.enabled = false
            } catch {
                // ignore
            }
            try {
                this.localTrack.stop()
            } catch {
                // ignore
            }
        }
        if (this.localStream) {
            try {
                for (const track of this.localStream.getTracks()) {
                    try {
                        track.stop()
                    } catch {
                        // ignore
                    }
                }
            } catch {
                // ignore
            }
        }

        this.localTrack = null
        this.localStream = null
    }

    private resetPeerState() {
        this.peer = null
        this.sender = null
        this.transceiver = null
        this.pendingRemoteIceCandidates.length = 0
        this.setupComplete = false
        this.currentDirection = "unknown"
        this.lastOutboundSnapshot = null
        this.lastOutboundStats = { ...DEFAULT_OUTBOUND, timestampMs: Date.now() }
    }

    private attachTrackLifecycle(track: MediaStreamTrack) {
        const markInactive = () => {
            if (this.localTrack !== track) {
                return
            }

            this.enabled = false
            this.expectedEnabled = false
            this.notifyStateChanged()
        }

        track.addEventListener("ended", markInactive)
        track.addEventListener("mute", () => {
            this.notifyStateChanged()
        })
        track.addEventListener("unmute", () => {
            this.notifyStateChanged()
        })
    }

    setDeviceId(deviceId: string): TransportMicrophoneDeviceResult {
        this.selectedDeviceId = String(deviceId || "default").trim() || "default"
        return {
            ok: true,
            deviceId: this.selectedDeviceId
        }
    }

    getDeviceId(): string {
        return this.selectedDeviceId
    }

    async listDevices(): Promise<Array<TransportMicrophoneDevice>> {
        if (!(navigator.mediaDevices && typeof navigator.mediaDevices.enumerateDevices == "function")) {
            return []
        }

        try {
            const raw = await navigator.mediaDevices.enumerateDevices()
            const microphones = raw
                .filter((device) => device && device.kind == "audioinput")
                .map((device, index) => ({
                    deviceId: String(device.deviceId || ""),
                    label: String(device.label || `Microphone ${index + 1}`),
                    groupId: String(device.groupId || "")
                }))

            if (!microphones.some((device) => device.deviceId == "default")) {
                microphones.unshift({
                    deviceId: "default",
                    label: "Default microphone",
                    groupId: ""
                })
            }

            return microphones
        } catch {
            return []
        }
    }

    private async getOutboundStats(): Promise<TransportMicrophoneOutboundStats> {
        const fallback: TransportMicrophoneOutboundStats = {
            ...DEFAULT_OUTBOUND,
            timestampMs: Date.now()
        }
        const sender = this.sender
        if (!sender || typeof sender.getStats != "function") {
            this.lastOutboundStats = fallback
            return fallback
        }

        try {
            const statsReport = await sender.getStats()
            let outbound: RTCStatsLike | null = null
            for (const report of statsReport.values()) {
                const isOutboundAudio = report
                    && report.type == "outbound-rtp"
                    && !("isRemote" in report && report.isRemote)
                    && (
                        getStatString(report, "kind") == "audio"
                        || getStatString(report, "mediaType") == "audio"
                    )
                if (isOutboundAudio) {
                    outbound = report
                    break
                }
            }

            if (!outbound) {
                this.lastOutboundStats = fallback
                return fallback
            }

            const timestampMs = getStatNumber(outbound, "timestamp") ?? Date.now()
            const packetsSent = getStatNumber(outbound, "packetsSent")
            const bytesSent = getStatNumber(outbound, "bytesSent")
            let bitrateKbps: number | null = null
            if (
                bytesSent != null
                && this.lastOutboundSnapshot
                && this.lastOutboundSnapshot.bytesSent != null
                && timestampMs > this.lastOutboundSnapshot.timestampMs
            ) {
                const deltaBytes = bytesSent - this.lastOutboundSnapshot.bytesSent
                const deltaMs = timestampMs - this.lastOutboundSnapshot.timestampMs
                if (deltaBytes >= 0 && deltaMs > 0) {
                    bitrateKbps = (deltaBytes * 8) / deltaMs
                }
            }

            this.lastOutboundSnapshot = {
                timestampMs,
                bytesSent
            }

            const result: TransportMicrophoneOutboundStats = {
                supported: true,
                timestampMs,
                bitrateKbps,
                packetsSent,
                bytesSent
            }
            this.lastOutboundStats = result
            return result
        } catch {
            this.lastOutboundStats = fallback
            return fallback
        }
    }

    private buildRoutePathSummary(route: TransportMicrophoneRouteDiagnostics): string | null {
        const buildSide = (type: string | null, protocol: string | null, family: string | null) => {
            if (!type && !protocol) {
                return null
            }

            const protocolLabel = protocol
                ? `${protocol}${family == "v4" || family == "v6" ? family : ""}`
                : null
            return [type, protocolLabel].filter((value): value is string => !!value).join("/")
        }

        const local = buildSide(route.localCandidateType, route.localProtocol, route.localAddressFamily)
        const remote = buildSide(route.remoteCandidateType, route.remoteProtocol, route.remoteAddressFamily)
        const parts: string[] = []
        if (local && remote) {
            parts.push(`${local} -> ${remote}`)
        } else if (local || remote) {
            parts.push(local ?? remote ?? "--")
        }
        if (route.relayProtocol) {
            parts.push(`relay ${route.relayProtocol}`)
        }
        if (route.selectedPairState) {
            parts.push(`pair ${route.selectedPairState}`)
        }

        return parts.length > 0 ? parts.join(" / ") : null
    }

    private findSelectedCandidatePair(stats: RTCStatsReport): RTCStatsLike | null {
        for (const value of stats.values()) {
            const stat = value as RTCStatsLike
            if (stat.type != "transport") {
                continue
            }

            const selectedCandidatePairId = getStatString(stat, "selectedCandidatePairId")
            if (!selectedCandidatePairId) {
                continue
            }

            const selectedPair = stats.get(selectedCandidatePairId)
            if (selectedPair?.type == "candidate-pair") {
                return selectedPair as RTCStatsLike
            }
        }

        let fallbackPair: RTCStatsLike | null = null
        for (const value of stats.values()) {
            const stat = value as RTCStatsLike
            if (stat.type != "candidate-pair") {
                continue
            }

            if (getStatBool(stat, "selected") === true) {
                return stat
            }
            if (fallbackPair == null && (getStatBool(stat, "nominated") === true || getStatString(stat, "state") == "succeeded")) {
                fallbackPair = stat
            }
        }

        return fallbackPair
    }

    private describeCandidate(candidate: RTCStatsLike | null): string {
        const candidateType = getStatString(candidate, "candidateType") ?? "unknown"
        const protocol = getStatString(candidate, "protocol") ?? "unknown"
        const family = inferCandidateAddressFamily(candidate)
        const relayProtocol = getStatString(candidate, "relayProtocol")
        const protocolLabel = family && family != "unknown"
            ? `${protocol}${family}`
            : protocol

        return relayProtocol
            ? `${candidateType}/${protocolLabel}/${relayProtocol}`
            : `${candidateType}/${protocolLabel}`
    }

    private async getRouteDiagnostics(): Promise<TransportMicrophoneRouteDiagnostics | null> {
        const peer = this.peer
        if (!peer || typeof peer.getStats != "function") {
            this.lastRouteDiagnostics = null
            return null
        }

        try {
            const stats = await peer.getStats()
            const selectedPair = this.findSelectedCandidatePair(stats)
            if (!selectedPair) {
                this.lastRouteDiagnostics = null
                return null
            }

            const localCandidateId = getStatString(selectedPair, "localCandidateId")
            const remoteCandidateId = getStatString(selectedPair, "remoteCandidateId")
            const localCandidate = localCandidateId ? stats.get(localCandidateId) as RTCStatsLike | undefined : undefined
            const remoteCandidate = remoteCandidateId ? stats.get(remoteCandidateId) as RTCStatsLike | undefined : undefined

            const localCandidateType = getStatString(localCandidate ?? null, "candidateType")
            const remoteCandidateType = getStatString(remoteCandidate ?? null, "candidateType")
            const localProtocol = getStatString(localCandidate ?? null, "protocol")
            const remoteProtocol = getStatString(remoteCandidate ?? null, "protocol")
            const localAddressFamily = inferCandidateAddressFamily(localCandidate ?? null)
            const remoteAddressFamily = inferCandidateAddressFamily(remoteCandidate ?? null)
            const relayProtocol = getStatString(localCandidate ?? null, "relayProtocol") ?? getStatString(remoteCandidate ?? null, "relayProtocol")
            const usesRelay = localCandidateType == "relay" || remoteCandidateType == "relay"
            const selectedPairState = getStatString(selectedPair, "state")
            const currentRoundTripTime = getStatNumber(selectedPair, "currentRoundTripTime")
            const selectedPairRttMs = currentRoundTripTime != null ? currentRoundTripTime * 1000 : null

            const diagnostics: TransportMicrophoneRouteDiagnostics = {
                route: usesRelay ? "relay" : "direct",
                pathSummary: null,
                summary: usesRelay
                    ? `Mic sidecar route: relay via TURN (local=${this.describeCandidate(localCandidate ?? null)}, remote=${this.describeCandidate(remoteCandidate ?? null)})`
                    : `Mic sidecar route: direct peer-to-peer (local=${this.describeCandidate(localCandidate ?? null)}, remote=${this.describeCandidate(remoteCandidate ?? null)})`,
                selectedPairState,
                selectedPairRttMs,
                localCandidateType,
                localProtocol,
                localAddressFamily,
                remoteCandidateType,
                remoteProtocol,
                remoteAddressFamily,
                relayProtocol
            }
            diagnostics.pathSummary = this.buildRoutePathSummary(diagnostics)

            const signature = [
                diagnostics.route ?? "--",
                diagnostics.pathSummary ?? "--",
                diagnostics.selectedPairState ?? "--",
                diagnostics.selectedPairRttMs != null ? Math.round(diagnostics.selectedPairRttMs) : "--"
            ].join("|")
            if (signature != this.lastRouteLogSignature) {
                this.lastRouteLogSignature = signature
                const parts = [diagnostics.pathSummary ?? diagnostics.summary ?? "--"]
                if (diagnostics.selectedPairRttMs != null) {
                    parts.push(`rtt ${Math.round(diagnostics.selectedPairRttMs)} ms`)
                }
                this.log(`Mic sidecar path ${parts.join(" / ")}`)
            }

            this.lastRouteDiagnostics = diagnostics
            return diagnostics
        } catch {
            return this.lastRouteDiagnostics
        }
    }

    getState(): TransportMicrophoneState {
        if (this.localTrack?.readyState != "live") {
            this.enabled = false
        }

        return {
            supported: !!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function"),
            enabled: this.enabled,
            attached: !!(this.localTrack && this.localTrack.readyState == "live" && this.sender),
            uplinkNegotiated: this.peer ? true : null,
            direction: this.currentDirection,
            selectedDeviceId: this.selectedDeviceId,
            level: this.updateLevelEstimate(),
            outbound: this.lastOutboundStats
        }
    }

    async getDiagnostics(): Promise<TransportMicrophoneDiagnostics> {
        return {
            selectedDeviceId: this.selectedDeviceId,
            level: this.updateLevelEstimate(),
            outbound: await this.getOutboundStats(),
            route: await this.getRouteDiagnostics()
        }
    }

    private sendSignal(message: StreamSignalingMessage) {
        const ws = this.ws
        if (!ws || ws.readyState != WebSocket.OPEN) {
            return
        }

        const payload: MicSidecarClientMessage = {
            WebRtc: message
        }
        ws.send(JSON.stringify(payload))
    }

    private configureAudioCodecPreferences(peer: RTCPeerConnection, transceiver: RTCRtpTransceiver) {
        if (typeof transceiver.setCodecPreferences != "function") {
            return
        }
        if (typeof RTCRtpSender == "undefined" || typeof RTCRtpSender.getCapabilities != "function") {
            return
        }

        const capabilities = RTCRtpSender.getCapabilities("audio")
        if (!capabilities?.codecs?.length) {
            return
        }

        const opusCodecs = capabilities.codecs
            .filter((codec) => String(codec.mimeType || "").toLowerCase() == "audio/opus")
            .map((codec) => ({
                ...codec,
                channels: 1,
                sdpFmtpLine: mergeFmtpLine(codec.sdpFmtpLine, MIC_SIDECAR_OPUS_FMTP.split(";"))
            }))

        if (opusCodecs.length == 0) {
            return
        }

        try {
            transceiver.setCodecPreferences(opusCodecs)
            this.log(`Mic sidecar codec tuned for voice uplink (${MIC_SIDECAR_TARGET_BITRATE_BPS / 1000} kbps target).`)
        } catch (error) {
            console.info("[Mic Sidecar] Failed to set codec preferences", error)
        }
    }

    private async applySenderParameters() {
        const sender = this.sender
        if (!sender || typeof sender.getParameters != "function" || typeof sender.setParameters != "function") {
            return
        }

        try {
            const parameters = sender.getParameters()
            const encodings = parameters.encodings && parameters.encodings.length > 0
                ? parameters.encodings.map((encoding) => ({ ...encoding }))
                : [{}]

            encodings[0].maxBitrate = MIC_SIDECAR_TARGET_BITRATE_BPS
            ;(encodings[0] as Record<string, unknown>).priority = "low"
            ;(encodings[0] as Record<string, unknown>).networkPriority = "low"
            parameters.encodings = encodings
            await sender.setParameters(parameters)
        } catch (error) {
            console.info("[Mic Sidecar] Failed to apply sender parameters", error)
        }
    }

    private async tryDequeueRemoteIceCandidates() {
        if (!this.peer) {
            return
        }

        const queued = this.pendingRemoteIceCandidates.splice(0)
        for (const candidate of queued) {
            await this.peer.addIceCandidate(candidate)
        }
    }

    private async handleSignalMessage(message: MicSidecarServerMessage) {
        if ("DebugLog" in message) {
            this.log(message.DebugLog.message, message.DebugLog.ty ?? null)
            return
        }
        if ("Setup" in message) {
            return
        }
        if (!this.peer) {
            if ("WebRtc" in message && "AddIceCandidate" in message.WebRtc) {
                const candidate = message.WebRtc.AddIceCandidate
                this.pendingRemoteIceCandidates.push({
                    candidate: candidate.candidate,
                    sdpMid: candidate.sdp_mid ?? null,
                    sdpMLineIndex: candidate.sdp_mline_index ?? null,
                    usernameFragment: candidate.username_fragment ?? null
                })
            }
            return
        }

        const signal = message.WebRtc
        if ("Description" in signal) {
            const description = signal.Description
            await this.peer.setRemoteDescription({
                type: description.ty as RTCSdpType,
                sdp: description.sdp
            })
            await this.tryDequeueRemoteIceCandidates()
        } else if ("AddIceCandidate" in signal) {
            const candidate = signal.AddIceCandidate
            const iceCandidate: RTCIceCandidateInit = {
                candidate: candidate.candidate,
                sdpMid: candidate.sdp_mid ?? null,
                sdpMLineIndex: candidate.sdp_mline_index ?? null,
                usernameFragment: candidate.username_fragment ?? null
            }
            if (!this.peer.remoteDescription) {
                this.pendingRemoteIceCandidates.push(iceCandidate)
            } else {
                await this.peer.addIceCandidate(iceCandidate)
            }
        }
    }

    private async openSignaling(sessionId: number): Promise<{ ws: WebSocket, iceServers: RTCIceServer[] }> {
        return await new Promise((resolve, reject) => {
            const ws = new WebSocket(this.signalingUrl)
            let settled = false

            const fail = (error: unknown) => {
                if (settled) {
                    return
                }
                settled = true
                try {
                    ws.close()
                } catch {
                    // ignore
                }
                reject(error)
            }

            ws.addEventListener("open", () => {
                const payload: MicSidecarClientMessage = {
                    Init: {
                        host_id: this.hostId
                    }
                }
                ws.send(JSON.stringify(payload))
            })

            ws.addEventListener("error", () => {
                fail(new Error("mic_sidecar_ws_error"))
            })

            ws.addEventListener("close", () => {
                if (!settled) {
                    fail(new Error("mic_sidecar_ws_closed"))
                    return
                }

                if (this.ws === ws) {
                    this.enabled = false
                    this.expectedEnabled = false
                    this.connecting = false
                    this.resetPeerState()
                    this.releaseLocalTrack()
                    this.notifyStateChanged()
                }
            })

            ws.addEventListener("message", (event) => {
                if (typeof event.data != "string") {
                    return
                }

                const message = JSON.parse(event.data) as MicSidecarServerMessage
                if (!settled) {
                    if (!("Setup" in message)) {
                        if ("DebugLog" in message) {
                            this.log(message.DebugLog.message, message.DebugLog.ty ?? null)
                        }
                        return
                    }

                    settled = true
                    this.ws = ws
                    ws.addEventListener("message", (innerEvent) => {
                        if (typeof innerEvent.data != "string") {
                            return
                        }
                        const innerMessage = JSON.parse(innerEvent.data) as MicSidecarServerMessage
                        void this.handleSignalMessage(innerMessage)
                    })
                    resolve({
                        ws,
                        iceServers: message.Setup.ice_servers.map((server) => ({
                            urls: server.urls,
                            username: server.username || undefined,
                            credential: server.credential || undefined
                        }))
                    })
                    return
                }
            })

            if (sessionId != this.sessionSerial) {
                fail(new Error("mic_sidecar_session_superseded"))
            }
        })
    }

    async close(explicit = true): Promise<void> {
        this.expectedEnabled = false
        this.enabled = false
        this.connecting = false
        this.sessionSerial += 1
        this.lastRouteDiagnostics = null
        this.lastRouteLogSignature = ""

        const ws = this.ws
        const peer = this.peer
        const sender = this.sender
        const transceiver = this.transceiver

        this.ws = null
        this.resetPeerState()

        if (sender && typeof sender.replaceTrack == "function") {
            try {
                await sender.replaceTrack(null)
            } catch {
                // ignore
            }
        }

        if (transceiver) {
            try {
                transceiver.direction = "inactive"
            } catch {
                // ignore
            }
            try {
                if (typeof transceiver.stop == "function") {
                    transceiver.stop()
                }
            } catch {
                // ignore
            }
        }

        if (peer) {
            try {
                peer.close()
            } catch {
                // ignore
            }
        }

        if (ws) {
            try {
                if (explicit && ws.readyState == WebSocket.OPEN) {
                    const payload: MicSidecarClientMessage = "Stop"
                    ws.send(JSON.stringify(payload))
                }
            } catch {
                // ignore
            }
            try {
                ws.close()
            } catch {
                // ignore
            }
        }

        this.releaseLocalTrack()
        this.notifyStateChanged()
    }

    async setEnabled(enabled: boolean): Promise<TransportMicrophoneSetResult> {
        const wantEnabled = !!enabled
        const browserSupported = !!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function")
        this.expectedEnabled = wantEnabled

        if (!browserSupported) {
            return {
                supported: false,
                enabled: false,
                reason: "media_unsupported"
            }
        }

        if (!wantEnabled) {
            await this.close(true)
            return {
                supported: true,
                enabled: false
            }
        }

        this.connecting = true
        this.notifyStateChanged()

        const sessionId = this.sessionSerial + 1

        try {
            await this.close(false)
            this.sessionSerial = sessionId
            this.expectedEnabled = true
            this.connecting = true

            const audioConstraints: MediaTrackConstraints = {
                echoCancellation: false,
                noiseSuppression: false,
                autoGainControl: false,
                channelCount: { ideal: 1, max: 1 },
                sampleRate: { ideal: MIC_SIDECAR_TARGET_SAMPLE_RATE },
                sampleSize: { ideal: 16 }
            }
            if (this.selectedDeviceId != "default") {
                audioConstraints.deviceId = { exact: this.selectedDeviceId }
            }

            let stream: MediaStream
            try {
                stream = await navigator.mediaDevices.getUserMedia({
                    audio: audioConstraints,
                    video: false
                })
            } catch (error) {
                const errorName = error && typeof error == "object" && "name" in error ? String(error.name || "") : ""
                const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message || "") : String(error || "")
                this.connecting = false
                this.notifyStateChanged()
                return {
                    supported: true,
                    enabled: false,
                    reason: "capture_failed",
                    errorName,
                    errorMessage
                }
            }

            if (sessionId != this.sessionSerial || !this.expectedEnabled) {
                for (const track of stream.getTracks()) {
                    track.stop()
                }
                return {
                    supported: true,
                    enabled: false,
                    reason: "superseded"
                }
            }

            const track = stream.getAudioTracks()[0] || null
            if (!track) {
                for (const streamTrack of stream.getTracks()) {
                    streamTrack.stop()
                }
                this.connecting = false
                this.notifyStateChanged()
                return {
                    supported: true,
                    enabled: false,
                    reason: "no_audio_track"
                }
            }

            this.localStream = stream
            this.localTrack = track
            track.contentHint = "speech"
            this.attachTrackLifecycle(track)
            this.startLevelMeter(stream)

            const { iceServers } = await this.openSignaling(sessionId)
            if (sessionId != this.sessionSerial || !this.expectedEnabled) {
                await this.close(false)
                return {
                    supported: true,
                    enabled: false,
                    reason: "superseded"
                }
            }

            const peer = new RTCPeerConnection({
                iceServers,
                iceCandidatePoolSize: 2
            })
            this.peer = peer
            this.currentDirection = "sendonly"

            peer.addEventListener("icecandidate", (event) => {
                if (!event.candidate) {
                    return
                }

                const candidate = event.candidate.toJSON()
                this.sendSignal({
                    AddIceCandidate: {
                        candidate: candidate.candidate ?? "",
                        sdp_mid: candidate.sdpMid ?? null,
                        sdp_mline_index: candidate.sdpMLineIndex ?? null,
                        username_fragment: candidate.usernameFragment ?? null
                    }
                })
            })
            peer.addEventListener("connectionstatechange", () => {
                this.notifyStateChanged()
                void this.getRouteDiagnostics()
            })
            peer.addEventListener("iceconnectionstatechange", () => {
                this.notifyStateChanged()
                void this.getRouteDiagnostics()
            })

            this.transceiver = peer.addTransceiver("audio", { direction: "sendonly" })
            this.sender = this.transceiver.sender
            this.configureAudioCodecPreferences(peer, this.transceiver)
            await this.sender.replaceTrack(track)
            await this.applySenderParameters()

            const offer = await peer.createOffer()
            await peer.setLocalDescription(offer)
            const localDescription = peer.localDescription
            if (!localDescription) {
                throw new Error("mic_sidecar_missing_offer")
            }

            this.sendSignal({
                Description: {
                    ty: localDescription.type,
                    sdp: localDescription.sdp ?? ""
                }
            })

            this.enabled = track.readyState == "live"
            this.connecting = false
            this.notifyStateChanged()

            return {
                supported: true,
                enabled: this.enabled
            }
        } catch (error) {
            const errorName = error && typeof error == "object" && "name" in error ? String(error.name || "") : ""
            const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message || "") : String(error || "")
            await this.close(false)
            this.connecting = false
            this.notifyStateChanged()
            return {
                supported: true,
                enabled: false,
                reason: "sidecar_failed",
                errorName,
                errorMessage
            }
        }
    }
}
