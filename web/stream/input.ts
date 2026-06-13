import { StreamCapabilities, StreamControllerCapabilities, StreamMouseButton, TransportChannelId } from "../api_bindings.js"
import { ByteBuffer, I16_MAX, U16_MAX, U8_MAX } from "./buffer.js"
import { areGamepadStatesEqual, ControllerConfig, emptyGamepadState, extractGamepadState, GamepadState, getGamepadAdvertisement, getVirtualControllerAdvertisement } from "./gamepad.js"
import { convertToKey, convertToModifiers } from "./keyboard.js"
import { convertToButton } from "./mouse.js"
import { DataTransportChannel, Transport, TransportChannelIdKey, TransportChannelIdValue } from "./transport/index.js"

// Smooth scrolling multiplier
const TOUCH_HIGH_RES_SCROLL_MULTIPLIER = 10
// Normal scrolling multiplier
const TOUCH_SCROLL_MULTIPLIER = 1
// Distance until a touch is 100% a click
const TOUCH_AS_CLICK_MAX_DISTANCE = 30
// Time till it's registered as a click, else it might be scrolling
const TOUCH_AS_CLICK_MIN_TIME_MS = 100
// Everything greater than this is a right click
const TOUCH_AS_CLICK_MAX_TIME_MS = 300
// How much to move to open up the screen keyboard when having three touches at the same time
const TOUCHES_AS_KEYBOARD_DISTANCE = 100
const I16_MIN = -I16_MAX - 1
const TOUCH_MOUSE_MOVE_PRECISION_MIN_FACTOR = 1
const TOUCH_MOUSE_MOVE_PRECISION_FULL_DISTANCE = 26
const TOUCH_MOUSE_MOVE_IGNORE_DISTANCE = 0.08
const TOUCH_MOUSE_MOVE_MAX_STEP_DISTANCE = 48
const TOUCH_NATIVE_RELATIVE_PRECISION_MIN_FACTOR = 0.68
const TOUCH_NATIVE_RELATIVE_PRECISION_FULL_DISTANCE = 18
const TOUCH_NATIVE_RELATIVE_MAX_STREAM_SCALE = 1.4
const TOUCH_NATIVE_RELATIVE_DEADZONE = 0.06
const TOUCH_NATIVE_RELATIVE_MAX_STEP = 18
const RELATIVE_MOUSE_EMA_ALPHA_SLOW = 1
const RELATIVE_MOUSE_EMA_ALPHA_FAST = 1
const RELATIVE_MOUSE_EMA_FAST_DISTANCE = 18
const RELATIVE_MOUSE_DEADZONE = 0.18
const POINTER_LOCK_RELATIVE_MOUSE_GAIN = 1.15
const POINTER_LOCK_RELATIVE_MOUSE_MAX_STEP = 240
const MOUSE_BATCHING_INTERVAL_MS = 0
const RELATIVE_MOUSE_PACKET_SIZE = 1 + 2 + 2
const ABSOLUTE_MOUSE_PACKET_SIZE = 1 + 2 + 2 + 2 + 2
const TOUCH_MOUSE_EVENT_SUPPRESSION_MS = 750

const CONTROLLER_RUMBLE_INTERVAL_MS = 60
const DEFAULT_VIRTUAL_CONTROLLER_ADVERTISEMENT = getVirtualControllerAdvertisement("xbox")

export const STREAM_TOUCH_SENSITIVITY_MIN = 0.6
export const STREAM_TOUCH_SENSITIVITY_MAX = 2.2
export const STREAM_TOUCH_SENSITIVITY_DEFAULT = 0.9
export const STREAM_TOUCH_LONG_PRESS_MIN_MS = 180
export const STREAM_TOUCH_LONG_PRESS_MAX_MS = 600
export const STREAM_TOUCH_LONG_PRESS_DEFAULT_MS = TOUCH_AS_CLICK_MAX_TIME_MS

function debugInputEventsEnabled() {
    try {
        return typeof window != "undefined"
            && window.localStorage != null
            && window.localStorage.getItem("ML_DEBUG_INPUT_EVENTS") == "1"
    } catch (_error) {
        return false
    }
}

const ANDROID_NATIVE_MOUSE_BRIDGE_FLAG_KEY = "ML_ANDROID_NATIVE_MOUSE_BRIDGE"
const ANDROID_NATIVE_MOUSE_EVENT_NAME = "ml-android-native-mouse"

export type AndroidNativeMouseBridgeMode = "relative" | "dragdrop" | "disabled"

export type AndroidNativeMouseBridgeEventDetail =
    | { type: "move", deltaX: number, deltaY: number }
    | { type: "button", button: number, isDown: boolean }
    | { type: "click", button: number }
    | { type: "wheel", deltaX: number, deltaY: number, highRes?: boolean }
    | { type: "status", active?: boolean, note?: string }

export type AndroidNativeMouseBridgeStatus = {
    available: boolean
    active: boolean
    provider: string | null
    version: string | null
    mode: AndroidNativeMouseBridgeMode
    reason: string
}

type AndroidNativeMouseBridgeSessionConfig = {
    enabled: boolean
    mode: AndroidNativeMouseBridgeMode
    streamerWidth: number
    streamerHeight: number
    viewportWidth: number
    viewportHeight: number
    touchSensitivity: number
    touchLongPressMs: number
    twoFingerRightClick: boolean
}

type AndroidNativeMouseBridgeAdapter = {
    isAvailable?: () => boolean
    getProviderName?: () => string
    getVersion?: () => string
    configureSession?: (configJson: string) => void
    configure?: (config: AndroidNativeMouseBridgeSessionConfig) => void
    setSessionEnabled?: (enabled: boolean) => void
    setEnabled?: (enabled: boolean) => void
}

declare global {
    interface Window {
        MoonlightAndroidNativeMouse?: AndroidNativeMouseBridgeAdapter
    }
}

function isAndroidTouchClient(): boolean {
    const touchPoints = Number(navigator.maxTouchPoints || 0)
    const userAgent = String(navigator.userAgent || "")
    return touchPoints > 0 && /Android/i.test(userAgent)
}

function isAndroidNativeMouseBridgeEnabled(): boolean {
    if (!isAndroidTouchClient()) {
        return false
    }

    try {
        return window.localStorage.getItem(ANDROID_NATIVE_MOUSE_BRIDGE_FLAG_KEY) != "0"
    } catch {
        return true
    }
}

function trySendChannel(channel: DataTransportChannel | null, buffer: ByteBuffer) {
    if (!channel) {
        return
    }

    buffer.flip()
    const readBuffer = buffer.getRemainingBuffer()
    if (readBuffer.length == 0) {
        throw "illegal buffer size"
    }
    channel.send(readBuffer.buffer)
}

export type MouseScrollMode = "highres" | "normal"
export type MouseMode = "relative" | "follow" | "pointAndDrag"

export type StreamInputConfig = {
    mouseMode: MouseMode
    mouseScrollMode: MouseScrollMode
    touchMode: "touch" | "mouseRelative" | "pointAndDrag"
    touchSensitivity: number
    touchLongPressMs: number
    twoFingerRightClick: boolean
    controllerConfig: ControllerConfig
}

export function defaultStreamInputConfig(): StreamInputConfig {
    return {
        mouseMode: "follow",
        mouseScrollMode: "highres",
        touchMode: "mouseRelative",
        touchSensitivity: STREAM_TOUCH_SENSITIVITY_DEFAULT,
        touchLongPressMs: STREAM_TOUCH_LONG_PRESS_DEFAULT_MS,
        twoFingerRightClick: true,
        controllerConfig: {
            invertAB: false,
            invertXY: false,
            sendIntervalOverride: null
        }
    }
}

export type PredictedTouchAction = "default" | "scroll" | "screenKeyboard"
export type ScreenKeyboardSetVisibleEvent = CustomEvent<{ visible: boolean }>
export type RelativeMousePreviewEvent = CustomEvent<{
    deltaX: number
    deltaY: number
    x: number
    y: number
    source?: "pointerLock" | "desktopRelative" | "touchRelative" | "touchFollow" | "wakePulse"
}>
export type TouchDebugSnapshot = {
    touchMode: StreamInputConfig["touchMode"]
    predictedAction: PredictedTouchAction
    trackedTouches: number
    primaryTouchActive: boolean
    twoFingerRightClickActive: boolean
    twoFingerRightClickTrackedTouches: number
    touchSupported: boolean | null
    nativeMouseEngine: "browser" | "native"
    nativeMouseBridgeAvailable: boolean
    nativeMouseBridgeProvider: string | null
    nativeMouseBridgeReason: string
}

export class StreamInput {

    private eventTarget = new EventTarget()

    private buffer: ByteBuffer = new ByteBuffer(1024)

    private connected = false
    private transitionLocked = false
    private config: StreamInputConfig
    private capabilities: StreamCapabilities = { touch: true }
    // Size of the streamer device
    private streamerSize: [number, number] = [0, 0]

    private keyboard: DataTransportChannel | null = null
    private mouseReliable: DataTransportChannel | null = null
    private mouseAbsolute: DataTransportChannel | null = null
    private mouseRelative: DataTransportChannel | null = null
    private touch: DataTransportChannel | null = null
    private controllers: DataTransportChannel | null = null
    private controllerInputs: Array<DataTransportChannel | null> = []
    private virtualControllerId = 15
    private virtualControllerEnabled = false
    private virtualControllerConnected = false
    private virtualControllerState: GamepadState = emptyGamepadState()
    private virtualControllerType = DEFAULT_VIRTUAL_CONTROLLER_ADVERTISEMENT.type
    private virtualControllerSupportedButtons = DEFAULT_VIRTUAL_CONTROLLER_ADVERTISEMENT.supportedButtons
    private virtualControllerCapabilities = DEFAULT_VIRTUAL_CONTROLLER_ADVERTISEMENT.capabilities
    private virtualControllerReleaseTimers: Map<number, number> = new Map()
    private virtualControllerTriggerReleaseTimers: Map<"left" | "right", number> = new Map()

    private touchSupported: boolean | null = null
    private readonly boundOnTouchData = this.onTouchData.bind(this)
    private readonly boundOnControllerData = this.onControllerData.bind(this)
    private relativeMouseFlushTimer: number | null = null
    private absoluteMouseFlushTimer: number | null = null
    private pendingRelativeMouseX = 0
    private pendingRelativeMouseY = 0
    private pressedMouseButtons: Set<number> = new Set()
    private pendingAbsoluteMouse: {
        x: number
        y: number
        referenceWidth: number
        referenceHeight: number
    } | null = null
    private syntheticMouseSuppressedUntil = 0
    private lastInputRect: DOMRect | null = null
    private androidNativeMouseBridgeStatus: AndroidNativeMouseBridgeStatus = {
        available: false,
        active: false,
        provider: null,
        version: null,
        mode: "disabled",
        reason: "Browser relative mouse path is active."
    }
    private readonly boundOnAndroidNativeMouseEvent = this.onAndroidNativeMouseEvent.bind(this)

