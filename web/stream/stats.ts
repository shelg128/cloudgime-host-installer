import { StreamerStatsUpdate, TransportChannelId } from "../api_bindings.js"
import { BIG_BUFFER, ByteBuffer } from "./buffer.js"
import { Logger } from "./log.js"
import { Pipe } from "./pipeline/index.js"
import { DataTransportChannel, Transport } from "./transport/index.js"

export type StatValue = string | number
function getLocalStatsPollIntervalMs(): number {
    return typeof navigator != "undefined" && (navigator.maxTouchPoints || 0) > 0
        ? 1500
        : 500
}

export type StreamStatsData = {
    videoCodec: string | null
    videoWidth: number | null
    videoHeight: number | null
    videoFps: number | null
    videoPipeline: string | null
    audioPipeline: string | null
    hdrEnabled: boolean | null
    streamerRttMs: number | null
    streamerRttVarianceMs: number | null
    minHostProcessingLatencyMs: number | null
    maxHostProcessingLatencyMs: number | null
    avgHostProcessingLatencyMs: number | null
    minStreamerProcessingTimeMs: number | null
    maxStreamerProcessingTimeMs: number | null
    avgStreamerProcessingTimeMs: number | null
    streamerOutputFps: number | null
    browserRtt: number | null
    transport: Record<string, StatValue>
    video: Record<string, StatValue>
    audio: Record<string, StatValue>
}

function num(value: number | null | undefined, suffix?: string): string | null {
    if (value == null) {
        return null
    } else {
        return `${value.toFixed(2)}${suffix ?? ""}`
    }
}

function integer(value: number | null | undefined, suffix?: string): string | null {
    if (value == null) {
        return null
    }

    return `${Math.round(value)}${suffix ?? ""}`
}

function getNumberStat(stats: Record<string, StatValue>, key: string): number | null {
    const value = stats[key]
    return typeof value == "number" ? value : null
}

function getStringStat(stats: Record<string, StatValue>, key: string): string | null {
    const value = stats[key]
    return typeof value == "string" ? value : null
}

function getDisplayableTransportJitterMs(
    jitterMs: number | null,
    effectiveBufferMs: number | null,
    networkRttMs: number | null
): number | null {
    if (jitterMs == null || !Number.isFinite(jitterMs) || jitterMs < 0) {
        return null
    }

    let noisyThresholdMs = 1200
    if (effectiveBufferMs != null) {
        noisyThresholdMs = Math.max(noisyThresholdMs, effectiveBufferMs * 4)
    }
    if (networkRttMs != null) {
        noisyThresholdMs = Math.max(noisyThresholdMs, networkRttMs * 6)
    }

    if (jitterMs > noisyThresholdMs) {
        return null
    }

    return jitterMs
}

function getGameplayLatencyMetrics(statsData: StreamStatsData) {
    const pairRttMs = getNumberStat(statsData.transport, "webrtcSelectedPairRttMs")
    const currentRttMs = getNumberStat(statsData.transport, "webrtcCurrentRoundTripTimeMs")
    const remoteRttMs = getNumberStat(statsData.transport, "webrtcRemoteRoundTripTimeMs")
    const networkRttMs = pairRttMs ?? currentRttMs ?? remoteRttMs ?? statsData.streamerRttMs
    const networkOneWayMs = networkRttMs != null ? Math.max(0, networkRttMs / 2) : null
    const jitterBufferDelayMs = getNumberStat(statsData.transport, "webrtcJitterBufferDelayMs")
    const jitterBufferTargetDelayMs = getNumberStat(statsData.transport, "webrtcJitterBufferTargetDelayMs")
    // `jitterBufferTargetDelay` is a target chosen by the browser and can remain high even when
    // the currently buffered playout delay is already low again. For gaming diagnostics, prefer
    // the measured jitter buffer delay and only fall back to the target when the actual value is
    // unavailable.
    const effectiveBufferMs = jitterBufferDelayMs ?? jitterBufferTargetDelayMs
    const totalDecodeTimeMs = getNumberStat(statsData.transport, "webrtcTotalDecodeTimeMs")
    const totalProcessingDelayMs = getNumberStat(statsData.transport, "webrtcTotalProcessingDelayMs")
    const decodeStageMs = totalDecodeTimeMs != null
        ? totalDecodeTimeMs
        : totalProcessingDelayMs != null && effectiveBufferMs != null
            ? Math.max(0, totalProcessingDelayMs - effectiveBufferMs)
            : totalProcessingDelayMs

    let playEstimateMs: number | null = null
    if (networkOneWayMs != null) {
        let sum = networkOneWayMs
        let components = 1
        if (statsData.avgHostProcessingLatencyMs != null) {
            sum += statsData.avgHostProcessingLatencyMs
            components += 1
        }
        if (effectiveBufferMs != null) {
            sum += effectiveBufferMs
            components += 1
        }
        if (decodeStageMs != null) {
            sum += decodeStageMs
            components += 1
        }
        if (components >= 3) {
            playEstimateMs = sum
        }
    }

    return {
        networkRttMs,
        effectiveBufferMs,
        decodeStageMs,
        playEstimateMs
    }
}

