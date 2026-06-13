import { StreamSignalingMessage, TransportChannelId } from "../../api_bindings.js";
import { Logger } from "../log.js";
import { StatValue } from "../stats.js";
import { allVideoCodecs, CAPABILITIES_CODECS, emptyVideoCodecs, maybeVideoCodecs, VideoCodecSupport } from "../video.js";
import {
    AudioTrackTransportChannel,
    DataTransportChannel,
    TrackTransportChannel,
    Transport,
    TRANSPORT_CHANNEL_OPTIONS,
    TransportAudioSetup,
    TransportChannel,
    TransportChannelIdKey,
    TransportChannelIdValue,
    TransportMicrophoneDevice,
    TransportMicrophoneDeviceResult,
    TransportMicrophoneDiagnostics,
    TransportMicrophoneOutboundStats,
    TransportMicrophoneSetResult,
    TransportMicrophoneState,
    TransportShutdown,
    TransportVideoSetup,
    VideoTrackTransportChannel
} from "./index.js";

type RTCStatsLike = RTCStats & Record<string, unknown>
type LowLatencyRtpReceiver = RTCRtpReceiver & {
    jitterBufferTarget?: number
    playoutDelayHint?: number
}
type InboundVideoActivitySnapshot = {
    packetsReceived: number
    bytesReceived: number
    framesDecoded: number
    framesDecodedKnown: boolean
}

const PREFER_IPV4_CANDIDATES = (() => {
    try {
        return typeof window != "undefined"
            && window.localStorage != null
            && window.localStorage.getItem("ML_PREFER_IPV4_CANDIDATES") == "1"
    } catch (_error) {
        return false
    }
})()

function preferStableTcpCandidates(): boolean {
    try {
        return typeof window != "undefined"
            && window.sessionStorage != null
            && window.sessionStorage.getItem("ML_PREFER_STABLE_TCP_P2P") == "1"
    } catch (_error) {
        return false
    }
}

function debugTransportPacketsEnabled(): boolean {
    try {
        return typeof window != "undefined"
            && window.localStorage != null
            && window.localStorage.getItem("ML_DEBUG_TRANSPORT_PACKETS") == "1"
    } catch (_error) {
        return false
    }
}

const IPV6_CANDIDATE_DEFER_MS = 1200
const PUBLIC_HOST_UDP_CANDIDATE_DEFER_MS = 4500

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