    constructor(config?: StreamInputConfig) {
        this.config = defaultStreamInputConfig()
        if (config) {
            this.setConfig(config)
        }
        if (typeof window != "undefined") {
            window.addEventListener(ANDROID_NATIVE_MOUSE_EVENT_NAME, this.boundOnAndroidNativeMouseEvent as EventListener)
        }
        this.syncAndroidNativeMouseBridge()
    }

    private getDataChannel(transport: Transport, id: TransportChannelIdValue): DataTransportChannel {
        const channel = transport.getChannel(id)
        if (channel.type == "data") {
            return channel
        }
        throw `Failed to get channel ${id} as data transport channel`
    }
    setTransport(transport: Transport) {
        this.keyboard = this.getDataChannel(transport, TransportChannelId.KEYBOARD)

        this.mouseReliable = this.getDataChannel(transport, TransportChannelId.MOUSE_RELIABLE)
        this.mouseAbsolute = this.getDataChannel(transport, TransportChannelId.MOUSE_ABSOLUTE)
        this.mouseRelative = this.getDataChannel(transport, TransportChannelId.MOUSE_RELATIVE)

        if (this.touch) {
            this.touch.removeReceiveListener(this.boundOnTouchData)
        }
        this.touch = this.getDataChannel(transport, TransportChannelId.TOUCH)
        this.touch.addReceiveListener(this.boundOnTouchData)

        if (this.controllers) {
            this.controllers.removeReceiveListener(this.boundOnControllerData)
        }
        this.controllers = this.getDataChannel(transport, TransportChannelId.CONTROLLERS)
        this.controllers.addReceiveListener(this.boundOnControllerData)

        this.controllerInputs.length = 0
        for (let i = 0; i < 16; i++) {
            const channelId = TransportChannelId[`CONTROLLER${i}` as TransportChannelIdKey]

            this.controllerInputs[i] = this.getDataChannel(transport, channelId)
        }
    }

    setConfig(config: StreamInputConfig) {
        Object.assign(this.config, config)

        // Touch
        this.primaryTouch = null
        this.touchTracker.clear()
        this.clearPendingTouchMouseMove()
        this.clearTwoFingerRightClickGesture()
        this.clearRelativeMouseCarry()
        this.clearTouchRelativeMouseCarry()
        this.clearRelativeMouseSmoothing()
        this.clearMouseBatchState()
        this.syncAndroidNativeMouseBridge()
    }
    setTransitionLocked(locked: boolean) {
        this.transitionLocked = locked
        if (!locked) {
            this.syncAndroidNativeMouseBridge()
            return
        }

        this.clearMouseBatchState()
        this.clearPendingTouchMouseMove()
        this.clearTwoFingerRightClickGesture()
        this.resetRelativeMouseState()
        this.syncAndroidNativeMouseBridge()
    }
    getConfig(): StreamInputConfig {
        return this.config
    }

    getCapabilities(): StreamCapabilities {
        return this.capabilities
    }

    // -- External Event Listeners
    addScreenKeyboardVisibleEvent(listener: (event: ScreenKeyboardSetVisibleEvent) => void) {
        this.eventTarget.addEventListener("ml-screenkeyboardvisible", listener as any)
    }

    addRelativeMousePreviewEvent(listener: (event: RelativeMousePreviewEvent) => void) {
        this.eventTarget.addEventListener("ml-relativemousepreview", listener as any)
    }

    // -- On Stream Start
    onStreamStart(capabilities: StreamCapabilities, streamerSize: [number, number]) {
        this.connected = true

        this.capabilities = capabilities
        this.streamerSize = streamerSize
        this.clearMouseBatchState()
        this.clearPressedMouseButtons()
        this.clearRelativeMouseCarry()
        this.clearTouchRelativeMouseCarry()
        this.clearRelativeMouseSmoothing()
        this.registerBufferedControllers()
        if (this.virtualControllerEnabled) {
            this.ensureVirtualControllerConnected()
        }
        this.syncAndroidNativeMouseBridge()
    }

    updateStreamerSize(streamerSize: [number, number]) {
        this.streamerSize = streamerSize
        this.clearMouseBatchState()
        this.clearRelativeMouseCarry()
        this.clearTouchRelativeMouseCarry()
        this.clearRelativeMouseSmoothing()
        this.syncAndroidNativeMouseBridge()
    }

    onStreamStop() {
        this.connected = false
        this.clearMouseBatchState()
        this.clearPressedMouseButtons()
        this.clearPendingTouchMouseMove()
        this.clearTwoFingerRightClickGesture()
        this.clearRelativeMouseCarry()
        this.clearTouchRelativeMouseCarry()
        this.clearRelativeMouseSmoothing()
        this.disconnectVirtualController()
        this.syncAndroidNativeMouseBridge()
    }

    // -- Keyboard
    private pressedKeys: Set<number> = new Set()

    onKeyDown(event: KeyboardEvent) {
        this.sendKeyEvent(true, event)
    }
    onKeyUp(event: KeyboardEvent) {
        this.sendKeyEvent(false, event)
    }

    onPaste(event: ClipboardEvent) {

        const data = event.clipboardData
        if (!data) {
            return
        }

        if (debugInputEventsEnabled()) {
            console.debug("PASTE", data)
        }

        const text = data.getData("text/plain")
        if (text) {
            if (debugInputEventsEnabled()) {
                console.debug("PASTE TEXT", text)
            }

            // Before sending text raise all keys
            this.raiseAllKeys()

            this.sendText(text)
        }
    }

    private sendKeyEvent(isDown: boolean, event: KeyboardEvent) {
        const key = convertToKey(event)
        if (key == null) {
            return
        }

        if (isDown) {
            if (this.pressedKeys.has(key)) {
                return
            }

            this.pressedKeys.add(key)
        } else {
            if (!this.pressedKeys.has(key)) {
                return
            }

            this.pressedKeys.delete(key)
        }

        const modifiers = convertToModifiers(event)

        if (debugInputEventsEnabled() && "debug" in console) {
            console.debug(
                isDown ? "DOWN" : "UP",
                event.code,
                convertToKey(event),
                convertToModifiers(event).toString(16)
            )
        }
        this.sendKey(isDown, key, modifiers)
    }

    raiseAllKeys() {
        for (const key of this.pressedKeys) {
            this.sendKey(false, key, 0)
        }
        this.pressedKeys.clear()
    }

    // Note: key = StreamKeys.VK_, modifiers = StreamKeyModifiers.
    sendKey(isDown: boolean, key: number, modifiers: number) {
        this.buffer.reset()

        this.buffer.putU8(0)

        this.buffer.putBool(isDown)
        this.buffer.putU8(modifiers)
        this.buffer.putU16(key)

        trySendChannel(this.keyboard, this.buffer)
    }
    sendText(text: string) {
        this.buffer.putU8(1)

        this.buffer.putU8(text.length)
        this.buffer.putUtf8Raw(text)

        trySendChannel(this.keyboard, this.buffer)
    }

    // -- Mouse
    private noteTouchInteraction() {
        this.syntheticMouseSuppressedUntil = Date.now() + TOUCH_MOUSE_EVENT_SUPPRESSION_MS
    }

    private shouldSuppressSyntheticMouseEvent(): boolean {
        return Date.now() < this.syntheticMouseSuppressedUntil
    }

    onMouseDown(event: MouseEvent, rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        if (this.shouldSuppressSyntheticMouseEvent()) {
            return
        }
        const button = convertToButton(event)
        if (button == null) {
            return
        }

        if (this.config.mouseMode == "relative") {
            this.sendMouseButton(true, button)
        } else if (this.config.mouseMode == "follow" || this.config.mouseMode == "pointAndDrag") {
            this.sendMousePositionClientCoordinates(event.clientX, event.clientY, rect, true, button)
        }
    }
    onMouseUp(event: MouseEvent) {
        if (this.transitionLocked) {
            return
        }
        if (this.shouldSuppressSyntheticMouseEvent()) {
            return
        }
        const button = convertToButton(event)
        if (button == null) {
            return
        }

        if (this.config.mouseMode == "relative" || this.config.mouseMode == "follow") {
            this.sendMouseButton(false, button)
        } else if (this.config.mouseMode == "pointAndDrag") {
            this.sendMouseButton(false, button)
        }
    }
    onMouseMove(event: MouseEvent, rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        if (this.shouldSuppressSyntheticMouseEvent()) {
            return
        }
        if (this.config.mouseMode == "relative") {
            if (typeof document != "undefined" && !!document.pointerLockElement) {
                this.sendPointerLockedRelativeMouseMove(event.movementX, event.movementY)
                return
            }
            this.sendMouseMoveClientCoordinates(event.movementX, event.movementY, rect)
        } else if (this.config.mouseMode == "follow") {
            this.sendMousePositionClientCoordinates(event.clientX, event.clientY, rect, false)
        } else if (this.config.mouseMode == "pointAndDrag") {
            if (event.buttons) {
                // some button pressed
                this.sendMouseMoveClientCoordinates(event.movementX, event.movementY, rect)
            }
        }
    }
    onMouseWheel(event: WheelEvent) {
        if (this.transitionLocked) {
            return
        }
        if (this.shouldSuppressSyntheticMouseEvent()) {
            return
        }
        if (this.config.mouseScrollMode == "highres") {
            this.sendMouseWheelHighRes(event.deltaX, -event.deltaY)
        } else if (this.config.mouseScrollMode == "normal") {
            this.sendMouseWheel(event.deltaX, -event.deltaY)
        }
    }

    private clearMouseBatchTimers() {
        if (this.relativeMouseFlushTimer != null) {
            window.clearTimeout(this.relativeMouseFlushTimer)
            this.relativeMouseFlushTimer = null
        }
        if (this.absoluteMouseFlushTimer != null) {
            window.clearTimeout(this.absoluteMouseFlushTimer)
            this.absoluteMouseFlushTimer = null
        }
    }

    private clearMouseBatchState() {
        this.clearMouseBatchTimers()
        this.pendingRelativeMouseX = 0
        this.pendingRelativeMouseY = 0
        this.pendingAbsoluteMouse = null
    }

    private clearPressedMouseButtons() {
        this.pressedMouseButtons.clear()
    }

    private scheduleRelativeMouseFlush() {
        if (!this.connected || this.relativeMouseFlushTimer != null) {
            return
        }

        this.relativeMouseFlushTimer = window.setTimeout(() => {
            this.relativeMouseFlushTimer = null
            this.flushPendingRelativeMouseMove()
        }, MOUSE_BATCHING_INTERVAL_MS)
    }