function getGameplayExperienceLabel(statsData: StreamStatsData): string {
    const metrics = getGameplayLatencyMetrics(statsData)
    const route = getStringStat(statsData.transport, "webrtcRoute")
    const packetsReceived = getNumberStat(statsData.transport, "webrtcPacketsReceived")
    const packetsLost = getNumberStat(statsData.transport, "webrtcPacketsLost")
    const packetLossPercent = packetsReceived != null && packetsLost != null && (packetsReceived + packetsLost) > 0
        ? (packetsLost / (packetsReceived + packetsLost)) * 100
        : null

    if (route == "websocket") {
        return "Fallback"
    }
    if (route == "relay") {
        return "Playable"
    }
    if (route != "direct") {
        return "Checking"
    }

    const playMs = metrics.playEstimateMs
    const bufferMs = metrics.effectiveBufferMs
    const loss = packetLossPercent ?? 0

    if ((playMs != null && playMs <= 45) && (bufferMs == null || bufferMs <= 20) && loss < 0.3) {
        return "Excellent"
    }
    if ((playMs != null && playMs <= 65) && (bufferMs == null || bufferMs <= 35) && loss < 0.8) {
        return "Good"
    }
    if ((playMs != null && playMs <= 95) && (bufferMs == null || bufferMs <= 55) && loss < 1.5) {
        return "Playable"
    }
    if ((playMs != null && playMs > 150) || (bufferMs != null && bufferMs > 120) || loss >= 3) {
        return "Unstable"
    }
    if (playMs != null || bufferMs != null) {
        return "Laggy"
    }

    return "Checking"
}

function compactLine(label: string, parts: Array<string | null | undefined>): string | null {
    const filtered = parts.filter((part): part is string => part != null && part !== "")
    if (filtered.length === 0) {
        return null
    }

    return `${label}: ${filtered.join(" | ")}`
}