function inferCandidateAddressFamily(candidate: RTCStatsLike | null): string | null {
    const address = getStatString(candidate, "address") ?? getStatString(candidate, "ip")
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

function getCandidateString(candidate: RTCIceCandidateInit | null | undefined): string {
    return (candidate?.candidate ?? "").trim()
}

function getCandidateParts(candidateString: string): string[] {
    if (!candidateString) {
        return []
    }

    return candidateString.split(/\s+/)
}

function getCandidateAddress(candidate: RTCIceCandidateInit | null | undefined): string {
    const parts = getCandidateParts(getCandidateString(candidate))
    if (parts.length < 5) {
        return ""
    }

    return parts[4]
}

function isIpv6Address(address: string): boolean {
    return address.includes(":")
}

function isIpv4Address(address: string): boolean {
    return /^\d{1,3}(?:\.\d{1,3}){3}$/.test(address)
}

function parseIpv4Octets(address: string): number[] | null {
    if (!isIpv4Address(address)) {
        return null
    }

    const octets = address.split(".").map((part) => Number.parseInt(part, 10))
    if (octets.length != 4 || octets.some((part) => !Number.isFinite(part) || part < 0 || part > 255)) {
        return null
    }

    return octets
}

function isPrivateIpv4Address(address: string): boolean {
    const octets = parseIpv4Octets(address)
    if (!octets) {
        return false
    }

    if (octets[0] == 10) {
        return true
    }
    if (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31) {
        return true
    }
    if (octets[0] == 192 && octets[1] == 168) {
        return true
    }
    if (octets[0] == 169 && octets[1] == 254) {
        return true
    }
    if (octets[0] == 127) {
        return true
    }

    return false
}

function getPrivateIpv4Prefix(address: string): string | null {
    const octets = parseIpv4Octets(address)
    if (!octets || !isPrivateIpv4Address(address)) {
        return null
    }

    return `${octets[0]}.${octets[1]}.${octets[2]}`
}

function getCandidateProtocol(candidate: RTCIceCandidateInit | null | undefined): string {
    const parts = getCandidateParts(getCandidateString(candidate))
    if (parts.length < 3) {
        return ""
    }

    return parts[2].toLowerCase()
}

function getCandidateType(candidate: RTCIceCandidateInit | null | undefined): string {
    const parts = getCandidateParts(getCandidateString(candidate))
    const typeIndex = parts.indexOf("typ")
    if (typeIndex >= 0 && parts.length > typeIndex + 1) {
        return parts[typeIndex + 1].toLowerCase()
    }

    return ""
}

function isPrivateHostCandidate(candidate: RTCIceCandidateInit | null | undefined): boolean {
    if (getCandidateType(candidate) != "host") {
        return false
    }

    const address = getCandidateAddress(candidate)
    return getPrivateIpv4Prefix(address) != null
}

function isPublicHostCandidate(candidate: RTCIceCandidateInit | null | undefined): boolean {
    if (getCandidateType(candidate) != "host") {
        return false
    }

    if (isPrivateHostCandidate(candidate)) {
        return false
    }

    const address = getCandidateAddress(candidate)
    return isIpv4Address(address) || isIpv6Address(address)
}

function getRemoteCandidatePriority(candidate: RTCIceCandidateInit): number {
    const protocol = getCandidateProtocol(candidate)
    const type = getCandidateType(candidate)
    const address = getCandidateAddress(candidate)
    const preferTcp = preferStableTcpCandidates()
    const privateHost = isPrivateHostCandidate(candidate)
    const publicHost = isPublicHostCandidate(candidate)

    let score = 0
    if (protocol == "tcp") {
        score += (preferTcp || publicHost || type == "srflx" || type == "prflx") ? 170 : 60
    } else if (protocol == "udp") {
        if (publicHost) {
            score += 20
        } else if (preferTcp) {
            score += 90
        } else {
            score += 130
        }
    }

    if (type == "srflx" || type == "prflx") {
        score += 90
    } else if (privateHost) {
        score += 70
    } else if (publicHost) {
        score += protocol == "tcp" ? 35 : -40
    } else if (type == "relay") {
        score += 20
    }

    if (PREFER_IPV4_CANDIDATES) {
        if (isIpv4Address(address)) {
            score += 20
        } else if (isIpv6Address(address)) {
            score -= 5
        }
    }

    return -score
}

const DEFAULT_MICROPHONE_OUTBOUND_STATS: TransportMicrophoneOutboundStats = {
    supported: false,
    timestampMs: 0,
    bitrateKbps: null,
    packetsSent: null,
    bytesSent: null
}

export class WebRTCTransport implements Transport {
    implementationName: string = "webrtc"
    private readonly initialConnectionTimeoutMs = 6000

    private logger: Logger | null

    private peer: RTCPeerConnection | null = null
    private closeRequested = false
    private closeDispatched = false
    private connectionTimeoutId: number | null = null
    private recoveryTimeoutId: number | null = null
    private recoveryRestartIssued = false
    private recoveryActivitySnapshot: InboundVideoActivitySnapshot | null = null
    private recoveryActivityExtensions = 0
    private signalingQueue: Promise<void> = Promise.resolve()
    private pendingRemoteDescriptions: Array<RTCSessionDescriptionInit | null> = []
    private selectedRouteStats: Record<string, StatValue> = {}
    private receiverLatencyHintStats: Record<string, StatValue> = {}
    private lastReportedRouteSummary: string | null = null
    private rollingCounterSnapshots = new Map<string, { total: number, count: number }>()
    private hasReceivedIpv4Candidate = false
    private deferredIpv6Candidates: Array<RTCIceCandidateInit> = []
    private deferIpv6Timer: number | null = null
    private deferredPublicHostCandidates: Array<RTCIceCandidateInit> = []
    private deferPublicHostTimer: number | null = null
    private localIceCandidateCount = 0
    private localIceCandidateTypes = new Set<string>()
    private localPrivateIpv4Prefixes = new Set<string>()
    private localMicrophoneStream: MediaStream | null = null
    private localMicrophoneTrack: MediaStreamTrack | null = null
    private localMicrophoneSender: RTCRtpSender | null = null
    private localMicrophoneTransceiver: RTCRtpTransceiver | null = null
    private microphoneEnabled = false
    private microphoneUplinkNegotiated: boolean | null = null
    private microphoneUplinkDirection = "unknown"
    private preferredMicrophoneDeviceId = "default"
    private microphoneLevelValue = 0
    private microphoneLevelContext: AudioContext | null = null
    private microphoneLevelSource: MediaStreamAudioSourceNode | null = null
    private microphoneLevelAnalyser: AnalyserNode | null = null
    private microphoneLevelData: Uint8Array | null = null
    private lastMicrophoneOutboundSnapshot: { timestampMs: number, bytesSent: number | null } | null = null
    private lastMicrophoneOutboundStats: TransportMicrophoneOutboundStats = { ...DEFAULT_MICROPHONE_OUTBOUND_STATS }

    constructor(logger?: Logger) {
        this.logger = logger ?? null
    }

    private queueSignalingOperation(label: string, operation: () => Promise<void>): Promise<void> {
        const run = async () => {
            try {
                await operation()
            } catch (error) {
                const errorName = error && typeof error == "object" && "name" in error ? String(error.name) : ""
                const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message) : String(error ?? "")
                this.logger?.debug(`${label} failed${errorName || errorMessage ? `: ${errorName}${errorMessage ? ` ${errorMessage}` : ""}` : ""}`)
            }
        }

        this.signalingQueue = this.signalingQueue.then(run, run)
        return this.signalingQueue
    }

    private flushRemoteDescriptionQueue(): Promise<void> {
        return this.queueSignalingOperation("Remote description handling", async () => {
            if (!this.peer) {
                return
            }

            while (this.pendingRemoteDescriptions.length > 0) {
                const remoteDescription = this.pendingRemoteDescriptions.shift() ?? null
                if (!remoteDescription) {
                    continue
                }

                await this.peer.setRemoteDescription(remoteDescription)

                if (remoteDescription.type == "offer") {
                    await this.peer.setLocalDescription()
                    const localDescription = this.peer.localDescription
                    if (!localDescription) {
                        this.logger?.debug("Peer didn't have a localDescription whilst receiving an offer and trying to answer")
                        continue
                    }

                    this.logger?.debug(`Responding to offer description: ${localDescription.type}`)
                    this.sendMessage({
                        Description: {
                            ty: localDescription.type,
                            sdp: localDescription.sdp ?? ""
                        }
                    })
                }
            }
        })
    }

    private normalizeDirection(direction: RTCRtpTransceiverDirection | string | null | undefined): string {
        const value = String(direction || "").toLowerCase()
        if (value == "sendrecv" || value == "sendonly" || value == "recvonly" || value == "inactive") {
            return value
        }

        return "unknown"
    }

    private isDirectionSendCapable(direction: string): boolean {
        return direction == "sendrecv" || direction == "sendonly"
    }

    private refreshMicrophoneUplinkState(): { negotiated: boolean | null, direction: string } {
        this.microphoneUplinkNegotiated = null
        this.microphoneUplinkDirection = "unknown"

        let direction: string | null = null
        if (this.localMicrophoneTransceiver?.currentDirection) {
            direction = this.normalizeDirection(this.localMicrophoneTransceiver.currentDirection)
        } else if (this.peer && typeof this.peer.getTransceivers == "function") {
            for (const transceiver of this.peer.getTransceivers()) {
                if (!transceiver || !transceiver.sender) {
                    continue
                }
                if (this.localMicrophoneSender && transceiver.sender !== this.localMicrophoneSender) {
                    continue
                }

                const track = transceiver.sender.track
                const isAudioSender = (track && track.kind == "audio") || !!this.localMicrophoneSender
                if (!isAudioSender) {
                    continue
                }
                if (transceiver.currentDirection) {
                    direction = this.normalizeDirection(transceiver.currentDirection)
                    break
                }
            }
        }

        if (direction && direction != "unknown") {
            this.microphoneUplinkDirection = direction
            this.microphoneUplinkNegotiated = this.isDirectionSendCapable(direction)
        }

        return {
            negotiated: this.microphoneUplinkNegotiated,
            direction: this.microphoneUplinkDirection
        }
    }

    private stopMicrophoneLevelMeter() {
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

    private startMicrophoneLevelMeter(stream: MediaStream) {
        this.stopMicrophoneLevelMeter()

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
            this.stopMicrophoneLevelMeter()
        }
    }

    private updateMicrophoneLevelEstimate(): number {
        if (!this.microphoneLevelAnalyser || !this.microphoneLevelData) {
            this.microphoneLevelValue = 0
            return this.microphoneLevelValue
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
            return this.microphoneLevelValue
        }
    }

    private isLocalMicrophoneTrackLive(): boolean {
        return !!(this.localMicrophoneTrack && this.localMicrophoneTrack.readyState == "live")
    }

    private syncMicrophoneTrackRuntimeState() {
        if (!this.isLocalMicrophoneTrackLive()) {
            this.microphoneEnabled = false
        }
    }

    private attachMicrophoneTrackLifecycle(track: MediaStreamTrack) {
        const markInactive = (reason: string) => {
            if (this.localMicrophoneTrack !== track) {
                return
            }

            this.microphoneEnabled = false
            this.logger?.debug(`Microphone track became inactive: ${reason}`)
        }

        track.addEventListener("ended", () => {
            markInactive("ended")
        })
        track.addEventListener("mute", () => {
            this.logger?.debug("Microphone track muted")
        })
        track.addEventListener("unmute", () => {
            if (this.localMicrophoneTrack === track && track.readyState == "live") {
                this.logger?.debug("Microphone track unmuted")
            }
        })
    }

    private releaseLocalMicrophoneTrack() {
        this.stopMicrophoneLevelMeter()

        if (this.localMicrophoneTrack) {
            try {
                this.localMicrophoneTrack.stop()
            } catch {
                // ignore
            }
        }
        if (this.localMicrophoneStream) {
            try {
                for (const track of this.localMicrophoneStream.getTracks()) {
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

        this.localMicrophoneTrack = null
        this.localMicrophoneStream = null
    }

    setMicrophoneDeviceId(deviceId: string): TransportMicrophoneDeviceResult {
        const normalized = String(deviceId || "default").trim() || "default"
        this.preferredMicrophoneDeviceId = normalized
        return {
            ok: true,
            deviceId: this.preferredMicrophoneDeviceId
        }
    }

    getMicrophoneDeviceId(): string {
        return this.preferredMicrophoneDeviceId
    }

    async listMicrophoneDevices(): Promise<Array<TransportMicrophoneDevice>> {
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

            const hasDefault = microphones.some((device) => device.deviceId == "default")
            if (!hasDefault) {
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

    private async getMicrophoneOutboundStats(): Promise<TransportMicrophoneOutboundStats> {
        const fallback: TransportMicrophoneOutboundStats = {
            ...DEFAULT_MICROPHONE_OUTBOUND_STATS,
            timestampMs: Date.now()
        }
        if (!this.peer) {
            this.lastMicrophoneOutboundStats = fallback
            return fallback
        }

        let sender = this.localMicrophoneSender
        if (!sender && typeof this.peer.getSenders == "function") {
            for (const peerSender of this.peer.getSenders()) {
                if (peerSender && peerSender.track && peerSender.track.kind == "audio") {
                    sender = peerSender
                    break
                }
            }
        }

        if (!sender || typeof sender.getStats != "function") {
            this.lastMicrophoneOutboundStats = fallback
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
                this.lastMicrophoneOutboundStats = fallback
                return fallback
            }

            const timestampMs = getStatNumber(outbound, "timestamp") ?? Date.now()
            const packetsSent = getStatNumber(outbound, "packetsSent")
            const bytesSent = getStatNumber(outbound, "bytesSent")
            let bitrateKbps: number | null = null
            if (
                bytesSent != null
                && this.lastMicrophoneOutboundSnapshot
                && this.lastMicrophoneOutboundSnapshot.bytesSent != null
                && timestampMs > this.lastMicrophoneOutboundSnapshot.timestampMs
            ) {
                const deltaBytes = bytesSent - this.lastMicrophoneOutboundSnapshot.bytesSent
                const deltaMs = timestampMs - this.lastMicrophoneOutboundSnapshot.timestampMs
                if (deltaBytes >= 0 && deltaMs > 0) {
                    bitrateKbps = (deltaBytes * 8) / deltaMs
                }
            }

            this.lastMicrophoneOutboundSnapshot = {
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
            this.lastMicrophoneOutboundStats = result
            return result
        } catch {
            this.lastMicrophoneOutboundStats = fallback
            return fallback
        }
    }

    async getMicrophoneDiagnostics(): Promise<TransportMicrophoneDiagnostics> {
        this.syncMicrophoneTrackRuntimeState()
        return {
            selectedDeviceId: this.preferredMicrophoneDeviceId,
            level: this.updateMicrophoneLevelEstimate(),
            outbound: await this.getMicrophoneOutboundStats()
        }
    }

    getMicrophoneState(): TransportMicrophoneState {
        this.syncMicrophoneTrackRuntimeState()
        const uplink = this.refreshMicrophoneUplinkState()
        return {
            supported: !!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function"),
            enabled: this.microphoneEnabled,
            attached: this.isLocalMicrophoneTrackLive(),
            uplinkNegotiated: uplink.negotiated,
            direction: uplink.direction,
            selectedDeviceId: this.preferredMicrophoneDeviceId,
            level: this.updateMicrophoneLevelEstimate(),
            outbound: this.lastMicrophoneOutboundStats
        }
    }

    async setMicrophoneEnabled(enabled: boolean): Promise<TransportMicrophoneSetResult> {
        const wantEnabled = !!enabled
        if (!this.peer) {
            return {
                supported: false,
                enabled: false,
                reason: "no_peer"
            }
        }

        const peerState = String(this.peer.connectionState || "")
        const iceState = String(this.peer.iceConnectionState || "")
        const peerReady = peerState == "connected"
            && (iceState == "connected" || iceState == "completed")
        if (wantEnabled && !peerReady) {
            return {
                supported: true,
                enabled: false,
                reason: "peer_not_ready",
                errorName: "",
                errorMessage: `peer=${peerState}, ice=${iceState}`
            }
        }

        if (!(navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia == "function")) {
            return {
                supported: false,
                enabled: false,
                reason: "media_unsupported"
            }
        }

        if (!wantEnabled) {
            this.microphoneEnabled = false
            if (this.localMicrophoneSender) {
                try {
                    if (typeof this.localMicrophoneSender.replaceTrack == "function") {
                        await this.localMicrophoneSender.replaceTrack(null)
                    }
                } catch {
                    // ignore
                }
            }
            this.releaseLocalMicrophoneTrack()
            this.logger?.debug("Microphone disabled")
            return {
                supported: true,
                enabled: false
            }
        }

        try {
            const uplink = this.refreshMicrophoneUplinkState()
            if (uplink.negotiated === false) {
                return {
                    supported: true,
                    enabled: false,
                    reason: "uplink_not_negotiated",
                    errorName: "",
                    errorMessage: `Audio direction=${uplink.direction}. Runtime renegotiation disabled for stability.`
                }
            }

            let stream: MediaStream | null = null
            try {
                const audioConstraints: MediaTrackConstraints = {
                    echoCancellation: true,
                    noiseSuppression: true,
                    autoGainControl: true,
                    channelCount: 1
                }
                if (this.preferredMicrophoneDeviceId && this.preferredMicrophoneDeviceId != "default") {
                    audioConstraints.deviceId = { exact: this.preferredMicrophoneDeviceId }
                }

                stream = await navigator.mediaDevices.getUserMedia({
                    audio: audioConstraints,
                    video: false
                })
            } catch (error) {
                const shouldFallbackToDefault = this.preferredMicrophoneDeviceId != "default"
                    && error
                    && typeof error == "object"
                    && "name" in error
                    && (String(error.name) == "OverconstrainedError" || String(error.name) == "NotFoundError")

                if (shouldFallbackToDefault) {
                    try {
                        stream = await navigator.mediaDevices.getUserMedia({
                            audio: {
                                echoCancellation: true,
                                noiseSuppression: true,
                                autoGainControl: true,
                                channelCount: 1
                            },
                            video: false
                        })
                        this.preferredMicrophoneDeviceId = "default"
                    } catch {
                        stream = null
                    }
                }

                if (!stream) {
                    const errorName = error && typeof error == "object" && "name" in error ? String(error.name) : ""
                    const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message) : String(error || "")
                    this.logger?.debug(`Failed to capture microphone: ${errorName}${errorMessage ? ` ${errorMessage}` : ""}`)
                    return {
                        supported: true,
                        enabled: false,
                        reason: "capture_failed",
                        errorName,
                        errorMessage
                    }
                }
            }

            const track = stream.getAudioTracks()[0] || null
            if (!track) {
                return {
                    supported: true,
                    enabled: false,
                    reason: "no_audio_track"
                }
            }

            this.releaseLocalMicrophoneTrack()
            this.localMicrophoneStream = stream
            this.localMicrophoneTrack = track
            this.attachMicrophoneTrackLifecycle(track)
            this.startMicrophoneLevelMeter(stream)

            try {
                let sender = this.localMicrophoneSender
                if (!sender && this.localMicrophoneTransceiver?.sender) {
                    sender = this.localMicrophoneTransceiver.sender
                }
                if (sender && typeof sender.replaceTrack == "function") {
                    await sender.replaceTrack(track)
                } else {
                    return {
                        supported: true,
                        enabled: false,
                        reason: "sender_unavailable",
                        errorName: "",
                        errorMessage: "No microphone sender/transceiver available"
                    }
                }

                this.localMicrophoneSender = sender
                this.microphoneEnabled = track.readyState == "live"
                this.logger?.debug("Microphone enabled")
                return {
                    supported: true,
                    enabled: this.microphoneEnabled
                }
            } catch (error) {
                const errorName = error && typeof error == "object" && "name" in error ? String(error.name) : ""
                const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message) : String(error || "")
                this.microphoneEnabled = false
                this.releaseLocalMicrophoneTrack()
                this.logger?.debug(`Failed to attach microphone track to peer: ${errorName}${errorMessage ? ` ${errorMessage}` : ""}`)
                return {
                    supported: true,
                    enabled: false,
                    reason: "attach_failed",
                    errorName,
                    errorMessage
                }
            }
        } catch (error) {
            this.microphoneEnabled = false
            this.releaseLocalMicrophoneTrack()
            const errorName = error && typeof error == "object" && "name" in error ? String(error.name) : ""
            const errorMessage = error && typeof error == "object" && "message" in error ? String(error.message) : String(error || "")
            return {
                supported: true,
                enabled: false,
                reason: "unknown_failed",
                errorName,
                errorMessage
            }
        }
    }

    async initPeer(configuration?: RTCConfiguration) {
        this.logger?.debug(`Creating Client Peer`)

        if (this.peer) {
            this.logger?.debug(`Cannot create Peer because a Peer already exists`)
            return
        }

        this.closeRequested = false
        this.closeDispatched = false
        this.wasConnected = false
        this.clearConnectionTimeout()
        this.signalingQueue = Promise.resolve()
        this.selectedRouteStats = {}
        this.receiverLatencyHintStats = {}
        this.lastReportedRouteSummary = null
        this.rollingCounterSnapshots.clear()
        this.hasReceivedIpv4Candidate = false
        this.deferredIpv6Candidates.length = 0
        this.clearDeferredIpv6Timer()
        this.deferredPublicHostCandidates.length = 0
        this.clearDeferredPublicHostTimer()
        this.localIceCandidateCount = 0
        this.localIceCandidateTypes.clear()
        this.localPrivateIpv4Prefixes.clear()

        // Configure web rtc
        this.peer = new RTCPeerConnection({
            iceCandidatePoolSize: 4,
            ...configuration
        })
        this.peer.addEventListener("error", this.onError.bind(this))

        this.peer.addEventListener("negotiationneeded", this.onNegotiationNeeded.bind(this))
        this.peer.addEventListener("icecandidate", this.onIceCandidate.bind(this))
        this.peer.addEventListener("icecandidateerror", this.onIceCandidateError.bind(this))

        this.peer.addEventListener("connectionstatechange", this.onConnectionStateChange.bind(this))
        this.peer.addEventListener("signalingstatechange", this.onSignalingStateChange.bind(this))
        this.peer.addEventListener("iceconnectionstatechange", this.onIceConnectionStateChange.bind(this))
        this.peer.addEventListener("icegatheringstatechange", this.onIceGatheringStateChange.bind(this))

        this.peer.addEventListener("track", this.onTrack.bind(this))
        this.peer.addEventListener("datachannel", this.onDataChannel.bind(this))

        this.initChannels()

        // Maybe we already received data
        if (this.pendingRemoteDescriptions.length > 0) {
            await this.flushRemoteDescriptionQueue()
        } else {
            await this.onNegotiationNeeded()
        }
        await this.tryDequeueIceCandidates()
        this.armConnectionTimeout()
    }

    private onError(event: Event) {
        this.logger?.debug(`WebRTC Peer Error`)

        console.error(`WebRTC Peer Error`, event)
    }

    onsendmessage: ((message: StreamSignalingMessage) => void) | null = null
    private sendMessage(message: StreamSignalingMessage) {
        if (this.onsendmessage) {
            this.onsendmessage(message)
        } else {
            this.logger?.debug("Failed to call onicecandidate because no handler is set")
        }
    }
    async onReceiveMessage(message: StreamSignalingMessage) {
        if ("Description" in message) {
            const description = message.Description;
            await this.handleRemoteDescription({
                type: description.ty as RTCSdpType,
                sdp: description.sdp
            })
        } else if ("AddIceCandidate" in message) {
            const candidate = message.AddIceCandidate
            await this.addIceCandidate({
                candidate: candidate.candidate,
                sdpMid: candidate.sdp_mid,
                sdpMLineIndex: candidate.sdp_mline_index,
                usernameFragment: candidate.username_fragment
            })
        }
    }

    private async onNegotiationNeeded() {
        await this.queueSignalingOperation("OnNegotiationNeeded", async () => {
            if (!this.peer) {
                this.logger?.debug("OnNegotiationNeeded without a peer")
                return
            }

            if (this.pendingRemoteDescriptions.length > 0) {
                this.logger?.debug("Skipping OnNegotiationNeeded because a remote description is pending")
                return
            }

            if (this.peer.signalingState != "stable") {
                this.logger?.debug(`Skipping OnNegotiationNeeded while signalingState=${this.peer.signalingState}`)
                return
            }

            await this.peer.setLocalDescription()
            const localDescription = this.peer.localDescription
            if (!localDescription) {
                this.logger?.debug("Failed to set local description in OnNegotiationNeeded")
                return
            }

            this.logger?.debug(`OnNegotiationNeeded: Sending local description: ${localDescription.type}`)
            this.sendMessage({
                Description: {
                    ty: localDescription.type,
                    sdp: localDescription.sdp ?? ""
                }
            })
        })
    }

    private async handleRemoteDescription(sdp: RTCSessionDescriptionInit | null) {
        this.logger?.debug(`Received remote description: ${sdp?.type}`)
        this.pendingRemoteDescriptions.push(sdp)
        if (!this.peer) {
            return
        }

        await this.flushRemoteDescriptionQueue()
    }

    private onIceCandidate(event: RTCPeerConnectionIceEvent) {
        if (event.candidate) {
            const candidate = event.candidate.toJSON()
            const candidateText = candidate.candidate ?? ""
            const candidateType = this.parseCandidateType(candidateText)
            const candidateAddress = getCandidateAddress(candidate)
            this.localIceCandidateCount += 1
            if (candidateType) {
                this.localIceCandidateTypes.add(candidateType)
            }
            if (candidateType == "host") {
                const prefix = getPrivateIpv4Prefix(candidateAddress)
                if (prefix) {
                    this.localPrivateIpv4Prefixes.add(prefix)
                }
            }
            this.logger?.debug(`Sending ice candidate: ${candidate.candidate}`)

            this.sendMessage({
                AddIceCandidate: {
                    candidate: candidate.candidate ?? "",
                    sdp_mid: candidate.sdpMid ?? null,
                    sdp_mline_index: candidate.sdpMLineIndex ?? null,
                    username_fragment: candidate.usernameFragment ?? null
                }
            })
        } else {
            if (this.localIceCandidateCount == 0) {
                this.logger?.debug("No new ice candidates. Browser did not produce any local ICE candidates. This usually means client-side STUN/UDP is blocked or unavailable.")
            } else {
                const candidateTypes = [...this.localIceCandidateTypes].sort().join(", ")
                this.logger?.debug(`No new ice candidates. Local ICE candidates sent: ${this.localIceCandidateCount}${candidateTypes ? ` (${candidateTypes})` : ""}`)
            }
        }
    }

    private onIceCandidateError(event: RTCPeerConnectionIceErrorEvent) {
        const errorText = event.errorText || "unknown"
        const url = event.url || "unknown"
        const address = event.address || "unknown"
        const port = event.port || 0

        this.logger?.debug(`ICE candidate error: url=${url}, address=${address}, port=${port}, code=${event.errorCode}, text=${errorText}`)
    }

    private parseCandidateType(candidate: string): string | null {
        const match = candidate.match(/\styp\s([a-z0-9]+)/i)
        if (!match) {
            return null
        }

        return match[1].toLowerCase()
    }

    private iceCandidates: Array<RTCIceCandidateInit> = []
    private clearDeferredIpv6Timer() {
        if (this.deferIpv6Timer != null) {
            window.clearTimeout(this.deferIpv6Timer)
            this.deferIpv6Timer = null
        }
    }

    private clearDeferredPublicHostTimer() {
        if (this.deferPublicHostTimer != null) {
            window.clearTimeout(this.deferPublicHostTimer)
            this.deferPublicHostTimer = null
        }
    }

    private async flushDeferredIpv6Candidates() {
        this.clearDeferredIpv6Timer()

        if (this.deferredIpv6Candidates.length == 0) {
            return
        }

        const buffered = [...this.deferredIpv6Candidates]
        this.deferredIpv6Candidates.length = 0
        for (const candidate of buffered) {
            await this.addIceCandidate(candidate)
        }
    }

    private async flushDeferredPublicHostCandidates(force = false) {
        this.clearDeferredPublicHostTimer()

        if (this.deferredPublicHostCandidates.length == 0) {
            return
        }

        const peerConnected = this.wasConnected
            || this.peer?.connectionState == "connected"
            || this.peer?.iceConnectionState == "connected"
            || this.peer?.iceConnectionState == "completed"
        if (!force && peerConnected) {
            this.logger?.debug(`Discarding ${this.deferredPublicHostCandidates.length} deferred remote public host candidate(s) because a direct path is already established`)
            this.deferredPublicHostCandidates.length = 0
            return
        }

        const buffered = [...this.deferredPublicHostCandidates]
        this.deferredPublicHostCandidates.length = 0
        for (const candidate of buffered) {
            await this.addIceCandidate(candidate, true)
        }
    }

    private async addIceCandidate(candidate: RTCIceCandidateInit, skipPublicHostDefer = false) {
        this.logger?.debug(`Received ice candidate: ${candidate.candidate}`)

        const address = getCandidateAddress(candidate)
        const candidateType = getCandidateType(candidate)
        const protocol = getCandidateProtocol(candidate)
        const peerConnected = this.wasConnected
            || this.peer?.connectionState == "connected"
            || this.peer?.iceConnectionState == "connected"
            || this.peer?.iceConnectionState == "completed"
        if (candidateType == "host") {
            const privatePrefix = getPrivateIpv4Prefix(address)
            if (privatePrefix && !this.localPrivateIpv4Prefixes.has(privatePrefix)) {
                this.logger?.debug(`Ignoring remote private host candidate outside local LAN: ${candidate.candidate}`)
                return
            }

            const looksLikePublicHostIp = !privatePrefix && (isIpv4Address(address) || isIpv6Address(address))
            const publicHostUdp = looksLikePublicHostIp && protocol == "udp"
            if (publicHostUdp && peerConnected) {
                this.logger?.debug(`Ignoring remote public host UDP candidate after direct path establishment: ${candidate.candidate}`)
                return
            }
            if (publicHostUdp && !skipPublicHostDefer) {
                this.deferredPublicHostCandidates.push(candidate)
                if (this.deferPublicHostTimer == null) {
                    this.deferPublicHostTimer = window.setTimeout(() => {
                        this.deferPublicHostTimer = null
                        void this.flushDeferredPublicHostCandidates()
                    }, PUBLIC_HOST_UDP_CANDIDATE_DEFER_MS)
                }
                this.logger?.debug(`Deferring remote public host UDP candidate for ${PUBLIC_HOST_UDP_CANDIDATE_DEFER_MS} ms: ${candidate.candidate}`)
                return
            }
        }
        if (isIpv4Address(address)) {
            this.hasReceivedIpv4Candidate = true
            await this.flushDeferredIpv6Candidates()
        } else if (PREFER_IPV4_CANDIDATES && isIpv6Address(address) && !this.hasReceivedIpv4Candidate) {
            this.deferredIpv6Candidates.push(candidate)
            if (this.deferIpv6Timer == null) {
                this.deferIpv6Timer = window.setTimeout(() => {
                    this.deferIpv6Timer = null
                    void this.flushDeferredIpv6Candidates()
                }, IPV6_CANDIDATE_DEFER_MS)
            }
            return
        }

        if (!this.peer) {
            this.logger?.debug("Buffering ice candidate")

            this.iceCandidates.push(candidate)
            return
        }
        await this.tryDequeueIceCandidates()

        await this.peer.addIceCandidate(candidate)
    }
    private async tryDequeueIceCandidates() {
        if (!this.peer) {
            this.logger?.debug("called tryDequeueIceCandidates without a peer")
            return
        }

        this.iceCandidates.sort((left, right) => getRemoteCandidatePriority(left) - getRemoteCandidatePriority(right))
        for (const candidate of this.iceCandidates) {
            await this.peer.addIceCandidate(candidate)
        }
        this.iceCandidates.length = 0
    }

    private wasConnected = false
    private armConnectionTimeout() {
        this.clearConnectionTimeout()
        this.connectionTimeoutId = window.setTimeout(() => {
            this.connectionTimeoutId = null

            if (!this.peer || this.closeRequested || this.wasConnected) {
                return
            }

            if (this.peer.connectionState == "connected" || this.peer.iceConnectionState == "connected") {
                return
            }

            this.logger?.debug(`WebRTC direct path timed out after ${this.initialConnectionTimeoutMs} ms without a connection.`)
            this.emitClose("failednoconnect")
        }, this.initialConnectionTimeoutMs)
    }

    private clearConnectionTimeout() {
        if (this.connectionTimeoutId != null) {
            window.clearTimeout(this.connectionTimeoutId)
            this.connectionTimeoutId = null
        }
    }

    private clearRecoveryTimeout() {
        if (this.recoveryTimeoutId != null) {
            window.clearTimeout(this.recoveryTimeoutId)
            this.recoveryTimeoutId = null
        }
        this.recoveryRestartIssued = false
        this.recoveryActivitySnapshot = null
        this.recoveryActivityExtensions = 0
    }

    private armRecoveryTimeout(reason: "failed" | "disconnected") {
        if (!this.peer || this.closeRequested || !this.wasConnected) {
            return
        }

        if (this.recoveryTimeoutId != null) {
            return
        }

        const recoveryTimeoutMs = reason == "disconnected" ? 30000 : 45000
        this.logger?.debug(`Allowing ${reason} state to recover before closing transport (${recoveryTimeoutMs} ms)`)
        void this.captureInboundVideoActivitySnapshot().then((snapshot) => {
            this.recoveryActivitySnapshot = snapshot
        })
        this.recoveryTimeoutId = window.setTimeout(() => {
            this.recoveryTimeoutId = null
            void this.evaluateRecoveryTimeout(reason, recoveryTimeoutMs)
        }, recoveryTimeoutMs)
    }

    private async evaluateRecoveryTimeout(reason: "failed" | "disconnected", recoveryTimeoutMs: number) {
        if (!this.peer || this.closeRequested) {
            return
        }

        const connectionState = this.peer.connectionState
        const iceState = this.peer.iceConnectionState
        if (
            connectionState == "connected"
            || iceState == "connected"
            || iceState == "completed"
        ) {
            this.recoveryRestartIssued = false
            this.recoveryActivitySnapshot = null
            this.recoveryActivityExtensions = 0
            return
        }

        const hadActivity = await this.hasInboundVideoActivitySince(this.recoveryActivitySnapshot)
        if (hadActivity && this.recoveryActivityExtensions < 4) {
            this.recoveryActivityExtensions += 1
            this.recoveryActivitySnapshot = await this.captureInboundVideoActivitySnapshot()
            this.logger?.debug(
                `Suppressing ${reason} close because inbound video is still progressing (extension ${this.recoveryActivityExtensions}, ${recoveryTimeoutMs} ms)`
            )
            this.recoveryTimeoutId = window.setTimeout(() => {
                this.recoveryTimeoutId = null
                void this.evaluateRecoveryTimeout(reason, recoveryTimeoutMs)
            }, recoveryTimeoutMs)
            return
        }

        this.emitClose("failed")
    }

    async captureInboundVideoActivitySnapshot(): Promise<InboundVideoActivitySnapshot | null> {
        if (!this.videoReceiver) {
            return null
        }

        try {
            const stats = await this.videoReceiver.getStats()
            for (const value of stats.values()) {
                const statsType = "type" in value && typeof value.type == "string" ? value.type : null
                const mediaKind = "kind" in value && typeof value.kind == "string"
                    ? value.kind
                    : "mediaType" in value && typeof value.mediaType == "string"
                        ? value.mediaType
                        : null
                const isVideoInboundRtp = statsType == "inbound-rtp" && mediaKind == "video"
                if (!isVideoInboundRtp) {
                    continue
                }

                return {
                    packetsReceived: getStatNumber(value, "packetsReceived") ?? 0,
                    bytesReceived: getStatNumber(value, "bytesReceived") ?? 0,
                    framesDecoded: getStatNumber(value, "framesDecoded") ?? 0,
                    framesDecodedKnown: getStatNumber(value, "framesDecoded") != null
                }
            }
        } catch {
            return null
        }

        return null
    }

    async hasInboundVideoActivitySince(snapshot: InboundVideoActivitySnapshot | null): Promise<boolean> {
        const current = await this.captureInboundVideoActivitySnapshot()
        if (!snapshot || !current) {
            return false
        }

        if (snapshot.framesDecodedKnown && current.framesDecodedKnown) {
            if (current.framesDecoded >= snapshot.framesDecoded + 1) {
                return true
            }
        }

        return current.packetsReceived >= snapshot.packetsReceived + 1
            || current.bytesReceived >= snapshot.bytesReceived + 1024
    }

    private onConnectionStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnConnectionStateChange without a peer")
            return
        }

        let type: null | "fatal" | "recover" = null

        if (this.peer.connectionState == "connected") {
            this.clearDeferredIpv6Timer()
            this.deferredIpv6Candidates.length = 0
            this.clearDeferredPublicHostTimer()
            this.deferredPublicHostCandidates.length = 0
            type = "recover"
            this.clearConnectionTimeout()
            this.clearRecoveryTimeout()

            void this.reportSelectedCandidatePair()

            if (this.onconnect) {
                this.onconnect()
            }
            this.wasConnected = true
        } else if (this.peer.connectionState == "failed" && this.wasConnected) {
            this.armRecoveryTimeout("failed")
        } else if ((this.peer.connectionState == "failed" || this.peer.connectionState == "closed") && this.peer.iceGatheringState == "complete") {
            type = "fatal"
        }

        if (this.peer.connectionState == "closed") {
            this.emitClose(this.closeRequested ? "disconnect" : this.wasConnected ? "failed" : "failednoconnect")
        } else if (this.peer.connectionState == "failed" && !this.wasConnected && this.peer.iceGatheringState == "complete") {
            this.emitClose(this.closeRequested ? "disconnect" : "failednoconnect")
        }

        this.logger?.debug(`Changing Peer State to ${this.peer.connectionState}`, {
            type: type ?? undefined
        })
    }
    private onSignalingStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnSignalingStateChange without a peer")
            return
        }
        this.logger?.debug(`Changing Peer Signaling State to ${this.peer.signalingState}`)
    }
    private onIceConnectionStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnIceConnectionStateChange without a peer")
            return
        }
        if (this.peer.iceConnectionState == "connected" || this.peer.iceConnectionState == "completed") {
            this.clearConnectionTimeout()
            this.clearRecoveryTimeout()
        } else if ((this.peer.iceConnectionState == "disconnected" || this.peer.iceConnectionState == "failed") && this.wasConnected) {
            this.armRecoveryTimeout(this.peer.iceConnectionState == "failed" ? "failed" : "disconnected")
        }
        this.logger?.debug(`Changing Peer Ice State to ${this.peer.iceConnectionState}`)
    }
    private onIceGatheringStateChange() {
        if (!this.peer) {
            this.logger?.debug("OnIceGatheringStateChange without a peer")
            return
        }
        this.logger?.debug(`Changing Peer Ice Gathering State to ${this.peer.iceGatheringState}`)
    }

    private emitClose(shutdown: TransportShutdown) {
        if (this.closeDispatched) {
            return
        }

        this.clearConnectionTimeout()
        this.clearRecoveryTimeout()
        this.closeDispatched = true
        this.logger?.debug(
            `WebRTC emitClose shutdown=${shutdown} closeRequested=${this.closeRequested ? "yes" : "no"} wasConnected=${this.wasConnected ? "yes" : "no"} connectionState=${this.peer?.connectionState ?? "none"} iceState=${this.peer?.iceConnectionState ?? "none"}`
        )
        this.onclose?.(shutdown)
    }

    private channels: Array<TransportChannel | null> = []
    private initChannels() {
        if (!this.peer) {
            this.logger?.debug("Failed to initialize channel without peer")
            return
        }
        if (this.channels.length > 0) {
            this.logger?.debug("Already initialized channels")
            return
        }

        for (const channelRaw in TRANSPORT_CHANNEL_OPTIONS) {
            const channel = channelRaw as TransportChannelIdKey
            const options = TRANSPORT_CHANNEL_OPTIONS[channel]

            if (channel == "HOST_VIDEO") {
                const channel: VideoTrackTransportChannel = new WebRTCInboundTrackTransportChannel<"videotrack">(this.logger, "videotrack", "video", this.videoTrackHolder)
                this.channels[TransportChannelId.HOST_VIDEO] = channel
                continue
            }
            if (channel == "HOST_AUDIO") {
                const channel: AudioTrackTransportChannel = new WebRTCInboundTrackTransportChannel<"audiotrack">(this.logger, "audiotrack", "audio", this.audioTrackHolder)
                this.channels[TransportChannelId.HOST_AUDIO] = channel
                continue
            }

            const id = TransportChannelId[channel]
            const channelLabel = String(channel)
            const dataChannel = options.serverCreated ? null : this.peer.createDataChannel(channelLabel.toLowerCase(), {
                ordered: options.ordered,
                maxRetransmits: options.reliable ? undefined : 0
            })

            this.channels[id] = new WebRTCDataTransportChannel(channelLabel, dataChannel)
        }
    }

    private videoTrackHolder: TrackHolder = { ontrack: null, track: null }
    private videoReceiver: RTCRtpReceiver | null = null

    private audioTrackHolder: TrackHolder = { ontrack: null, track: null }

    private applyLowLatencyReceiverHints(receiver: RTCRtpReceiver, track: MediaStreamTrack) {
        const typedReceiver = receiver as LowLatencyRtpReceiver
        const applied: string[] = []
        const failed: string[] = []

        if ("jitterBufferTarget" in typedReceiver) {
            try {
                typedReceiver.jitterBufferTarget = 0
                applied.push("jitterBufferTarget=0")
            } catch (error) {
                failed.push(`jitterBufferTarget:${error instanceof Error ? error.name : "error"}`)
            }
        }

        if ("playoutDelayHint" in typedReceiver) {
            try {
                typedReceiver.playoutDelayHint = 0
                applied.push("playoutDelayHint=0")
            } catch (error) {
                failed.push(`playoutDelayHint:${error instanceof Error ? error.name : "error"}`)
            }
        }

        if (track.kind == "video" && "contentHint" in track) {
            try {
                track.contentHint = "motion"
                applied.push("contentHint=motion")
            } catch (error) {
                failed.push(`contentHint:${error instanceof Error ? error.name : "error"}`)
            }
        }

        const prefix = track.kind == "video" ? "webrtcVideoLowLatency" : "webrtcAudioLowLatency"
        this.receiverLatencyHintStats[`${prefix}Hints`] = applied.length > 0 ? applied.join(",") : "unsupported"
        if (failed.length > 0) {
            this.receiverLatencyHintStats[`${prefix}HintErrors`] = failed.join(",")
        } else {
            delete this.receiverLatencyHintStats[`${prefix}HintErrors`]
        }

        this.logger?.debug(`Receiver low-latency hints for ${track.kind}: ${applied.length > 0 ? applied.join(", ") : "unsupported"}${failed.length > 0 ? `; failed ${failed.join(", ")}` : ""}`)
    }

    private onTrack(event: RTCTrackEvent) {
        const track = event.track

        const receiver = event.receiver
        if (track.kind == "video") {
            this.videoReceiver = receiver
        }

        this.applyLowLatencyReceiverHints(receiver, track)

        this.logger?.debug(`Adding receiver: ${track.kind}, ${track.id}, ${track.label}`)

        if (track.kind == "video") {
            this.videoTrackHolder.track = track
            if (!this.videoTrackHolder.ontrack) {
                throw "No video track listener registered!"
            }
            this.videoTrackHolder.ontrack()
        } else if (track.kind == "audio") {
            this.audioTrackHolder.track = track
            if (!this.audioTrackHolder.ontrack) {
                throw "No audio track listener registered!"
            }
            this.audioTrackHolder.ontrack()
        }
    }

    // Handle data channels created by the remote peer (server)
    private onDataChannel(event: RTCDataChannelEvent) {
        const remoteChannel = event.channel
        const label = remoteChannel.label

        this.logger?.debug(`Received remote data channel: ${label}`)

        // Map the channel label to the corresponding TransportChannelId
        const channelKey = label.toUpperCase() as TransportChannelIdKey
        if (channelKey in TransportChannelId) {
            const id = TransportChannelId[channelKey]
            const existingChannel = this.channels[id]

            // If we already have a channel for this ID, replace its underlying RTCDataChannel
            // with the remote one so we can receive messages from the server
            if (existingChannel && existingChannel.type === "data") {
                this.logger?.debug(`Replacing underlying channel for ${label} with remote channel`);
                (existingChannel as WebRTCDataTransportChannel).replaceChannel(remoteChannel)
            } else {
                this.logger?.debug(`Creating new channel for ${label}`)
                this.channels[id] = new WebRTCDataTransportChannel(label, remoteChannel)
            }
        } else {
            this.logger?.debug(`Unknown remote data channel: ${label}`)
        }
    }

    async setupHostVideo(_setup: TransportVideoSetup): Promise<VideoCodecSupport> {
        // TODO: check transport type

        let capabilities
        if ("getCapabilities" in RTCRtpReceiver && (capabilities = RTCRtpReceiver.getCapabilities("video"))) {
            const codecs = emptyVideoCodecs()

            for (const codec in codecs) {
                const supportRequirements = CAPABILITIES_CODECS[codec]

                if (!supportRequirements) {
                    continue
                }

                let supported = false
                capabilityCodecLoop: for (const codecCapability of capabilities.codecs) {
                    if (codecCapability.mimeType != supportRequirements.mimeType) {
                        continue
                    }

                    for (const fmtpLine of supportRequirements.fmtpLine) {
                        if (!codecCapability.sdpFmtpLine?.includes(fmtpLine)) {
                            continue capabilityCodecLoop
                        }
                    }

                    supported = true
                    break
                }

                codecs[codec] = supported
            }

            return codecs
        } else {
            return maybeVideoCodecs()
        }
    }

    async setupHostAudio(_setup: TransportAudioSetup): Promise<void> {
        // TODO: check transport type
    }

    getChannel(id: TransportChannelIdValue): TransportChannel {
        const channel = this.channels[id]
        if (!channel) {
            this.logger?.debug("Failed to setup video without peer")
            throw `Failed to get channel because it is not yet initialized, Id: ${id}`
        }

        return channel
    }

    onconnect: (() => void) | null = null

    onclose: ((shutdown: TransportShutdown) => void) | null = null
    async close(): Promise<void> {
        this.logger?.debug(
            `Closing WebRTC Peer closeRequested=${this.closeRequested ? "yes" : "no"} wasConnected=${this.wasConnected ? "yes" : "no"} connectionState=${this.peer?.connectionState ?? "none"} iceState=${this.peer?.iceConnectionState ?? "none"}`
        )

        this.closeRequested = true
        this.clearConnectionTimeout()
        this.clearRecoveryTimeout()
        this.microphoneEnabled = false
        this.localMicrophoneSender = null
        this.localMicrophoneTransceiver = null
        this.microphoneUplinkNegotiated = null
        this.microphoneUplinkDirection = "unknown"
        this.lastMicrophoneOutboundSnapshot = null
        this.lastMicrophoneOutboundStats = {
            ...DEFAULT_MICROPHONE_OUTBOUND_STATS,
            timestampMs: Date.now()
        }
        this.rollingCounterSnapshots.clear()
        this.receiverLatencyHintStats = {}
        this.hasReceivedIpv4Candidate = false
        this.deferredIpv6Candidates.length = 0
        this.clearDeferredIpv6Timer()
        this.deferredPublicHostCandidates.length = 0
        this.clearDeferredPublicHostTimer()
        this.localPrivateIpv4Prefixes.clear()
        this.releaseLocalMicrophoneTrack()
        this.peer?.close()
    }

    async getStats(): Promise<Record<string, StatValue>> {
        const statsData: Record<string, StatValue> = {}
        const secondsToMs = (value: number | null | undefined): number | null => {
            if (value == null) {
                return null
            }

            return value * 1000
        }
        const averageSecondsCounterToMs = (total: number | null | undefined, count: number | null | undefined): number | null => {
            if (total == null) {
                return null
            }

            if (count != null && count > 0) {
                return (total / count) * 1000
            }

            return total * 1000
        }
        const windowedAverageSecondsCounterToMs = (
            snapshotKey: string,
            total: number | null | undefined,
            count: number | null | undefined
        ): number | null => {
            if (total == null) {
                return null
            }

            const safeCount = count != null && Number.isFinite(count) ? count : 0
            const previous = this.rollingCounterSnapshots.get(snapshotKey)
            this.rollingCounterSnapshots.set(snapshotKey, {
                total,
                count: safeCount
            })

            if (!previous) {
                return averageSecondsCounterToMs(total, safeCount)
            }

            const deltaTotal = total - previous.total
            const deltaCount = safeCount - previous.count
            if (deltaTotal >= 0 && deltaCount > 0) {
                return (deltaTotal / deltaCount) * 1000
            }

            return averageSecondsCounterToMs(total, safeCount)
        }

        if (this.peer) {
            statsData.webrtcConnectionState = this.peer.connectionState
            statsData.webrtcIceConnectionState = this.peer.iceConnectionState

            const peerStats = await this.peer.getStats()
            const routeStats = this.extractSelectedCandidatePairStats(peerStats)
            if (Object.keys(routeStats.stats).length > 0) {
                this.selectedRouteStats = routeStats.stats
            }
            Object.assign(statsData, this.selectedRouteStats)
        }
        Object.assign(statsData, this.receiverLatencyHintStats)

        if (!this.videoReceiver) {
            return statsData
        }
        const stats = await this.videoReceiver.getStats()
        for (const [key, value] of stats.entries()) {
            const statsType = "type" in value && typeof value.type == "string" ? value.type : null
            const mediaKind = "kind" in value && typeof value.kind == "string"
                ? value.kind
                : "mediaType" in value && typeof value.mediaType == "string"
                    ? value.mediaType
                    : null
            const isVideoInboundRtp = statsType == "inbound-rtp" && mediaKind == "video"

            if ("decoderImplementation" in value && value.decoderImplementation != null) {
                statsData.decoderImplementation = value.decoderImplementation
            }
            if ("frameWidth" in value && value.frameWidth != null) {
                statsData.videoWidth = value.frameWidth
            }
            if ("frameHeight" in value && value.frameHeight != null) {
                statsData.videoHeight = value.frameHeight
            }
            if ("framesPerSecond" in value && value.framesPerSecond != null) {
                statsData.webrtcFps = value.framesPerSecond
            }

            if (isVideoInboundRtp) {
                const jitterBufferEmittedCount = "jitterBufferEmittedCount" in value &&
                    typeof value.jitterBufferEmittedCount == "number" ?
                    value.jitterBufferEmittedCount :
                    null
                const framesDecoded = "framesDecoded" in value && typeof value.framesDecoded == "number" ?
                    value.framesDecoded :
                    null
                if (framesDecoded != null) {
                    statsData.webrtcFramesDecoded = framesDecoded
                }
                const framesAssembledFromMultiplePackets = "framesAssembledFromMultiplePackets" in value &&
                    typeof value.framesAssembledFromMultiplePackets == "number" ?
                    value.framesAssembledFromMultiplePackets :
                    null

                if ("jitterBufferDelay" in value && value.jitterBufferDelay != null && jitterBufferEmittedCount != null && jitterBufferEmittedCount > 0) {
                    const averageJitterBufferDelayMs = windowedAverageSecondsCounterToMs(
                        `${key}:jitterBufferDelay`,
                        value.jitterBufferDelay,
                        jitterBufferEmittedCount
                    )
                    if (averageJitterBufferDelayMs != null) {
                        statsData.webrtcJitterBufferDelayMs = averageJitterBufferDelayMs
                    }
                }
                if ("jitterBufferTargetDelay" in value && value.jitterBufferTargetDelay != null && jitterBufferEmittedCount != null && jitterBufferEmittedCount > 0) {
                    const averageJitterBufferTargetDelayMs = windowedAverageSecondsCounterToMs(
                        `${key}:jitterBufferTargetDelay`,
                        value.jitterBufferTargetDelay,
                        jitterBufferEmittedCount
                    )
                    if (averageJitterBufferTargetDelayMs != null) {
                        statsData.webrtcJitterBufferTargetDelayMs = averageJitterBufferTargetDelayMs
                    }
                }
                if ("jitterBufferMinimumDelay" in value && value.jitterBufferMinimumDelay != null) {
                    const jitterBufferMinimumDelayMs = secondsToMs(value.jitterBufferMinimumDelay)
                    if (jitterBufferMinimumDelayMs != null) {
                        statsData.webrtcJitterBufferMinimumDelayMs = jitterBufferMinimumDelayMs
                    }
                }
                if ("jitter" in value && value.jitter != null) {
                    const jitterMs = secondsToMs(value.jitter)
                    if (jitterMs != null) {
                        statsData.webrtcJitterMs = jitterMs
                    }
                }
                if ("currentRoundTripTime" in value && value.currentRoundTripTime != null) {
                    const currentRoundTripTimeMs = secondsToMs(value.currentRoundTripTime)
                    if (currentRoundTripTimeMs != null) {
                        statsData.webrtcCurrentRoundTripTimeMs = currentRoundTripTimeMs
                    }
                }
                if ("roundTripTime" in value && value.roundTripTime != null) {
                    const remoteRoundTripTimeMs = secondsToMs(value.roundTripTime)
                    if (remoteRoundTripTimeMs != null) {
                        statsData.webrtcRemoteRoundTripTimeMs = remoteRoundTripTimeMs
                    }
                }
                if ("totalDecodeTime" in value && value.totalDecodeTime != null && framesDecoded != null && framesDecoded > 0) {
                    const averageDecodeTimeMs = windowedAverageSecondsCounterToMs(
                        `${key}:totalDecodeTime`,
                        value.totalDecodeTime,
                        framesDecoded
                    )
                    if (averageDecodeTimeMs != null) {
                        statsData.webrtcTotalDecodeTimeMs = averageDecodeTimeMs
                    }
                }
                if ("totalAssemblyTime" in value && value.totalAssemblyTime != null && framesAssembledFromMultiplePackets != null && framesAssembledFromMultiplePackets > 0) {
                    const averageAssemblyTimeMs = windowedAverageSecondsCounterToMs(
                        `${key}:totalAssemblyTime`,
                        value.totalAssemblyTime,
                        framesAssembledFromMultiplePackets
                    )
                    if (averageAssemblyTimeMs != null) {
                        statsData.webrtcTotalAssemblyTimeMs = averageAssemblyTimeMs
                    }
                }
                if ("totalProcessingDelay" in value && value.totalProcessingDelay != null && framesDecoded != null && framesDecoded > 0) {
                    const averageProcessingDelayMs = windowedAverageSecondsCounterToMs(
                        `${key}:totalProcessingDelay`,
                        value.totalProcessingDelay,
                        framesDecoded
                    )
                    if (averageProcessingDelayMs != null) {
                        statsData.webrtcTotalProcessingDelayMs = averageProcessingDelayMs
                    }
                }
                if ("freezeCount" in value && value.freezeCount != null) {
                    statsData.webrtcFreezeCount = value.freezeCount
                }
                if ("pauseCount" in value && value.pauseCount != null) {
                    statsData.webrtcPauseCount = value.pauseCount
                }
            }
            if ("packetsReceived" in value && value.packetsReceived != null) {
                statsData.webrtcPacketsReceived = value.packetsReceived
            }
            if ("bytesReceived" in value && value.bytesReceived != null) {
                statsData.webrtcBytesReceived = value.bytesReceived
            }
            if ("packetsLost" in value && value.packetsLost != null) {
                statsData.webrtcPacketsLost = value.packetsLost
            }
            if ("framesDropped" in value && value.framesDropped != null) {
                statsData.webrtcFramesDropped = value.framesDropped
            }
            if ("keyFramesDecoded" in value && value.keyFramesDecoded != null) {
                statsData.webrtcKeyFramesDecoded = value.keyFramesDecoded
            }
            if ("nackCount" in value && value.nackCount != null) {
                statsData.webrtcNackCount = value.nackCount
            }
        }

        return statsData
    }

    private async reportSelectedCandidatePair(attempt = 0): Promise<void> {
        if (!this.peer) {
            return
        }

        const stats = await this.peer.getStats()
        const routeStats = this.extractSelectedCandidatePairStats(stats)
        if (!routeStats.summary) {
            if (attempt < 5) {
                setTimeout(() => {
                    void this.reportSelectedCandidatePair(attempt + 1)
                }, 250)
            }
            return
        }

        this.selectedRouteStats = routeStats.stats
        if (routeStats.summary != this.lastReportedRouteSummary) {
            this.lastReportedRouteSummary = routeStats.summary
            this.logger?.debug(routeStats.summary)
        }
    }

    private extractSelectedCandidatePairStats(stats: RTCStatsReport): { summary: string | null, stats: Record<string, StatValue> } {
        const selectedPair = this.findSelectedCandidatePair(stats)
        if (!selectedPair) {
            return {
                summary: null,
                stats: {}
            }
        }

        const localCandidateId = getStatString(selectedPair, "localCandidateId")
        const remoteCandidateId = getStatString(selectedPair, "remoteCandidateId")

        const localCandidate = localCandidateId ? stats.get(localCandidateId) as RTCStatsLike | undefined : undefined
        const remoteCandidate = remoteCandidateId ? stats.get(remoteCandidateId) as RTCStatsLike | undefined : undefined

        const localCandidateType = getStatString(localCandidate ?? null, "candidateType") ?? "unknown"
        const remoteCandidateType = getStatString(remoteCandidate ?? null, "candidateType") ?? "unknown"
        const localProtocol = getStatString(localCandidate ?? null, "protocol") ?? "unknown"
        const remoteProtocol = getStatString(remoteCandidate ?? null, "protocol") ?? "unknown"
        const localAddressFamily = inferCandidateAddressFamily(localCandidate ?? null) ?? "unknown"
        const remoteAddressFamily = inferCandidateAddressFamily(remoteCandidate ?? null) ?? "unknown"
        const relayProtocol = getStatString(localCandidate ?? null, "relayProtocol") ?? getStatString(remoteCandidate ?? null, "relayProtocol")

        const usesRelay = localCandidateType == "relay" || remoteCandidateType == "relay"
        const route = usesRelay ? "relay" : "direct"
        const selectedPairState = getStatString(selectedPair, "state")
        const currentRoundTripTime = getStatNumber(selectedPair, "currentRoundTripTime")

        const localDescription = this.describeCandidate(localCandidate ?? null)
        const remoteDescription = this.describeCandidate(remoteCandidate ?? null)
        const summary = usesRelay
            ? `WebRTC route: relay via TURN (local=${localDescription}, remote=${remoteDescription})`
            : `WebRTC route: direct peer-to-peer (local=${localDescription}, remote=${remoteDescription})`

        const routeStats: Record<string, StatValue> = {
            webrtcRoute: route,
            webrtcLocalCandidateType: localCandidateType,
            webrtcRemoteCandidateType: remoteCandidateType,
            webrtcLocalProtocol: localProtocol,
            webrtcRemoteProtocol: remoteProtocol,
            webrtcLocalAddressFamily: localAddressFamily,
            webrtcRemoteAddressFamily: remoteAddressFamily,
        }

        if (relayProtocol) {
            routeStats.webrtcRelayProtocol = relayProtocol
        }
        if (selectedPairState) {
            routeStats.webrtcSelectedPairState = selectedPairState
        }
        if (currentRoundTripTime != null) {
            routeStats.webrtcSelectedPairRttMs = currentRoundTripTime * 1000
        }

        return {
            summary,
            stats: routeStats
        }
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
}