    private scheduleAbsoluteMouseFlush() {
        if (!this.connected || this.absoluteMouseFlushTimer != null) {
            return
        }

        this.absoluteMouseFlushTimer = window.setTimeout(() => {
            this.absoluteMouseFlushTimer = null
            this.flushPendingAbsoluteMousePosition()
        }, MOUSE_BATCHING_INTERVAL_MS)
    }

    private flushPendingRelativeMouseMove() {
        if (!this.connected) {
            this.pendingRelativeMouseX = 0
            this.pendingRelativeMouseY = 0
            return
        }

        let remainingX = this.pendingRelativeMouseX
        let remainingY = this.pendingRelativeMouseY
        if (remainingX == 0 && remainingY == 0) {
            return
        }

        const estimatedBufferedBytes = this.mouseRelative?.estimatedBufferedBytes()
        if (this.mouseRelative && estimatedBufferedBytes != null && estimatedBufferedBytes > RELATIVE_MOUSE_PACKET_SIZE) {
            this.scheduleRelativeMouseFlush()
            return
        }

        this.pendingRelativeMouseX = 0
        this.pendingRelativeMouseY = 0

        while (remainingX != 0 || remainingY != 0) {
            const sendX = Math.max(I16_MIN, Math.min(I16_MAX, remainingX))
            const sendY = Math.max(I16_MIN, Math.min(I16_MAX, remainingY))
            this.sendRawMouseMove(sendX, sendY)
            remainingX -= sendX
            remainingY -= sendY
        }
    }

    private flushPendingAbsoluteMousePosition() {
        if (!this.connected) {
            this.pendingAbsoluteMouse = null
            return
        }

        if (!this.pendingAbsoluteMouse) {
            return
        }

        const estimatedBufferedBytes = this.mouseAbsolute?.estimatedBufferedBytes()
        if (this.mouseAbsolute && estimatedBufferedBytes != null && estimatedBufferedBytes > ABSOLUTE_MOUSE_PACKET_SIZE) {
            this.scheduleAbsoluteMouseFlush()
            return
        }

        const pending = this.pendingAbsoluteMouse
        this.pendingAbsoluteMouse = null
        this.sendRawMousePosition(
            pending.x,
            pending.y,
            pending.referenceWidth,
            pending.referenceHeight,
            false
        )
    }

    private flushPendingMouseInputs() {
        this.flushPendingRelativeMouseMove()
        this.flushPendingAbsoluteMousePosition()
    }