export function streamStatsToText(statsData: StreamStatsData): string {
    const connectionState = getStringStat(statsData.transport, "webrtcConnectionState")
    const iceState = getStringStat(statsData.transport, "webrtcIceConnectionState")
    const route = getStringStat(statsData.transport, "webrtcRoute")
    const localCandidate = getStringStat(statsData.transport, "webrtcLocalCandidateType")
    const localProtocol = getStringStat(statsData.transport, "webrtcLocalProtocol")
    const localFamily = getStringStat(statsData.transport, "webrtcLocalAddressFamily")
    const remoteCandidate = getStringStat(statsData.transport, "webrtcRemoteCandidateType")
    const remoteProtocol = getStringStat(statsData.transport, "webrtcRemoteProtocol")
    const remoteFamily = getStringStat(statsData.transport, "webrtcRemoteAddressFamily")
    const relayProtocol = getStringStat(statsData.transport, "webrtcRelayProtocol")
    const metrics = getGameplayLatencyMetrics(statsData)
    const gameplayLabel = getGameplayExperienceLabel(statsData)
    const networkRttMs = metrics.networkRttMs

    const packetsReceived = getNumberStat(statsData.transport, "webrtcPacketsReceived")
    const packetsLost = getNumberStat(statsData.transport, "webrtcPacketsLost")
    const framesDropped = getNumberStat(statsData.transport, "webrtcFramesDropped")
    const nackCount = getNumberStat(statsData.transport, "webrtcNackCount")
    const jitterMs = getNumberStat(statsData.transport, "webrtcJitterMs")
    const displayJitterMs = getDisplayableTransportJitterMs(jitterMs, metrics.effectiveBufferMs, networkRttMs)
    const totalProcessingDelayMs = getNumberStat(statsData.transport, "webrtcTotalProcessingDelayMs")
    const decoderImplementation = getStringStat(statsData.transport, "decoderImplementation")

    const packetLossPercent = packetsReceived != null && packetsLost != null && (packetsReceived + packetsLost) > 0
        ? (packetsLost / (packetsReceived + packetsLost)) * 100
        : null

    const lines = [
        "status:",
        compactLine("route", [
            route,
            connectionState ? `peer ${connectionState}` : null,
            iceState ? `ice ${iceState}` : null,
            networkRttMs != null ? `pair ${integer(networkRttMs, " ms")}` : null,
            localCandidate && remoteCandidate
                ? `${localCandidate}/${localProtocol ?? "?"}${localFamily == "v4" || localFamily == "v6" ? localFamily : ""} -> ${remoteCandidate}/${remoteProtocol ?? "?"}${remoteFamily == "v4" || remoteFamily == "v6" ? remoteFamily : ""}`
                : null,
            relayProtocol ? `relay ${relayProtocol}` : null
        ]),
        compactLine("video", [
            statsData.videoCodec ?? null,
            statsData.videoWidth != null && statsData.videoHeight != null
                ? `${statsData.videoWidth}x${statsData.videoHeight}`
                : null,
            statsData.videoFps != null ? `${Math.round(statsData.videoFps)} fps` : null,
            statsData.hdrEnabled === true ? "HDR on" : statsData.hdrEnabled === false ? "HDR off" : null,
            decoderImplementation,
        ]),
        compactLine("gaming", [
            gameplayLabel,
            metrics.playEstimateMs != null ? `play ${integer(metrics.playEstimateMs, " ms")}` : null,
            networkRttMs != null ? `net ${integer(networkRttMs, " ms")} RTT` : null,
            metrics.effectiveBufferMs != null ? `buffer ${integer(metrics.effectiveBufferMs, " ms")}` : null
        ]),
        compactLine("latency", [
            metrics.playEstimateMs != null ? `play ${integer(metrics.playEstimateMs, " ms")}` : null,
            networkRttMs != null ? `net ${integer(networkRttMs, " ms")} RTT` : null,
            statsData.avgHostProcessingLatencyMs != null ? `host ${integer(statsData.avgHostProcessingLatencyMs, " ms")}` : null,
            metrics.effectiveBufferMs != null ? `buffer ${integer(metrics.effectiveBufferMs, " ms")}` : null,
            metrics.decodeStageMs != null ? `decode ${integer(metrics.decodeStageMs, " ms")}` : null,
            totalProcessingDelayMs != null ? `process ${integer(totalProcessingDelayMs, " ms")}` : null,
            statsData.streamerRttMs != null && networkRttMs == null ? `stream ${integer(statsData.streamerRttMs, " ms")}` : null,
            statsData.browserRtt != null && networkRttMs == null ? `browser ${integer(statsData.browserRtt, " ms")}` : null
        ]),
        compactLine("quality", [
            metrics.effectiveBufferMs != null ? `buffer ${integer(metrics.effectiveBufferMs, " ms")}` : null,
            packetsLost != null
                ? packetLossPercent != null
                    ? `loss ${Math.round(packetsLost)} (${packetLossPercent.toFixed(2)}%)`
                    : `loss ${Math.round(packetsLost)}`
                : null,
            framesDropped != null ? `dropped ${Math.round(framesDropped)}` : null,
            nackCount != null ? `nack ${Math.round(nackCount)}` : null,
            displayJitterMs != null ? `jitter ${num(displayJitterMs, " ms")}` : null,
            packetsReceived != null ? `recv ${Math.round(packetsReceived)}` : null
        ]),
    ].filter((line): line is string => line != null)

    return `${lines.join("\n")}\n`
}

export class StreamStats {

    private logger: Logger | null = null

    private enabled: boolean = false
    private transport: Transport | null = null
    private statsChannel: DataTransportChannel | null = null
    private updateIntervalId: number | null = null
    private readonly onRawDataBound = this.onRawData.bind(this)

    private videoPipe: Pipe | null = null
    private audioPipe: Pipe | null = null
    private statsData: StreamStatsData = {
        videoCodec: null,
        videoWidth: null,
        videoHeight: null,
        videoFps: null,
        videoPipeline: null,
        audioPipeline: null,
        hdrEnabled: null,
        streamerRttMs: null,
        streamerRttVarianceMs: null,
        minHostProcessingLatencyMs: null,
        maxHostProcessingLatencyMs: null,
        avgHostProcessingLatencyMs: null,
        minStreamerProcessingTimeMs: null,
        maxStreamerProcessingTimeMs: null,
        avgStreamerProcessingTimeMs: null,
        streamerOutputFps: null,
        browserRtt: null,
        transport: {},
        video: {},
        audio: {}
    }

    constructor(logger?: Logger) {
        if (logger) {
            this.logger = logger
        }
    }

    private resetTransportStats() {
        this.statsData.streamerRttMs = null
        this.statsData.streamerRttVarianceMs = null
        this.statsData.minHostProcessingLatencyMs = null
        this.statsData.maxHostProcessingLatencyMs = null
        this.statsData.avgHostProcessingLatencyMs = null
        this.statsData.minStreamerProcessingTimeMs = null
        this.statsData.maxStreamerProcessingTimeMs = null
        this.statsData.avgStreamerProcessingTimeMs = null
        this.statsData.browserRtt = null
        this.statsData.transport = {}
        this.statsData.video = {}
        this.statsData.audio = {}
    }