type TrackHolder = {
    ontrack: (() => void) | null
    track: MediaStreamTrack | null
}

// This receives track data
class WebRTCInboundTrackTransportChannel<T extends string> implements TrackTransportChannel {
    type: T

    canReceive: boolean = true
    canSend: boolean = false

    private logger: Logger | null

    private label: string
    private trackHolder: TrackHolder

    constructor(logger: Logger | null, type: T, label: string, trackHolder: TrackHolder) {
        this.logger = logger

        this.type = type
        this.label = label
        this.trackHolder = trackHolder

        this.trackHolder.ontrack = this.onTrack.bind(this)
    }
    setTrack(_track: MediaStreamTrack | null): void {
        throw "WebRTCInboundTrackTransportChannel cannot addTrack"
    }

    private onTrack() {
        const track = this.trackHolder.track
        if (!track) {
            this.logger?.debug("WebRTC TrackHolder.track is null!")
            return
        }

        for (const listener of this.trackListeners) {
            listener(track)
        }
    }


    private trackListeners: Array<(track: MediaStreamTrack) => void> = []
    addTrackListener(listener: (track: MediaStreamTrack) => void): void {
        if (this.trackHolder.track) {
            listener(this.trackHolder.track)
        }
        this.trackListeners.push(listener)
    }
    removeTrackListener(listener: (track: MediaStreamTrack) => void): void {
        const index = this.trackListeners.indexOf(listener)
        if (index != -1) {
            this.trackListeners.splice(index, 1)
        }
    }
}