    private sendRawMouseMove(movementX: number, movementY: number) {
        const clampedX = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(movementX)))
        const clampedY = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(movementY)))
        if (clampedX == 0 && clampedY == 0) {
            return
        }

        this.buffer.reset()

        this.buffer.putU8(0)
        this.buffer.putI16(clampedX)
        this.buffer.putI16(clampedY)

        trySendChannel(this.mouseRelative, this.buffer)
    }
    sendMouseMove(movementX: number, movementY: number) {
        const clampedX = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(movementX)))
        const clampedY = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(movementY)))
        if (clampedX == 0 && clampedY == 0) {
            return
        }

        const estimatedBufferedBytes = this.mouseRelative?.estimatedBufferedBytes()
        const canSendImmediately =
            this.connected
            && this.relativeMouseFlushTimer == null
            && this.pendingRelativeMouseX == 0
            && this.pendingRelativeMouseY == 0
            && (estimatedBufferedBytes == null || estimatedBufferedBytes <= RELATIVE_MOUSE_PACKET_SIZE)

        if (canSendImmediately) {
            this.sendRawMouseMove(clampedX, clampedY)
            return
        }

        this.pendingRelativeMouseX += clampedX
        this.pendingRelativeMouseY += clampedY
        this.scheduleRelativeMouseFlush()
    }
    private sendPointerLockedRelativeMouseMove(movementX: number, movementY: number) {
        if (!Number.isFinite(movementX) || !Number.isFinite(movementY)) {
            return
        }

        const adjustedX = movementX * POINTER_LOCK_RELATIVE_MOUSE_GAIN
        const adjustedY = movementY * POINTER_LOCK_RELATIVE_MOUSE_GAIN
        const clampedX = Math.max(-POINTER_LOCK_RELATIVE_MOUSE_MAX_STEP, Math.min(POINTER_LOCK_RELATIVE_MOUSE_MAX_STEP, adjustedX))
        const clampedY = Math.max(-POINTER_LOCK_RELATIVE_MOUSE_MAX_STEP, Math.min(POINTER_LOCK_RELATIVE_MOUSE_MAX_STEP, adjustedY))

        if (Math.abs(clampedX) < 0.01 && Math.abs(clampedY) < 0.01) {
            return
        }

        this.sendMouseMove(clampedX, clampedY)

        const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
            detail: {
                deltaX: clampedX,
                deltaY: clampedY,
                x: Number.NaN,
                y: Number.NaN,
                source: "pointerLock"
            }
        })
        this.eventTarget.dispatchEvent(previewEvent)
    }
    sendMouseMoveClientCoordinates(movementX: number, movementY: number, rect: DOMRect) {
        if (!rect || rect.width <= 0 || rect.height <= 0 || this.streamerSize[0] <= 0 || this.streamerSize[1] <= 0) {
            return
        }

        const scaledMovementX = movementX / rect.width * this.streamerSize[0]
        const scaledMovementY = movementY / rect.height * this.streamerSize[1]
        const scaledDistance = Math.hypot(scaledMovementX, scaledMovementY)
        const smoothingRamp = Math.min(1, scaledDistance / RELATIVE_MOUSE_EMA_FAST_DISTANCE)
        const smoothingAlpha = RELATIVE_MOUSE_EMA_ALPHA_SLOW
            + ((RELATIVE_MOUSE_EMA_ALPHA_FAST - RELATIVE_MOUSE_EMA_ALPHA_SLOW) * smoothingRamp)

        this.relativeMouseEmaX += (scaledMovementX - this.relativeMouseEmaX) * smoothingAlpha
        this.relativeMouseEmaY += (scaledMovementY - this.relativeMouseEmaY) * smoothingAlpha

        if (scaledDistance <= RELATIVE_MOUSE_DEADZONE) {
            this.relativeMouseEmaX *= 0.42
            this.relativeMouseEmaY *= 0.42
        }

        if (Math.hypot(this.relativeMouseEmaX, this.relativeMouseEmaY) <= RELATIVE_MOUSE_DEADZONE
            && Math.hypot(this.relativeMouseCarryX, this.relativeMouseCarryY) <= RELATIVE_MOUSE_DEADZONE) {
            return
        }

        const carriedX = this.relativeMouseEmaX + this.relativeMouseCarryX
        const carriedY = this.relativeMouseEmaY + this.relativeMouseCarryY
        const truncatedX = Math.trunc(carriedX)
        const truncatedY = Math.trunc(carriedY)
        const sendX = Math.max(I16_MIN, Math.min(I16_MAX, truncatedX))
        const sendY = Math.max(I16_MIN, Math.min(I16_MAX, truncatedY))

        this.relativeMouseCarryX = truncatedX == sendX ? carriedX - sendX : 0
        this.relativeMouseCarryY = truncatedY == sendY ? carriedY - sendY : 0
        this.sendMouseMove(sendX, sendY)

        if (this.config.mouseMode == "relative") {
            const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                detail: {
                    deltaX: sendX * (rect.width / this.streamerSize[0]),
                    deltaY: sendY * (rect.height / this.streamerSize[1]),
                    x: Number.NaN,
                    y: Number.NaN,
                    source: "desktopRelative"
                }
            })
            this.eventTarget.dispatchEvent(previewEvent)
        }
    }
    private sendRawMousePosition(x: number, y: number, referenceWidth: number, referenceHeight: number, reliable: boolean) {
        this.buffer.reset()

        this.buffer.putU8(1)
        this.buffer.putI16(x)
        this.buffer.putI16(y)
        this.buffer.putI16(referenceWidth)
        this.buffer.putI16(referenceHeight)

        if (reliable) {
            trySendChannel(this.mouseReliable, this.buffer)
        } else {
            trySendChannel(this.mouseAbsolute, this.buffer)
        }
    }
    sendMousePosition(x: number, y: number, referenceWidth: number, referenceHeight: number, reliable: boolean) {
        if (this.transitionLocked) {
            return
        }
        if (reliable) {
            this.flushPendingRelativeMouseMove()
            this.pendingAbsoluteMouse = null
            if (this.absoluteMouseFlushTimer != null) {
                window.clearTimeout(this.absoluteMouseFlushTimer)
                this.absoluteMouseFlushTimer = null
            }
            this.sendRawMousePosition(x, y, referenceWidth, referenceHeight, true)
            return
        }

        this.pendingAbsoluteMouse = {
            x,
            y,
            referenceWidth,
            referenceHeight,
        }

        const estimatedBufferedBytes = this.mouseAbsolute?.estimatedBufferedBytes()
        const canSendImmediately =
            this.connected
            && this.absoluteMouseFlushTimer == null
            && (estimatedBufferedBytes == null || estimatedBufferedBytes <= ABSOLUTE_MOUSE_PACKET_SIZE)

        if (canSendImmediately) {
            this.flushPendingAbsoluteMousePosition()
            return
        }

        this.scheduleAbsoluteMouseFlush()
    }
    sendMousePositionClientCoordinates(clientX: number, clientY: number, rect: DOMRect, reliable: boolean, mouseButton?: number) {
        const position = this.calcNormalizedPosition(clientX, clientY, rect)
        if (position) {
            const [x, y] = position
            this.sendMousePosition(x * 4096.0, y * 4096.0, 4096.0, 4096.0, reliable)

            if (mouseButton != undefined) {
                this.sendMouseButton(true, mouseButton)
            }
        }
    }
    // Note: button = StreamMouseButton.
    sendMouseButton(isDown: boolean, button: number) {
        if (this.transitionLocked) {
            return
        }

        if (isDown) {
            if (this.pressedMouseButtons.has(button)) {
                return
            }
            this.pressedMouseButtons.add(button)
        } else {
            if (!this.pressedMouseButtons.has(button)) {
                return
            }
            this.pressedMouseButtons.delete(button)
        }

        this.flushPendingMouseInputs()

        this.buffer.reset()

        this.buffer.putU8(2)
        this.buffer.putBool(isDown)
        this.buffer.putU8(button)

        trySendChannel(this.mouseReliable, this.buffer)
    }
    sendMouseWheelHighRes(deltaX: number, deltaY: number) {
        if (this.transitionLocked) {
            return
        }
        const clampedX = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(deltaX)))
        const clampedY = Math.max(I16_MIN, Math.min(I16_MAX, Math.trunc(deltaY)))
        if (clampedX == 0 && clampedY == 0) {
            return
        }
        this.buffer.reset()

        this.buffer.putU8(3)
        this.buffer.putI16(clampedX)
        this.buffer.putI16(clampedY)

        trySendChannel(this.mouseRelative, this.buffer)
    }
    sendMouseWheel(deltaX: number, deltaY: number) {
        if (this.transitionLocked) {
            return
        }
        const clampedX = Math.max(-128, Math.min(127, Math.trunc(deltaX)))
        const clampedY = Math.max(-128, Math.min(127, Math.trunc(deltaY)))
        if (clampedX == 0 && clampedY == 0) {
            return
        }
        this.buffer.reset()

        this.buffer.putU8(4)
        this.buffer.putI8(clampedX)
        this.buffer.putI8(clampedY)

        trySendChannel(this.mouseRelative, this.buffer)
    }

    // -- Touch
    private touchTracker: Map<number, {
        startTime: number
        originX: number
        originY: number
        x: number
        y: number
        mouseClicked: boolean
        mouseMoved: boolean
    }> = new Map()
    private touchMouseAction: PredictedTouchAction = "default"
    private primaryTouch: number | null = null
    private twoFingerRightClickActive = false
    private twoFingerRightClickTouchIds: Set<number> = new Set()
    private pendingTouchMouseMoveX = 0
    private pendingTouchMouseMoveY = 0
    private relativeMouseCarryX = 0
    private relativeMouseCarryY = 0
    private relativeMouseEmaX = 0
    private relativeMouseEmaY = 0
    private touchRelativeMouseCarryX = 0
    private touchRelativeMouseCarryY = 0

    private onTouchData(data: ArrayBuffer) {
        const buffer = new ByteBuffer(new Uint8Array(data))
        this.touchSupported = buffer.getBool()
    }

    private getRequestedAndroidNativeMouseBridgeMode(): AndroidNativeMouseBridgeMode {
        if (this.config.mouseMode == "relative"
            && this.config.touchMode == "mouseRelative"
            && isAndroidNativeMouseBridgeEnabled()) {
            return "relative"
        }

        return "disabled"
    }

    private getAndroidNativeMouseBridgeAdapter(): AndroidNativeMouseBridgeAdapter | null {
        if (typeof window == "undefined") {
            return null
        }

        const bridge = window.MoonlightAndroidNativeMouse
        if (!bridge) {
            return null
        }

        try {
            if (typeof bridge.isAvailable == "function" && !bridge.isAvailable()) {
                return null
            }
        } catch {
            return null
        }

        return bridge
    }

    private syncAndroidNativeMouseBridge(rect?: DOMRect | null) {
        if (rect) {
            this.lastInputRect = rect
        }

        const mode = this.getRequestedAndroidNativeMouseBridgeMode()
        const bridge = mode == "disabled" ? this.getAndroidNativeMouseBridgeAdapter() : this.getAndroidNativeMouseBridgeAdapter()
        const available = bridge != null
        const provider = bridge && typeof bridge.getProviderName == "function"
            ? bridge.getProviderName()
            : available ? "android-webview" : null
        const version = bridge && typeof bridge.getVersion == "function"
            ? bridge.getVersion()
            : null
        let active = false
        let reason = "Browser relative mouse path is active."

        if (mode == "relative") {
            if (!available) {
                reason = "Relative browser mouse remains active until an Android native bridge attaches."
            } else if (!this.connected) {
                reason = "Android native relative mouse bridge is ready for stream start."
            } else if (this.transitionLocked) {
                reason = "Android native relative mouse bridge is waiting for the current transition to settle."
            } else {
                active = true
                reason = "Android native relative mouse bridge is active."
            }
        } else if (available) {
            reason = "Android native mouse bridge is attached but idle."
        }

        if (bridge) {
            const viewport = this.lastInputRect
                ?? new DOMRect(0, 0, Math.max(window.innerWidth || 1, 1), Math.max(window.innerHeight || 1, 1))
            const sessionConfig: AndroidNativeMouseBridgeSessionConfig = {
                enabled: active,
                mode,
                streamerWidth: Math.max(0, Math.trunc(this.streamerSize[0] || 0)),
                streamerHeight: Math.max(0, Math.trunc(this.streamerSize[1] || 0)),
                viewportWidth: Math.max(1, Math.trunc(viewport.width || window.innerWidth || 1)),
                viewportHeight: Math.max(1, Math.trunc(viewport.height || window.innerHeight || 1)),
                touchSensitivity: this.config.touchSensitivity,
                touchLongPressMs: this.config.touchLongPressMs,
                twoFingerRightClick: this.config.twoFingerRightClick
            }

            try {
                if (typeof bridge.configureSession == "function") {
                    bridge.configureSession(JSON.stringify(sessionConfig))
                } else if (typeof bridge.configure == "function") {
                    bridge.configure(sessionConfig)
                }

                if (typeof bridge.setSessionEnabled == "function") {
                    bridge.setSessionEnabled(active)
                } else if (typeof bridge.setEnabled == "function") {
                    bridge.setEnabled(active)
                }
            } catch (error) {
                active = false
                reason = `Android native mouse bridge configure failed: ${error instanceof Error ? error.message : String(error)}`
            }
        }

        this.androidNativeMouseBridgeStatus = {
            available,
            active,
            provider,
            version,
            mode,
            reason
        }
    }

    private isAndroidNativeRelativeMouseBridgeActive(): boolean {
        return this.androidNativeMouseBridgeStatus.active && this.androidNativeMouseBridgeStatus.mode == "relative"
    }

    private onAndroidNativeMouseEvent(event: Event) {
        const customEvent = event as CustomEvent<AndroidNativeMouseBridgeEventDetail | undefined>
        const detail = customEvent.detail
        if (!detail) {
            return
        }

        if (detail.type == "status") {
            if (typeof detail.active == "boolean" && this.androidNativeMouseBridgeStatus.mode == "relative") {
                this.androidNativeMouseBridgeStatus = {
                    ...this.androidNativeMouseBridgeStatus,
                    active: detail.active,
                    reason: detail.note || this.androidNativeMouseBridgeStatus.reason
                }
            }
            return
        }

        if (!this.connected || this.transitionLocked || !this.isAndroidNativeRelativeMouseBridgeActive()) {
            return
        }

        this.noteTouchInteraction()
        const rect = this.lastInputRect
            ?? new DOMRect(0, 0, Math.max(window.innerWidth || 1, 1), Math.max(window.innerHeight || 1, 1))

        switch (detail.type) {
        case "move": {
            const movementX = detail.deltaX * this.config.touchSensitivity
            const movementY = detail.deltaY * this.config.touchSensitivity
            const [shapedMovementX, shapedMovementY] = this.shapeTouchMouseMovement(movementX, movementY)
            if (Math.abs(shapedMovementX) < 0.001 && Math.abs(shapedMovementY) < 0.001) {
                return
            }
            this.clearPendingTouchMouseMove()
            this.sendTouchRelativeMouseMoveClientCoordinates(shapedMovementX, shapedMovementY, rect)
            const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                detail: {
                    deltaX: shapedMovementX,
                    deltaY: shapedMovementY,
                    x: Number.NaN,
                    y: Number.NaN,
                    source: "touchRelative"
                }
            })
            this.eventTarget.dispatchEvent(previewEvent)
            return
        }
        case "button":
            this.sendMouseButton(detail.isDown, detail.button)
            return
        case "click":
            this.sendMouseButton(true, detail.button)
            this.sendMouseButton(false, detail.button)
            return
        case "wheel":
            if (detail.highRes ?? true) {
                this.sendMouseWheelHighRes(detail.deltaX, detail.deltaY)
            } else {
                this.sendMouseWheel(detail.deltaX, detail.deltaY)
            }
            return
        }
    }

    getAndroidNativeMouseBridgeStatus(): AndroidNativeMouseBridgeStatus {
        return { ...this.androidNativeMouseBridgeStatus }
    }

    private updateTouchTracker(touch: Touch) {
        const oldTouch = this.touchTracker.get(touch.identifier)
        if (!oldTouch) {
            this.touchTracker.set(touch.identifier, {
                startTime: Date.now(),
                originX: touch.clientX,
                originY: touch.clientY,
                x: touch.clientX,
                y: touch.clientY,
                mouseMoved: false,
                mouseClicked: false
            })
        } else {
            oldTouch.x = touch.clientX
            oldTouch.y = touch.clientY
        }
    }

    private calcTouchTime(touch: { startTime: number }): number {
        return Date.now() - touch.startTime
    }

    private isAbsoluteTouchCursorMode(): boolean {
        return this.config.mouseMode == "follow" && this.config.touchMode == "mouseRelative"
    }

    private queueTouchMouseMove(movementX: number, movementY: number) {
        if (!Number.isFinite(movementX) || !Number.isFinite(movementY)) {
            return
        }

        this.pendingTouchMouseMoveX += movementX
        this.pendingTouchMouseMoveY += movementY
    }

    private clearPendingTouchMouseMove() {
        this.pendingTouchMouseMoveX = 0
        this.pendingTouchMouseMoveY = 0
    }

    private clearRelativeMouseCarry() {
        this.relativeMouseCarryX = 0
        this.relativeMouseCarryY = 0
    }

    private clearTouchRelativeMouseCarry() {
        this.touchRelativeMouseCarryX = 0
        this.touchRelativeMouseCarryY = 0
    }

    private clearRelativeMouseSmoothing() {
        this.relativeMouseEmaX = 0
        this.relativeMouseEmaY = 0
    }
    resetRelativeMouseState() {
        if (this.relativeMouseFlushTimer != null) {
            window.clearTimeout(this.relativeMouseFlushTimer)
            this.relativeMouseFlushTimer = null
        }
        this.pendingRelativeMouseX = 0
        this.pendingRelativeMouseY = 0
        this.clearPressedMouseButtons()
        this.clearRelativeMouseCarry()
        this.clearTouchRelativeMouseCarry()
        this.clearRelativeMouseSmoothing()
    }

    private isNativeRelativeTouchMode(): boolean {
        return this.config.mouseMode == "relative" && this.config.touchMode == "mouseRelative"
    }

    private shapeTouchMouseMovement(movementX: number, movementY: number): [number, number] {
        const distance = Math.hypot(movementX, movementY)
        if (distance <= TOUCH_MOUSE_MOVE_IGNORE_DISTANCE) {
            return [0, 0]
        }

        if (this.isNativeRelativeTouchMode()) {
            return [movementX, movementY]
        }

        const ramp = Math.min(1, distance / TOUCH_MOUSE_MOVE_PRECISION_FULL_DISTANCE)
        const precisionFactor = TOUCH_MOUSE_MOVE_PRECISION_MIN_FACTOR + ((1 - TOUCH_MOUSE_MOVE_PRECISION_MIN_FACTOR) * ramp)
        const shapedX = movementX * precisionFactor
        const shapedY = movementY * precisionFactor

        if (Math.hypot(shapedX, shapedY) <= TOUCH_MOUSE_MOVE_IGNORE_DISTANCE) {
            return [0, 0]
        }

        return [shapedX, shapedY]
    }

    private flushPendingTouchMouseMove(rect: DOMRect, flushAll: boolean = false) {
        let remainingX = this.pendingTouchMouseMoveX
        let remainingY = this.pendingTouchMouseMoveY
        const pendingDistance = Math.hypot(remainingX, remainingY)
        if (pendingDistance <= TOUCH_MOUSE_MOVE_IGNORE_DISTANCE) {
            this.clearPendingTouchMouseMove()
            return
        }

        let appliedX = 0
        let appliedY = 0

        if (flushAll || pendingDistance <= TOUCH_MOUSE_MOVE_MAX_STEP_DISTANCE) {
            appliedX = remainingX
            appliedY = remainingY
            remainingX = 0
            remainingY = 0
        } else {
            const stepScale = Math.min(1, TOUCH_MOUSE_MOVE_MAX_STEP_DISTANCE / pendingDistance)
            appliedX = remainingX * stepScale
            appliedY = remainingY * stepScale
            remainingX -= appliedX
            remainingY -= appliedY
        }

        this.pendingTouchMouseMoveX = remainingX
        this.pendingTouchMouseMoveY = remainingY
        if (this.isNativeRelativeTouchMode()) {
            this.sendTouchRelativeMouseMoveClientCoordinates(appliedX, appliedY, rect)
        } else {
            this.sendMouseMoveClientCoordinates(appliedX, appliedY, rect)
        }
    }

    private sendTouchRelativeMouseMoveClientCoordinates(movementX: number, movementY: number, rect: DOMRect) {
        if (!rect || rect.width <= 0 || rect.height <= 0 || this.streamerSize[0] <= 0 || this.streamerSize[1] <= 0) {
            return
        }

        if (!Number.isFinite(movementX) || !Number.isFinite(movementY)) {
            return
        }

        const touchDistance = Math.hypot(movementX, movementY)
        if (touchDistance <= TOUCH_NATIVE_RELATIVE_DEADZONE) {
            return
        }

        const nativeRamp = Math.min(1, touchDistance / TOUCH_NATIVE_RELATIVE_PRECISION_FULL_DISTANCE)
        const nativePrecisionFactor = TOUCH_NATIVE_RELATIVE_PRECISION_MIN_FACTOR
            + ((1 - TOUCH_NATIVE_RELATIVE_PRECISION_MIN_FACTOR) * nativeRamp)
        const nativeScaleX = Math.min(TOUCH_NATIVE_RELATIVE_MAX_STREAM_SCALE, this.streamerSize[0] / rect.width)
        const nativeScaleY = Math.min(TOUCH_NATIVE_RELATIVE_MAX_STREAM_SCALE, this.streamerSize[1] / rect.height)

        let scaledMovementX = movementX * nativePrecisionFactor * nativeScaleX
        let scaledMovementY = movementY * nativePrecisionFactor * nativeScaleY
        const scaledDistance = Math.hypot(scaledMovementX, scaledMovementY)
        if (scaledDistance > TOUCH_NATIVE_RELATIVE_MAX_STEP) {
            const clampScale = TOUCH_NATIVE_RELATIVE_MAX_STEP / scaledDistance
            scaledMovementX *= clampScale
            scaledMovementY *= clampScale
        }

        const carriedX = scaledMovementX + this.touchRelativeMouseCarryX
        const carriedY = scaledMovementY + this.touchRelativeMouseCarryY
        const truncatedX = Math.trunc(carriedX)
        const truncatedY = Math.trunc(carriedY)
        const sendX = Math.max(I16_MIN, Math.min(I16_MAX, truncatedX))
        const sendY = Math.max(I16_MIN, Math.min(I16_MAX, truncatedY))

        this.touchRelativeMouseCarryX = truncatedX == sendX ? carriedX - sendX : 0
        this.touchRelativeMouseCarryY = truncatedY == sendY ? carriedY - sendY : 0
        this.sendMouseMove(sendX, sendY)
    }
    private calcTouchOriginDistance(
        touch: { x: number, y: number } | { clientX: number, clientY: number },
        oldTouch: { originX: number, originY: number }
    ): number {
        if ("clientX" in touch) {
            return Math.hypot(touch.clientX - oldTouch.originX, touch.clientY - oldTouch.originY)
        } else {
            return Math.hypot(touch.x - oldTouch.originX, touch.y - oldTouch.originY)
        }
    }

    private shouldTriggerTwoFingerRightClick(event: TouchEvent): boolean {
        if (!this.config.twoFingerRightClick || this.config.touchMode == "touch") {
            return false
        }

        if (!event.touches || event.touches.length !== 2) {
            return false
        }

        if (!event.changedTouches || event.changedTouches.length < 2) {
            return false
        }

        for (const touch of Array.from(event.touches).slice(0, 2)) {
            const trackedTouch = this.touchTracker.get(touch.identifier)
            if (!trackedTouch || trackedTouch.mouseMoved || trackedTouch.mouseClicked) {
                return false
            }
        }

        return true
    }

    private triggerTwoFingerRightClick(event: TouchEvent, rect: DOMRect): boolean {
        const touches = Array.from(event.touches || []).slice(0, 2)
        if (touches.length < 2) {
            return false
        }

        const centerX = (touches[0].clientX + touches[1].clientX) / 2
        const centerY = (touches[0].clientY + touches[1].clientY) / 2
        this.sendMousePositionClientCoordinates(centerX, centerY, rect, true)
        this.sendMouseButton(true, StreamMouseButton.RIGHT)
        this.sendMouseButton(false, StreamMouseButton.RIGHT)

        this.twoFingerRightClickActive = true
        this.twoFingerRightClickTouchIds.clear()
        for (const touch of touches) {
            this.twoFingerRightClickTouchIds.add(touch.identifier)
        }

        this.primaryTouch = null
        this.touchMouseAction = "default"
        this.clearPendingTouchMouseMove()
        return true
    }

    sendMouseWakePulse(triggerDummyClick: boolean = false): boolean {
        if (this.transitionLocked || !this.connected) {
            return false
        }

        const referenceWidth = Math.max(1, Math.trunc(this.streamerSize[0] || 0))
        const referenceHeight = Math.max(1, Math.trunc(this.streamerSize[1] || 0))
        if (referenceWidth <= 0 || referenceHeight <= 0) {
            return false
        }

        const centerX = Math.max(0, Math.floor((referenceWidth - 1) / 2))
        const centerY = Math.max(0, Math.floor((referenceHeight - 1) / 2))
        const nudgeX = Math.min(referenceWidth - 1, centerX + (referenceWidth > 2 ? 1 : 0))

        try {
            this.sendMousePosition(centerX, centerY, referenceWidth, referenceHeight, true)
            if (nudgeX != centerX) {
                this.sendMousePosition(nudgeX, centerY, referenceWidth, referenceHeight, true)
                this.sendMousePosition(centerX, centerY, referenceWidth, referenceHeight, true)
            }
            if (triggerDummyClick) {
                this.sendMouseButton(true, StreamMouseButton.LEFT)
                this.sendMouseButton(false, StreamMouseButton.LEFT)
            }

            const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                detail: {
                    deltaX: 0,
                    deltaY: 0,
                    x: Number.NaN,
                    y: Number.NaN,
                    source: "wakePulse"
                }
            })
            this.eventTarget.dispatchEvent(previewEvent)
            return true
        } catch (error) {
            console.debug("[stream-input] mouse wake pulse failed", error)
            return false
        }
    }

    private clearTwoFingerRightClickGesture() {
        this.twoFingerRightClickActive = false
        this.twoFingerRightClickTouchIds.clear()
        this.clearPendingTouchMouseMove()
    }

    private consumeTwoFingerRightClickGesture(event: TouchEvent): boolean {
        if (!this.twoFingerRightClickActive) {
            return false
        }

        event.preventDefault()
        event.stopPropagation()

        const currentTouches = event.touches ? Array.from(event.touches) : []
        if (currentTouches.length < 2) {
            this.clearTwoFingerRightClickGesture()
            return true
        }

        const activeIds = new Set(currentTouches.map((touch) => touch.identifier))
        let matchedTrackedTouches = 0
        this.twoFingerRightClickTouchIds.forEach((identifier) => {
            if (activeIds.has(identifier)) {
                matchedTrackedTouches += 1
            }
        })

        if (matchedTrackedTouches < 2) {
            this.clearTwoFingerRightClickGesture()
        }

        return true
    }

    onTouchStart(event: TouchEvent, rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        this.syncAndroidNativeMouseBridge(rect)
        this.noteTouchInteraction()
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            this.touchMouseAction = "default"
            this.primaryTouch = null
            this.touchTracker.clear()
            this.clearPendingTouchMouseMove()
            this.clearTwoFingerRightClickGesture()
            return
        }
        for (const touch of event.changedTouches) {
            this.updateTouchTracker(touch)
        }

        if (this.config.touchMode == "touch") {
            for (const touch of event.changedTouches) {
                this.sendTouch(0, touch, rect)
            }
        } else if (this.config.touchMode == "mouseRelative" || this.config.touchMode == "pointAndDrag") {
            if (this.shouldTriggerTwoFingerRightClick(event)) {
                this.triggerTwoFingerRightClick(event, rect)
                event.preventDefault()
                event.stopPropagation()
                return
            }

            for (const touch of event.changedTouches) {
                if (this.primaryTouch == null) {
                    this.primaryTouch = touch.identifier
                    this.touchMouseAction = "default"
                    if (this.isAbsoluteTouchCursorMode()) {
                        this.sendMousePositionClientCoordinates(touch.clientX, touch.clientY, rect, true)
                    }
                }
            }

            if (this.primaryTouch != null && this.touchTracker.size == 2) {
                const primaryTouch = this.touchTracker.get(this.primaryTouch)
                if (primaryTouch && !primaryTouch.mouseMoved && !primaryTouch.mouseClicked) {
                    this.touchMouseAction = "scroll"
                    this.clearPendingTouchMouseMove()

                    if (this.config.touchMode == "pointAndDrag") {
                        let middleX = 0;
                        let middleY = 0;
                        for (const touch of this.touchTracker.values()) {
                            middleX += touch.x;
                            middleY += touch.y;
                        }
                        // Tracker size = 2 so there will only be 2 elements
                        middleX /= 2;
                        middleY /= 2;

                        primaryTouch.mouseMoved = true
                        this.sendMousePositionClientCoordinates(middleX, middleY, rect, true)
                    }
                }
            } else if (this.touchTracker.size == 3) {
                this.touchMouseAction = "screenKeyboard"
                this.clearPendingTouchMouseMove()
            }
        }
    }

    onTouchUpdate(rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        this.syncAndroidNativeMouseBridge(rect)
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            return
        }
        if (!this.isAbsoluteTouchCursorMode()
            && !this.isNativeRelativeTouchMode()
            && (this.config.touchMode == "mouseRelative" || this.config.touchMode == "pointAndDrag")
            && this.touchMouseAction == "default") {
            this.flushPendingTouchMouseMove(rect)
        }

        if (this.config.touchMode == "pointAndDrag") {
            if (this.primaryTouch == null) {
                return
            }
            const touch = this.touchTracker.get(this.primaryTouch)
            if (!touch) {
                return
            }

            const time = this.calcTouchTime(touch)
            if (this.touchMouseAction == "default" && !touch.mouseMoved && time >= TOUCH_AS_CLICK_MIN_TIME_MS) {
                this.sendMousePositionClientCoordinates(touch.originX, touch.originY, rect, true)

                touch.mouseMoved = true
            }
        }
    }

    onTouchMove(event: TouchEvent, rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        this.syncAndroidNativeMouseBridge(rect)
        this.noteTouchInteraction()
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            return
        }
        if (this.config.touchMode == "touch") {
            for (const touch of event.changedTouches) {
                this.sendTouch(1, touch, rect)
            }
        } else if (this.config.touchMode == "mouseRelative" || this.config.touchMode == "pointAndDrag") {
            if (this.consumeTwoFingerRightClickGesture(event)) {
                for (const touch of event.changedTouches) {
                    this.updateTouchTracker(touch)
                }
                return
            }

            for (const touch of event.changedTouches) {
                if (this.primaryTouch != touch.identifier) {
                    continue
                }
                const oldTouch = this.touchTracker.get(this.primaryTouch)
                if (!oldTouch) {
                    continue
                }

                // mouse move
                const movementX = (touch.clientX - oldTouch.x) * this.config.touchSensitivity;
                const movementY = (touch.clientY - oldTouch.y) * this.config.touchSensitivity;
                const [shapedMovementX, shapedMovementY] = this.shapeTouchMouseMovement(movementX, movementY)

                if (this.touchMouseAction == "default") {
                    if (this.isAbsoluteTouchCursorMode()) {
                        this.clearPendingTouchMouseMove()
                        this.sendMousePositionClientCoordinates(touch.clientX, touch.clientY, rect, false)
                        const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                            detail: {
                                deltaX: shapedMovementX,
                                deltaY: shapedMovementY,
                                x: touch.clientX,
                                y: touch.clientY,
                                source: "touchFollow"
                            }
                        })
                        this.eventTarget.dispatchEvent(previewEvent)
                    } else if (this.isNativeRelativeTouchMode()) {
                        this.clearPendingTouchMouseMove()
                        this.sendTouchRelativeMouseMoveClientCoordinates(shapedMovementX, shapedMovementY, rect)
                        const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                            detail: {
                                deltaX: shapedMovementX,
                                deltaY: shapedMovementY,
                                x: Number.NaN,
                                y: Number.NaN,
                                source: "touchRelative"
                            }
                        })
                        this.eventTarget.dispatchEvent(previewEvent)
                    } else {
                        this.clearPendingTouchMouseMove()
                        this.sendMouseMoveClientCoordinates(shapedMovementX, shapedMovementY, rect)
                        if (rect.width > 0 && rect.height > 0 && this.streamerSize[0] > 0 && this.streamerSize[1] > 0) {
                            const previewEvent: RelativeMousePreviewEvent = new CustomEvent("ml-relativemousepreview", {
                                detail: {
                                    deltaX: shapedMovementX,
                                    deltaY: shapedMovementY,
                                    x: Number.NaN,
                                    y: Number.NaN,
                                    source: "touchRelative"
                                }
                            })
                            this.eventTarget.dispatchEvent(previewEvent)
                        }
                    }

                    const distance = this.calcTouchOriginDistance(touch, oldTouch)
                    if (this.config.touchMode == "pointAndDrag" && distance > TOUCH_AS_CLICK_MAX_DISTANCE) {
                        if (!oldTouch.mouseMoved) {
                            this.sendMousePositionClientCoordinates(touch.clientX, touch.clientY, rect, true)
                            oldTouch.mouseMoved = true
                        }

                        if (!oldTouch.mouseClicked) {
                            this.sendMousePositionClientCoordinates(oldTouch.originX, oldTouch.originY, rect, true)
                            this.sendMouseButton(true, StreamMouseButton.LEFT)
                            oldTouch.mouseClicked = true
                        }
                    }
                } else if (this.touchMouseAction == "scroll") {
                    // inverting horizontal scroll
                    if (this.config.mouseScrollMode == "highres") {
                        this.sendMouseWheelHighRes(-movementX * TOUCH_HIGH_RES_SCROLL_MULTIPLIER, movementY * TOUCH_HIGH_RES_SCROLL_MULTIPLIER)
                    } else if (this.config.mouseScrollMode == "normal") {
                        this.sendMouseWheel(-movementX * TOUCH_SCROLL_MULTIPLIER, movementY * TOUCH_SCROLL_MULTIPLIER)
                    }
                } else if (this.touchMouseAction == "screenKeyboard") {
                    const distanceY = touch.clientY - oldTouch.originY

                    if (distanceY < -TOUCHES_AS_KEYBOARD_DISTANCE) {
                        const customEvent: ScreenKeyboardSetVisibleEvent = new CustomEvent("ml-screenkeyboardvisible", {
                            detail: { visible: true }
                        })
                        this.eventTarget.dispatchEvent(customEvent)
                    } else if (distanceY > TOUCHES_AS_KEYBOARD_DISTANCE) {
                        const customEvent: ScreenKeyboardSetVisibleEvent = new CustomEvent("ml-screenkeyboardvisible", {
                            detail: { visible: false }
                        })
                        this.eventTarget.dispatchEvent(customEvent)
                    }
                }
            }
        }

        for (const touch of event.changedTouches) {
            this.updateTouchTracker(touch)
        }
    }

    onTouchEnd(event: TouchEvent, rect: DOMRect) {
        if (this.transitionLocked) {
            return
        }
        this.syncAndroidNativeMouseBridge(rect)
        this.noteTouchInteraction()
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            this.touchTracker.clear()
            this.primaryTouch = null
            this.touchMouseAction = "default"
            this.clearPendingTouchMouseMove()
            this.clearTwoFingerRightClickGesture()
            return
        }
        if (this.config.touchMode == "touch") {
            for (const touch of event.changedTouches) {
                this.sendTouch(2, touch, rect)
            }
        } else if (this.config.touchMode == "mouseRelative" || this.config.touchMode == "pointAndDrag") {
            if (this.twoFingerRightClickActive) {
                event.preventDefault()
                event.stopPropagation()
                this.primaryTouch = null
                this.touchMouseAction = "default"
                this.clearTwoFingerRightClickGesture()
                for (const touch of event.changedTouches) {
                    this.touchTracker.delete(touch.identifier)
                }
                return
            }

            for (const touch of event.changedTouches) {
                if (this.primaryTouch != touch.identifier) {
                    continue
                }
                const oldTouch = this.touchTracker.get(this.primaryTouch)
                this.primaryTouch = null
                if (!this.isAbsoluteTouchCursorMode()) {
                    this.flushPendingTouchMouseMove(rect, true)
                }

                if (oldTouch) {
                    const time = this.calcTouchTime(oldTouch)
                    const distance = this.calcTouchOriginDistance(touch, oldTouch)

                    if (this.touchMouseAction == "default") {
                        if (distance <= TOUCH_AS_CLICK_MAX_DISTANCE) {
                            if (time <= this.config.touchLongPressMs || oldTouch.mouseClicked) {
                                if (this.isAbsoluteTouchCursorMode()) {
                                    this.sendMousePositionClientCoordinates(touch.clientX, touch.clientY, rect, true)
                                } else if (!this.isNativeRelativeTouchMode() && this.config.touchMode == "pointAndDrag" && !oldTouch.mouseMoved) {
                                    this.sendMousePositionClientCoordinates(touch.clientX, touch.clientY, rect, true)
                                }
                                if (!oldTouch.mouseClicked) {
                                    this.sendMouseButton(true, StreamMouseButton.LEFT)
                                }
                                this.sendMouseButton(false, StreamMouseButton.LEFT)
                            } else {
                                this.sendMouseButton(true, StreamMouseButton.RIGHT)
                                this.sendMouseButton(false, StreamMouseButton.RIGHT)
                            }
                        } else if (this.config.touchMode == "pointAndDrag") {
                            this.sendMouseButton(true, StreamMouseButton.LEFT)
                            this.sendMouseButton(false, StreamMouseButton.LEFT)
                        }
                    }
                }
            }
        }

        for (const touch of event.changedTouches) {
            this.touchTracker.delete(touch.identifier)
        }

        if (!event.touches || event.touches.length == 0) {
            this.clearPendingTouchMouseMove()
        }
    }

    onTouchCancel(event: TouchEvent, rect: DOMRect) {
        this.syncAndroidNativeMouseBridge(rect)
        this.clearTwoFingerRightClickGesture()
        this.clearPendingTouchMouseMove()
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            this.touchTracker.clear()
            this.primaryTouch = null
            this.touchMouseAction = "default"
            return
        }
        this.onTouchEnd(event, rect)
    }

    private calcNormalizedPosition(clientX: number, clientY: number, rect: DOMRect): [number, number] | null {
        const x = (clientX - rect.left) / rect.width
        const y = (clientY - rect.top) / rect.height

        if (x < 0 || x > 1.0 || y < 0 || y > 1.0) {
            // invalid touch
            return null
        }
        return [x, y]
    }
    private sendTouch(type: number, touch: Touch, rect: DOMRect) {
        this.buffer.reset()

        this.buffer.putU8(type)

        this.buffer.putU32(touch.identifier)

        const position = this.calcNormalizedPosition(touch.clientX, touch.clientY, rect)
        if (!position) {
            return
        }
        const [x, y] = position
        this.buffer.putF32(x)
        this.buffer.putF32(y)

        this.buffer.putF32(touch.force)

        this.buffer.putF32(touch.radiusX)
        this.buffer.putF32(touch.radiusY)
        this.buffer.putU16(touch.rotationAngle)

        trySendChannel(this.touch, this.buffer)
    }

    isTouchSupported(): boolean | null {
        return this.touchSupported
    }

    getCurrentPredictedTouchAction(): PredictedTouchAction {
        return this.touchMouseAction
    }

    getTouchDebugSnapshot(): TouchDebugSnapshot {
        return {
            touchMode: this.config.touchMode,
            predictedAction: this.touchMouseAction,
            trackedTouches: this.touchTracker.size,
            primaryTouchActive: this.primaryTouch != null,
            twoFingerRightClickActive: this.twoFingerRightClickActive,
            twoFingerRightClickTrackedTouches: this.twoFingerRightClickTouchIds.size,
            touchSupported: this.touchSupported,
            nativeMouseEngine: this.isAndroidNativeRelativeMouseBridgeActive() ? "native" : "browser",
            nativeMouseBridgeAvailable: this.androidNativeMouseBridgeStatus.available,
            nativeMouseBridgeProvider: this.androidNativeMouseBridgeStatus.provider,
            nativeMouseBridgeReason: this.androidNativeMouseBridgeStatus.reason
        }
    }

    shouldRunTouchUpdateLoop(): boolean {
        if (this.isAndroidNativeRelativeMouseBridgeActive()) {
            return false
        }

        if (this.isAbsoluteTouchCursorMode()) {
            return false
        }

        if (this.isNativeRelativeTouchMode()) {
            return false
        }

        if (this.config.touchMode == "pointAndDrag") {
            return true
        }

        if (this.touchMouseAction == "scroll" || this.touchMouseAction == "screenKeyboard") {
            return true
        }

        return this.pendingTouchMouseMoveX != 0 || this.pendingTouchMouseMoveY != 0
    }

    shouldRunGamepadUpdateLoop(): boolean {
        if (this.virtualControllerEnabled || this.virtualControllerConnected) {
            return true
        }

        return this.gamepads.some((gamepad) => gamepad != null)
    }

    // -- Controller
    // Wait for stream to connect and then send controllers
    private bufferedControllers: Array<number> = []
    private registerBufferedControllers() {
        const gamepads = navigator.getGamepads()

        for (const index of this.bufferedControllers.splice(0)) {
            const gamepad = gamepads[index]
            if (gamepad) {
                this.onGamepadConnect(gamepad)
            }
        }
    }

    private collectActuators(gamepad: Gamepad): Array<GamepadHapticActuator> {
        const actuators = []
        if ("vibrationActuator" in gamepad && gamepad.vibrationActuator) {
            actuators.push(gamepad.vibrationActuator)
        }
        if ("hapticActuators" in gamepad && gamepad.hapticActuators) {
            const hapticActuators = gamepad.hapticActuators as Array<GamepadHapticActuator>
            actuators.push(...hapticActuators)
        }
        return actuators
    }

    setVirtualControllerEnabled(enabled: boolean): boolean {
        this.virtualControllerEnabled = !!enabled

        if (!this.connected) {
            return this.virtualControllerEnabled
        }

        if (this.virtualControllerEnabled) {
            this.ensureVirtualControllerConnected()
        } else {
            this.disconnectVirtualController()
        }

        return this.virtualControllerEnabled
    }

    isVirtualControllerEnabled(): boolean {
        return this.virtualControllerEnabled
    }

    isVirtualControllerConnected(): boolean {
        return this.virtualControllerConnected
    }

    setVirtualControllerButtonPressed(buttonFlag: number, isPressed: boolean): boolean {
        if (!this.connected || !this.virtualControllerEnabled) {
            return false
        }

        if (!this.ensureVirtualControllerConnected()) {
            return false
        }

        this.cancelVirtualControllerRelease(buttonFlag)
        this.setVirtualControllerButton(buttonFlag, isPressed)
        return true
    }

    setVirtualControllerTrigger(side: "left" | "right", value: number): boolean {
        if (!this.connected || !this.virtualControllerEnabled) {
            return false
        }

        if (!this.ensureVirtualControllerConnected()) {
            return false
        }

        const nextValue = Math.max(0.0, Math.min(1.0, value))
        this.cancelVirtualControllerTriggerRelease(side)
        if (side == "left") {
            this.virtualControllerState.leftTrigger = nextValue
        } else {
            this.virtualControllerState.rightTrigger = nextValue
        }

        this.sendController(this.virtualControllerId, this.virtualControllerState)
        return true
    }

    pulseVirtualControllerTrigger(side: "left" | "right", durationMs: number = 110): boolean {
        if (!this.connected || !this.virtualControllerEnabled) {
            return false
        }

        if (!this.ensureVirtualControllerConnected()) {
            return false
        }

        this.setVirtualControllerTrigger(side, 1.0)
        this.scheduleVirtualControllerTriggerRelease(side, durationMs)
        return true
    }

    setVirtualControllerStick(side: "left" | "right", x: number, y: number): boolean {
        if (!this.connected || !this.virtualControllerEnabled) {
            return false
        }

        if (!this.ensureVirtualControllerConnected()) {
            return false
        }

        const nextX = Math.max(-1.0, Math.min(1.0, x))
        const nextY = Math.max(-1.0, Math.min(1.0, y))
        if (side == "left") {
            this.virtualControllerState.leftStickX = nextX
            this.virtualControllerState.leftStickY = nextY
        } else {
            this.virtualControllerState.rightStickX = nextX
            this.virtualControllerState.rightStickY = nextY
        }

        this.sendController(this.virtualControllerId, this.virtualControllerState)
        return true
    }

    resetVirtualControllerMotion(): boolean {
        if (!this.connected || !this.virtualControllerEnabled || !this.virtualControllerConnected) {
            return false
        }

        this.virtualControllerState.leftTrigger = 0
        this.virtualControllerState.rightTrigger = 0
        this.virtualControllerState.leftStickX = 0
        this.virtualControllerState.leftStickY = 0
        this.virtualControllerState.rightStickX = 0
        this.virtualControllerState.rightStickY = 0
        this.sendController(this.virtualControllerId, this.virtualControllerState)
        return true
    }

    resetVirtualControllerState(): boolean {
        if (!this.connected || !this.virtualControllerEnabled || !this.virtualControllerConnected) {
            return false
        }

        this.virtualControllerState = emptyGamepadState()
        this.sendController(this.virtualControllerId, this.virtualControllerState)
        return true
    }

    pulseVirtualControllerButton(buttonFlag: number, durationMs: number = 110): boolean {
        if (!this.connected || !this.virtualControllerEnabled) {
            return false
        }

        if (!this.ensureVirtualControllerConnected()) {
            return false
        }

        this.setVirtualControllerButton(buttonFlag, true)
        this.scheduleVirtualControllerRelease(buttonFlag, durationMs)
        return true
    }

    reconnectVirtualController(): boolean {
        if (!this.connected) {
            return false
        }

        if (!this.virtualControllerEnabled) {
            this.virtualControllerEnabled = true
        }

        this.disconnectVirtualController()
        return this.ensureVirtualControllerConnected()
    }

    setVirtualControllerPromptStyle(style: "xbox" | "ds4"): boolean {
        const next = getVirtualControllerAdvertisement(style)
        const changed = this.virtualControllerType != next.type
            || this.virtualControllerSupportedButtons != next.supportedButtons
            || this.virtualControllerCapabilities != next.capabilities

        if (!changed) {
            return false
        }

        this.virtualControllerType = next.type
        this.virtualControllerSupportedButtons = next.supportedButtons
        this.virtualControllerCapabilities = next.capabilities

        if (this.connected && this.virtualControllerConnected) {
            return this.reconnectVirtualController()
        }

        return true
    }

    private ensureVirtualControllerConnected(): boolean {
        if (!this.virtualControllerEnabled || !this.connected) {
            return false
        }

        if (this.virtualControllerConnected) {
            return true
        }

        this.virtualControllerState = emptyGamepadState()
        this.sendControllerAdd(
            this.virtualControllerId,
            this.virtualControllerType,
            this.virtualControllerSupportedButtons,
            this.virtualControllerCapabilities
        )
        this.virtualControllerConnected = true
        return true
    }

    private disconnectVirtualController() {
        for (const timerId of this.virtualControllerReleaseTimers.values()) {
            window.clearTimeout(timerId)
        }
        this.virtualControllerReleaseTimers.clear()
        for (const timerId of this.virtualControllerTriggerReleaseTimers.values()) {
            window.clearTimeout(timerId)
        }
        this.virtualControllerTriggerReleaseTimers.clear()

        if (this.connected && this.virtualControllerConnected) {
            this.virtualControllerState = emptyGamepadState()
            this.sendController(this.virtualControllerId, this.virtualControllerState)
            this.sendControllerRemove(this.virtualControllerId)
        }

        this.virtualControllerConnected = false
        this.virtualControllerState = emptyGamepadState()
    }

    private setVirtualControllerButton(buttonFlag: number, isPressed: boolean) {
        if (!this.virtualControllerConnected) {
            return
        }

        if (isPressed) {
            this.virtualControllerState.buttonFlags |= buttonFlag
        } else {
            this.virtualControllerState.buttonFlags &= ~buttonFlag
        }

        this.sendController(this.virtualControllerId, this.virtualControllerState)
    }

    private scheduleVirtualControllerRelease(buttonFlag: number, durationMs: number) {
        this.cancelVirtualControllerRelease(buttonFlag)

        const nextTimerId = window.setTimeout(() => {
            this.virtualControllerReleaseTimers.delete(buttonFlag)
            this.setVirtualControllerButton(buttonFlag, false)
        }, Math.max(40, durationMs))

        this.virtualControllerReleaseTimers.set(buttonFlag, nextTimerId)
    }

    private cancelVirtualControllerRelease(buttonFlag: number) {
        const previousTimerId = this.virtualControllerReleaseTimers.get(buttonFlag)
        if (previousTimerId != null) {
            window.clearTimeout(previousTimerId)
            this.virtualControllerReleaseTimers.delete(buttonFlag)
        }
    }

    private scheduleVirtualControllerTriggerRelease(side: "left" | "right", durationMs: number) {
        this.cancelVirtualControllerTriggerRelease(side)

        const nextTimerId = window.setTimeout(() => {
            this.virtualControllerTriggerReleaseTimers.delete(side)
            this.setVirtualControllerTrigger(side, 0.0)
        }, Math.max(40, durationMs))

        this.virtualControllerTriggerReleaseTimers.set(side, nextTimerId)
    }

    private cancelVirtualControllerTriggerRelease(side: "left" | "right") {
        const previousTimerId = this.virtualControllerTriggerReleaseTimers.get(side)
        if (previousTimerId != null) {
            window.clearTimeout(previousTimerId)
            this.virtualControllerTriggerReleaseTimers.delete(side)
        }
    }

    private gamepads: Array<{ gamepadIndex: number, oldState: GamepadState } | null> = []
    private gamepadRumbleInterval: number | null = null

    private getOrCreateGamepadRumbleState(id: number) {
        const current = this.gamepadRumbleCurrent[id]
        if (current) {
            return current
        }

        const next = { lowFrequencyMotor: 0, highFrequencyMotor: 0, leftTrigger: 0, rightTrigger: 0 }
        this.gamepadRumbleCurrent[id] = next
        return next
    }

    onGamepadConnect(gamepad: Gamepad) {
        if (!this.connected) {
            this.bufferedControllers.push(gamepad.index)
            return
        }

        if (this.gamepads.find(value => value?.gamepadIndex == gamepad.index)) {
            return
        }

        let id = -1
        for (let i = 0; i < this.gamepads.length; i++) {
            if (this.gamepads[i] == null) {
                this.gamepads[i] = { gamepadIndex: gamepad.index, oldState: emptyGamepadState() }
                id = i
                break
            }
        }
        if (id == -1) {
            id = this.gamepads.length
            this.gamepads.push({ gamepadIndex: gamepad.index, oldState: emptyGamepadState() })
        }

        // Start Rumble interval
        if (this.gamepadRumbleInterval == null) {
            this.gamepadRumbleInterval = window.setInterval(this.onGamepadRumbleInterval.bind(this), CONTROLLER_RUMBLE_INTERVAL_MS - 10)
        }

        // Reset rumble
        this.gamepadRumbleCurrent[id] = { lowFrequencyMotor: 0, highFrequencyMotor: 0, leftTrigger: 0, rightTrigger: 0 }

        const advertisement = getGamepadAdvertisement(gamepad)
        let capabilities = advertisement.capabilities

        // Rumble capabilities
        for (const actuator of this.collectActuators(gamepad)) {
            if ("effects" in actuator) {
                const supportedEffects = actuator.effects as Array<string>

                for (const effect of supportedEffects) {
                    if (effect == "dual-rumble") {
                        capabilities |= StreamControllerCapabilities.CAPABILITY_RUMBLE
                    } else if (effect == "trigger-rumble") {
                        capabilities |= StreamControllerCapabilities.CAPABILITY_TRIGGER_RUMBLE
                    }
                }
            } else if ("type" in actuator && (actuator.type == "vibration" || actuator.type == "dual-rumble")) {
                capabilities |= StreamControllerCapabilities.CAPABILITY_RUMBLE
            } else if ("playEffect" in actuator && typeof actuator.playEffect == "function") {
                // we're just hoping at this point
                capabilities |= StreamControllerCapabilities.CAPABILITY_RUMBLE | StreamControllerCapabilities.CAPABILITY_TRIGGER_RUMBLE
            } else if ("pulse" in actuator && typeof actuator.pulse == "function") {
                capabilities |= StreamControllerCapabilities.CAPABILITY_RUMBLE
            }
        }

        this.sendControllerAdd(id, advertisement.type, advertisement.supportedButtons, capabilities)

        if (gamepad.mapping != "standard") {
            console.warn(`[Gamepad]: Using best-effort mapping for ${gamepad.mapping || "unknown"} gamepad input`)
        }
    }
    onGamepadDisconnect(event: GamepadEvent) {
        const index = this.gamepads.findIndex(value => value?.gamepadIndex == event.gamepad.index)
        if (index != -1) {
            this.sendControllerRemove(index)
            this.gamepads[index] = null
            this.gamepadRumbleCurrent[index] = {
                lowFrequencyMotor: 0,
                highFrequencyMotor: 0,
                leftTrigger: 0,
                rightTrigger: 0
            }
        }
    }

    private lastGamepadUpdate: number = performance.now()
    onGamepadUpdate() {
        if (this.config.controllerConfig.sendIntervalOverride != null) {
            const now = performance.now()
            if (now - this.lastGamepadUpdate < (1000 / this.config.controllerConfig.sendIntervalOverride)) {
                return
            }
            this.lastGamepadUpdate = performance.now()
        }

        for (let gamepadId = 0; gamepadId < this.gamepads.length; gamepadId++) {
            const oldGamepadState = this.gamepads[gamepadId]
            if (oldGamepadState == null) {
                continue
            }
            const gamepad = navigator.getGamepads()[oldGamepadState.gamepadIndex]
            if (!gamepad) {
                continue
            }

            const state = extractGamepadState(gamepad, this.config.controllerConfig)
            if (areGamepadStatesEqual(state, oldGamepadState.oldState)) {
                continue
            }
            oldGamepadState.oldState = state

            this.sendController(gamepadId, state)
        }
    }

    private onControllerData(data: ArrayBuffer) {
        this.buffer.reset()

        this.buffer.putU8Array(new Uint8Array(data))
        this.buffer.flip()

        // TODO: maybe move this into their respective controller channels?

        const ty = this.buffer.getU8()
        if (ty == 0) {
            // Rumble
            const id = this.buffer.getU8()
            const lowFrequencyMotor = this.buffer.getU16() / U16_MAX
            const highFrequencyMotor = this.buffer.getU16() / U16_MAX

            const gamepadIndex = this.gamepads[id]?.gamepadIndex
            if (gamepadIndex == null) {
                return
            }

            this.setGamepadEffect(id, "dual-rumble", { lowFrequencyMotor, highFrequencyMotor })
        } else if (ty == 1) {
            // Trigger Rumble
            const id = this.buffer.getU8()
            const leftTrigger = this.buffer.getU16() / U16_MAX
            const rightTrigger = this.buffer.getU16() / U16_MAX

            const gamepadIndex = this.gamepads[id]?.gamepadIndex
            if (gamepadIndex == null) {
                return
            }

            this.setGamepadEffect(id, "trigger-rumble", { leftTrigger, rightTrigger })
        }
    }

    // -- Controller rumble
    private gamepadRumbleCurrent: Array<{
        lowFrequencyMotor: number, highFrequencyMotor: number,
        leftTrigger: number, rightTrigger: number
    }> = []

    private setGamepadEffect(id: number, ty: "dual-rumble", params: { lowFrequencyMotor: number, highFrequencyMotor: number }): void
    private setGamepadEffect(id: number, ty: "trigger-rumble", params: { leftTrigger: number, rightTrigger: number }): void

    private setGamepadEffect(id: number, _ty: "dual-rumble" | "trigger-rumble", params: { lowFrequencyMotor: number, highFrequencyMotor: number } | { leftTrigger: number, rightTrigger: number }) {
        const rumble = this.getOrCreateGamepadRumbleState(id)

        Object.assign(rumble, params)
    }

    private onGamepadRumbleInterval() {
        for (let id = 0; id < this.gamepads.length; id++) {
            const gamepadIndex = this.gamepads[id]?.gamepadIndex
            if (gamepadIndex == null) {
                continue
            }

            const rumble = this.gamepadRumbleCurrent[id]
            const gamepad = navigator.getGamepads()[gamepadIndex]
            if (gamepad && rumble) {
                this.refreshGamepadRumble(rumble, gamepad)
            }
        }
    }
    private refreshGamepadRumble(
        rumble: {
            lowFrequencyMotor: number, highFrequencyMotor: number,
            leftTrigger: number, rightTrigger: number
        },
        gamepad: Gamepad
    ) {
        // Browsers are making this more complicated than it is

        const actuators = this.collectActuators(gamepad)

        for (const actuator of actuators) {
            if ("effects" in actuator) {
                const supportedEffects = actuator.effects as Array<string>

                for (const effect of supportedEffects) {
                    if (effect == "dual-rumble") {
                        actuator.playEffect("dual-rumble", {
                            duration: CONTROLLER_RUMBLE_INTERVAL_MS,
                            weakMagnitude: rumble.lowFrequencyMotor,
                            strongMagnitude: rumble.highFrequencyMotor
                        })
                    } else if (effect == "trigger-rumble") {
                        actuator.playEffect("trigger-rumble", {
                            duration: CONTROLLER_RUMBLE_INTERVAL_MS,
                            leftTrigger: rumble.leftTrigger,
                            rightTrigger: rumble.rightTrigger
                        })
                    }
                }
            } else if ("type" in actuator && (actuator.type == "vibration" || actuator.type == "dual-rumble")) {
                actuator.playEffect(actuator.type as any, {
                    duration: CONTROLLER_RUMBLE_INTERVAL_MS,
                    weakMagnitude: rumble.lowFrequencyMotor,
                    strongMagnitude: rumble.highFrequencyMotor
                })
            } else if ("playEffect" in actuator && typeof actuator.playEffect == "function") {
                actuator.playEffect("dual-rumble", {
                    duration: CONTROLLER_RUMBLE_INTERVAL_MS,
                    weakMagnitude: rumble.lowFrequencyMotor,
                    strongMagnitude: rumble.highFrequencyMotor
                })
                actuator.playEffect("trigger-rumble", {
                    duration: CONTROLLER_RUMBLE_INTERVAL_MS,
                    leftTrigger: rumble.leftTrigger,
                    rightTrigger: rumble.rightTrigger
                })
            } else if ("pulse" in actuator && typeof actuator.pulse == "function") {
                const weak = Math.min(Math.max(rumble.lowFrequencyMotor, 0), 1);
                const strong = Math.min(Math.max(rumble.highFrequencyMotor, 0), 1);

                const average = (weak + strong) / 2.0

                actuator.pulse(average, CONTROLLER_RUMBLE_INTERVAL_MS)
            }
        }
    }

    // -- Controller Sending
    sendControllerAdd(id: number, controllerType: number, supportedButtons: number, capabilities: number) {
        this.buffer.reset()

        this.buffer.putU8(0)
        this.buffer.putU8(id)
        this.buffer.putU8(controllerType)
        this.buffer.putU32(supportedButtons)
        this.buffer.putU16(capabilities)

        trySendChannel(this.controllers, this.buffer)
    }
    sendControllerRemove(id: number) {
        this.buffer.reset()

        this.buffer.putU8(1)
        this.buffer.putU8(id)

        trySendChannel(this.controllers, this.buffer)
    }
    // Values
    // - Trigger: range 0..1
    // - Stick: range -1..1
    sendController(id: number, state: GamepadState) {
        const PACKET_SIZE_BYTES = 1 + 4 + 1 + 1 + 2 + 2 + 2 + 2;

        const controllerChannel = this.controllerInputs[id]

        const estimatedBufferedBytes = controllerChannel?.estimatedBufferedBytes()
        if (controllerChannel && estimatedBufferedBytes != null && estimatedBufferedBytes > PACKET_SIZE_BYTES) {
            // Only send packets when we can handle them
            if (debugInputEventsEnabled()) {
                console.debug(`dropping controller packet for ${id} because the buffer amount is large enough: ${controllerChannel.estimatedBufferedBytes()}`)
            }
            return
        }

        this.buffer.reset()

        this.buffer.putU8(0)
        this.buffer.putU32(state.buttonFlags)
        this.buffer.putU8(Math.max(0.0, Math.min(1.0, state.leftTrigger)) * U8_MAX)
        this.buffer.putU8(Math.max(0.0, Math.min(1.0, state.rightTrigger)) * U8_MAX)
        this.buffer.putI16(Math.max(-1.0, Math.min(1.0, state.leftStickX)) * I16_MAX)
        this.buffer.putI16(Math.max(-1.0, Math.min(1.0, -state.leftStickY)) * I16_MAX)
        this.buffer.putI16(Math.max(-1.0, Math.min(1.0, state.rightStickX)) * I16_MAX)
        this.buffer.putI16(Math.max(-1.0, Math.min(1.0, -state.rightStickY)) * I16_MAX)

        trySendChannel(this.controllerInputs[id], this.buffer)
    }

}