    setTransport(transport: Transport | null) {
        const transportChanged = this.transport !== transport
        if (transportChanged) {
            if (this.statsChannel) {
                this.statsChannel.removeReceiveListener(this.onRawDataBound)
                this.statsChannel = null
            }
            this.resetTransportStats()
        }

        this.transport = transport

        this.checkEnabled()
    }
    private checkEnabled() {
        if (this.enabled) {
            if (this.statsChannel) {
                this.statsChannel.removeReceiveListener(this.onRawDataBound)
                this.statsChannel = null
            }

            if (!this.statsChannel && this.transport) {
                const channel = this.transport.getChannel(TransportChannelId.STATS)
                if (channel.type != "data") {
                    this.logger?.debug(`Failed initialize debug transport channel because type is "${channel.type}" and not "data"`)
                    return
                }
                channel.addReceiveListener(this.onRawDataBound)
                this.statsChannel = channel
            }
            if (this.updateIntervalId == null) {
                void this.updateLocalStats()
                this.updateIntervalId = setInterval(this.updateLocalStats.bind(this), getLocalStatsPollIntervalMs())
            }
        } else {
            if (this.updateIntervalId != null) {
                clearInterval(this.updateIntervalId)
                this.updateIntervalId = null
            }
        }
    }

    setEnabled(enabled: boolean) {
        this.enabled = enabled

        this.checkEnabled()
    }
    isEnabled(): boolean {
        return this.enabled
    }
    toggle() {
        this.setEnabled(!this.isEnabled())
    }

    private buffer: ByteBuffer = BIG_BUFFER
    private onRawData(data: ArrayBuffer) {
        this.buffer.reset()
        this.buffer.putU8Array(new Uint8Array(data))

        this.buffer.flip()

        const textLength = this.buffer.getU16()
        const text = this.buffer.getUtf8Raw(textLength)

        const json: StreamerStatsUpdate = JSON.parse(text)
        this.onMessage(json)
    }
    private onMessage(msg: StreamerStatsUpdate) {
        if ("Rtt" in msg) {
            this.statsData.streamerRttMs = msg.Rtt.rtt_ms
            this.statsData.streamerRttVarianceMs = msg.Rtt.rtt_variance_ms
        } else if ("Video" in msg) {
            if (msg.Video.host_processing_latency) {
                this.statsData.minHostProcessingLatencyMs = msg.Video.host_processing_latency.min_host_processing_latency_ms
                this.statsData.maxHostProcessingLatencyMs = msg.Video.host_processing_latency.max_host_processing_latency_ms
                this.statsData.avgHostProcessingLatencyMs = msg.Video.host_processing_latency.avg_host_processing_latency_ms
            } else {
                this.statsData.minHostProcessingLatencyMs = null
                this.statsData.maxHostProcessingLatencyMs = null
                this.statsData.avgHostProcessingLatencyMs = null
            }

            this.statsData.minStreamerProcessingTimeMs = msg.Video.min_streamer_processing_time_ms
            this.statsData.maxStreamerProcessingTimeMs = msg.Video.max_streamer_processing_time_ms
            this.statsData.avgStreamerProcessingTimeMs = msg.Video.avg_streamer_processing_time_ms
            this.statsData.streamerOutputFps = msg.Video.streamer_output_fps
        } else if ("BrowserRtt" in msg) {
            this.statsData.browserRtt = msg.BrowserRtt.rtt_ms
        }
    }

    private async updateLocalStats() {
        Promise.all([
            this.updateTransportStats(),
            this.updateVideoStats(),
            this.updateAudioStats(),
        ])
    }
    private async updateTransportStats() {
        if (!this.transport) {
            console.debug("Cannot query stats without transport")
            return
        }

        const stats = await this.transport?.getStats()
        this.statsData.transport = { ...stats }
    }
    private async updateVideoStats() {
        const stats = {}

        if (this.videoPipe && this.videoPipe.reportStats) {
            this.videoPipe.reportStats(stats)
        }

        this.statsData.video = stats
    }
    private async updateAudioStats() {
        const stats = {}

        if (this.audioPipe && this.audioPipe.reportStats) {
            this.audioPipe.reportStats(stats)
        }

        this.statsData.audio = stats
    }

    setVideoInfo(codec: string, width: number, height: number, fps: number) {
        this.statsData.videoCodec = codec
        this.statsData.videoWidth = width
        this.statsData.videoHeight = height
        this.statsData.videoFps = fps
    }
    setVideoPipeline(name: string, pipe: Pipe | null) {
        this.statsData.videoPipeline = name
        this.videoPipe = pipe
    }
    setAudioPipeline(name: string, pipe: Pipe | null) {
        this.statsData.audioPipeline = name
        this.audioPipe = pipe
    }
    setHdrEnabled(enabled: boolean) {
        this.statsData.hdrEnabled = enabled
    }

    getCurrentStats(): StreamStatsData {
        const data = {}
        Object.assign(data, this.statsData)
        return data as StreamStatsData
    }
}