class WebRTCDataTransportChannel implements DataTransportChannel {
    type: "data" = "data"

    canReceive: boolean = true
    canSend: boolean = true

    private label: string
    private channel: RTCDataChannel | null
    private boundOnMessage: (event: MessageEvent) => void

    constructor(label: string, channel: RTCDataChannel | null) {
        this.label = label
        this.channel = channel
        this.boundOnMessage = this.onMessage.bind(this)

        this.channel?.addEventListener("message", this.boundOnMessage)
    }

    // Replace the underlying channel with a new one (e.g., from remote peer)
    // This is used when we receive a data channel from the server that should
    // replace our locally created one for receiving messages
    replaceChannel(newChannel: RTCDataChannel): void {
        // Remove listener from old channel
        this.channel?.removeEventListener("message", this.boundOnMessage)
        // Add listener to new channel
        this.channel = newChannel
        this.channel.addEventListener("message", this.boundOnMessage)
    }

    private sendQueue: Array<ArrayBuffer> = []
    send(message: ArrayBuffer): void {
        if (debugTransportPacketsEnabled()) {
            console.debug(this.label, message)
        }

        if (!this.channel) {
            throw `Failed to send message on channel ${this.label}`
        }

        if (this.channel.readyState != "open") {
            if (debugTransportPacketsEnabled()) {
                console.debug(`Tried sending packet to ${this.label} with readyState ${this.channel.readyState}. Buffering it for the future.`)
            }
            this.sendQueue.push(message)
        } else {
            this.tryDequeueSendQueue()
            this.channel.send(message)
        }
    }
    private tryDequeueSendQueue() {
        for (const message of this.sendQueue) {
            this.channel?.send(message)
        }
        this.sendQueue.length = 0
    }

    private onMessage(event: MessageEvent) {
        const data = event.data
        if (!(data instanceof ArrayBuffer)) {
            console.warn(`received text data on webrtc channel ${this.label}`)
            return
        }

        for (const listener of this.receiveListeners) {
            listener(event.data)
        }
    }
    private receiveListeners: Array<(data: ArrayBuffer) => void> = []
    addReceiveListener(listener: (data: ArrayBuffer) => void): void {
        this.receiveListeners.push(listener)
    }
    removeReceiveListener(listener: (data: ArrayBuffer) => void): void {
        const index = this.receiveListeners.indexOf(listener)
        if (index != -1) {
            this.receiveListeners.splice(index, 1)
        }
    }
    estimatedBufferedBytes(): number | null {
        return this.channel?.bufferedAmount ?? null
    }
}
