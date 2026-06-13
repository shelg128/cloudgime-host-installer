import { Settings } from "./component/settings_menu.js"
import CONFIG from "./config.js"

const trueDefaultSettings: Settings =

// When updated, update the README
{
    // possible values: "left", "right", "up", "down"
    "sidebarEdge": "left",
    "uiLanguage": "en",
    "bitrate": 6000,
    "adaptiveBitrate": true,
    "packetSize": 960,
    "fps": 60,
    "videoFrameQueueSize": 1,
    // possible values: "720p", "1080p", "1440p", "4k", "native", "custom"
    "videoSize": "native",
    // only works if videoSize=custom
    "videoSizeCustom": {
        "width": 1920,
        "height": 1080
    },
    // possible values: "h264", "h265", "av1", "auto"
    "videoCodec": "h264",
    "forceVideoElementRenderer": false,
    "canvasRenderer": false,
    // Canvas only: when true, draw only on requestAnimationFrame (stable, may add ~0–17 ms). When false, draw on frame submit (low latency).
    "canvasVsync": false,
    "playAudioLocal": false,
    "audioSampleQueueSize": 8,
    // possible values: "highres", "normal"
    "mouseScrollMode": "highres",
    "controllerConfig": {
        "invertAB": false,
        "invertXY": false,
        // possible values: null or a number, example: 60, 120
        "sendIntervalOverride": null
    },
    // possible values: "auto", "webrtc", "websocket"
    "dataTransport": "webrtc",
    "toggleFullscreenWithKeybind": false,
    // possible values: "standard", "old"
    "pageStyle": "standard",
    "hdr": false,
    "useSelectElementPolyfill": false
}

function assignIfMissing(target: any, source: any) {
    for (const key in source) {
        if (!(key in target)) {
            target[key] = source[key]
        }
    }
}

const defaultSettings = {} as Settings

Object.assign(defaultSettings, trueDefaultSettings)
if (CONFIG?.default_settings) {
    Object.assign(defaultSettings, CONFIG.default_settings)

    // Just in case, i don't know if missing values will cause errors
    assignIfMissing(defaultSettings.controllerConfig, trueDefaultSettings.controllerConfig)
    assignIfMissing(defaultSettings.videoSizeCustom, trueDefaultSettings.videoSizeCustom)
}

export default defaultSettings as Settings
