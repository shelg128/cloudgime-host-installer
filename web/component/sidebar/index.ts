import { Component } from "../index.js"
import { showErrorPopup } from "../error.js"

export interface Sidebar extends Component {
    extended(): void
    unextend(): void
}

export type SidebarEdge = "up" | "down" | "left" | "right"
export type SidebarStyle = {
    edge?: SidebarEdge
}

const AUTO_HIDE_DELAY_MS = 2400
const EDGE_REVEAL_THRESHOLD_PX = 22
const EDGE_REVEAL_AXIS_WINDOW_PX = 180
const EDGE_REVEAL_ANDROID_THRESHOLD_PX = 56
const EDGE_REVEAL_ANDROID_RIGHT_THRESHOLD_PX = 28
const SWIPE_REVEAL_DISTANCE_PX = 24
const SUPPRESSED_REVEAL_DURATION_MS = 4200
const ANDROID_RIGHT_FLOATING_EDGE_PX = 18
const ANDROID_BOTTOM_RIGHT_ZONE_PX = 84
const DEFAULT_EDGE: SidebarEdge = "left"
export const BROWSER_NATIVE_BRIDGE_FLAG_KEY = "ML_BROWSER_NATIVE_BRIDGE"
export const BROWSER_LEGACY_HANDLES_FLAG_KEY = "ML_BROWSER_LEGACY_HANDLES"
export const FORCED_BROWSER_NATIVE_BRIDGE_CLASS = "stream-force-native-bridge-primary"

type TouchRevealMode = "sidebar" | "parentFloating" | "suppressedSidebar"
const STREAM_EDGE_REVEAL_EVENT = "pccloud:stream-edge-reveal"
export const STREAM_SIDEBAR_STATE_EVENT = "pccloud:stream-sidebar-state"

let sidebarExtended = false
const sidebarRoot = document.getElementById("sidebar-root")
const sidebarParent = document.getElementById("sidebar-parent")
const sidebarButton = document.getElementById("sidebar-button")

let sidebarComponent: Sidebar | null = null
let preferredEdge: SidebarEdge = DEFAULT_EDGE
let autoHideTimer: number | null = null
let lastProbeAt = 0
let activeTouchReveal: { startX: number, startY: number, mode: TouchRevealMode } | null = null
let sidebarHandleTouchReveal: { startX: number, startY: number } | null = null
let sidebarContentTouchGesture: { identifier: number, startX: number, startY: number, moved: boolean } | null = null
let sidebarContentSuppressClickUntil = 0
let sidebarSuppressed = false
let suppressedRevealTimer: number | null = null
let suppressedRevealUntil = 0
let uiOverlayActive = false
let sidebarPinnedByInteraction = false

function readLocalStorageFlag(key: string): boolean | null {
    try {
        const raw = window.localStorage.getItem(key)
        if (raw == null) {
            return null
        }

        const normalized = raw.trim().toLowerCase()
        if (["1", "true", "yes", "on", "enabled"].indexOf(normalized) >= 0) {
            return true
        }
        if (["0", "false", "no", "off", "disabled"].indexOf(normalized) >= 0) {
            return false
        }
    } catch {
        return null
    }

    return null
}

export function isBrowserNativeBridgePrimaryEnabled(): boolean {
    const localStoragePreference = readLocalStorageFlag(BROWSER_NATIVE_BRIDGE_FLAG_KEY)
    if (localStoragePreference != null) {
        return localStoragePreference
    }

    return document.documentElement.classList.contains(FORCED_BROWSER_NATIVE_BRIDGE_CLASS)
        || !!document.body?.classList.contains(FORCED_BROWSER_NATIVE_BRIDGE_CLASS)
}

export function areBrowserLegacyHandlesEnabled(): boolean {
    return readLocalStorageFlag(BROWSER_LEGACY_HANDLES_FLAG_KEY) ?? false
}

function shouldUseLegacyBrowserHandles(): boolean {
    return !isBrowserNativeBridgePrimaryEnabled() || areBrowserLegacyHandlesEnabled()
}

export function applyBrowserNativeBridgeModeClasses() {
    const nativeBridgePrimary = isBrowserNativeBridgePrimaryEnabled()
    const legacyHandlesEnabled = areBrowserLegacyHandlesEnabled()
    const legacyHandlesHidden = nativeBridgePrimary && !legacyHandlesEnabled
    const root = document.documentElement
    const body = document.body

    root.classList.toggle("stream-browser-native-bridge-primary", nativeBridgePrimary)
    body?.classList.toggle("stream-browser-native-bridge-primary", nativeBridgePrimary)
    root.classList.toggle("stream-browser-legacy-handles-enabled", legacyHandlesEnabled)
    body?.classList.toggle("stream-browser-legacy-handles-enabled", legacyHandlesEnabled)
    root.classList.toggle("stream-browser-legacy-handles-hidden", legacyHandlesHidden)
    body?.classList.toggle("stream-browser-legacy-handles-hidden", legacyHandlesHidden)

    sidebarRoot?.classList.toggle("stream-sidebar-legacy-handles-hidden", legacyHandlesHidden)
    sidebarButton?.classList.toggle("stream-sidebar-trigger-hidden", legacyHandlesHidden)
    if (legacyHandlesHidden) {
        sidebarButton?.setAttribute("aria-hidden", "true")
        sidebarButton?.setAttribute("tabindex", "-1")
    } else {
        sidebarButton?.removeAttribute("aria-hidden")
        sidebarButton?.removeAttribute("tabindex")
    }
}

function syncSidebarOpenClass() {
    const open = sidebarExtended && !isSidebarSuppressed()
    document.documentElement.classList.toggle("stream-sidebar-open", open)
    document.body?.classList.toggle("stream-sidebar-open", open)
}

function syncSidebarChromeVisibilityClasses() {
    const hiddenByAutoState = !!sidebarRoot
        && sidebarRoot.style.visibility !== "hidden"
        && sidebarRoot.classList.contains("sidebar-auto-hidden")
        && !sidebarExtended
    const forceHidden = !sidebarRoot
        || sidebarRoot.style.visibility === "hidden"
        || sidebarRoot.classList.contains("sidebar-force-hidden")

    document.documentElement.classList.toggle("stream-sidebar-auto-hidden", hiddenByAutoState)
    document.body?.classList.toggle("stream-sidebar-auto-hidden", hiddenByAutoState)
    document.documentElement.classList.toggle("stream-sidebar-force-hidden", forceHidden)
    document.body?.classList.toggle("stream-sidebar-force-hidden", forceHidden)
}

function isAndroidTouchDevice(): boolean {
    const userAgent = String(navigator.userAgent || "")
    const touchPoints = Number(navigator.maxTouchPoints || 0)
    if (touchPoints <= 0) {
        return false
    }

    const coarsePointer = typeof window.matchMedia == "function" && window.matchMedia("(pointer: coarse)").matches
    const appleTabletDesktopUa = /Macintosh/i.test(userAgent) && touchPoints > 1
    const mobileLikeUserAgent = /Android|iPhone|iPad|iPod|Mobile|Tablet/i.test(userAgent) || appleTabletDesktopUa
    const desktopLikeUserAgent = /Windows NT|X11|CrOS/i.test(userAgent)
    const shortestEdge = Math.min(
        Math.max(window.innerWidth || 0, document.documentElement.clientWidth || 0),
        Math.max(window.innerHeight || 0, document.documentElement.clientHeight || 0)
    )

    return mobileLikeUserAgent || (coarsePointer && !desktopLikeUserAgent && shortestEdge <= 900)
}

function isMobileStreamGesturePriorityMode(): boolean {
    if (!isAndroidTouchDevice()) {
        return false
    }

    const root = document.documentElement
    const body = document.body
    return root.classList.contains("stream-mode-mobile")
        || !!body?.classList.contains("stream-mode-mobile")
        || root.classList.contains("ml-android-touch")
        || !!body?.classList.contains("ml-android-touch")
}

function hasVisibleRect(element: Element | null): element is HTMLElement {
    if (!(element instanceof HTMLElement)) {
        return false
    }

    const rect = element.getBoundingClientRect()
    return Number.isFinite(rect.width) && Number.isFinite(rect.height) && rect.width > 0 && rect.height > 0
}

function isPointWithinRect(x: number, y: number, rect: DOMRect, expandX = 0, expandY = expandX): boolean {
    return x >= (rect.left - expandX)
        && x <= (rect.right + expandX)
        && y >= (rect.top - expandY)
        && y <= (rect.bottom + expandY)
}

function getSidebarTouchHandleRect(): DOMRect | null {
    if (!shouldUseLegacyBrowserHandles()) {
        return null
    }

    if (!hasVisibleRect(sidebarButton)) {
        return null
    }

    return sidebarButton.getBoundingClientRect()
}

function getAndroidBridgeTouchHandleRect(): DOMRect | null {
    const toggle = document.querySelector(".android-bridge-root .android-bridge-toggle")
    if (!hasVisibleRect(toggle)) {
        return null
    }

    return toggle.getBoundingClientRect()
}

function isNearAndroidFloatingRevealZone(x: number, y: number): boolean {
    const bridgeHandleRect = getAndroidBridgeTouchHandleRect()
    if (bridgeHandleRect) {
        return isPointWithinRect(x, y, bridgeHandleRect, 4, 8)
    }

    if (isMobileStreamGesturePriorityMode()) {
        return false
    }

    const width = Math.max(window.innerWidth || 0, document.documentElement.clientWidth || 0)
    const height = Math.max(window.innerHeight || 0, document.documentElement.clientHeight || 0)
    const nearRightEdge = x >= (width - ANDROID_RIGHT_FLOATING_EDGE_PX)
    const nearBottomRight = x >= (width - ANDROID_BOTTOM_RIGHT_ZONE_PX) && y >= (height - ANDROID_BOTTOM_RIGHT_ZONE_PX)
    return nearRightEdge || nearBottomRight
}

function getAndroidEdgeRevealThreshold(edge: SidebarEdge): number {
    if (edge === "right") {
        return EDGE_REVEAL_ANDROID_RIGHT_THRESHOLD_PX
    }
    return EDGE_REVEAL_ANDROID_THRESHOLD_PX
}

function getStreamDesktopRevealAnchor(): { x: number, y: number } | null {
    if (!shouldUseLegacyBrowserHandles()) {
        return null
    }

    if (!sidebarButton) {
        return null
    }

    const root = document.documentElement
    const body = document.body
    const streamDesktop = root.classList.contains("stream-mode-desktop")
        || body.classList.contains("stream-mode-desktop")
    if (!streamDesktop) {
        return null
    }

    const rect = sidebarButton.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) {
        return null
    }

    return {
        x: rect.left + (rect.width * 0.5),
        y: rect.top + (rect.height * 0.5)
    }
}

function notifyParentRevealFloatingMenu() {
    if (!window.parent || window.parent === window) {
        return
    }
    window.parent.postMessage({ type: "pccloud-android-reveal-floating-menu" }, window.location.origin)
}

function setSidebarEdgeClass(edge: SidebarEdge) {
    sidebarRoot?.classList.remove("sidebar-edge-left", "sidebar-edge-right", "sidebar-edge-up", "sidebar-edge-down")
    sidebarRoot?.classList.add(`sidebar-edge-${edge}`)
}

function applySidebarPlacement() {
    if (!sidebarRoot) {
        return
    }

    setSidebarEdgeClass(preferredEdge)
    const root = document.documentElement
    const body = document.body
    const mobileMode = root.classList.contains("stream-mode-mobile")
        || body.classList.contains("stream-mode-mobile")
        || body.classList.contains("ml-android-touch")
        || isAndroidTouchDevice()
    sidebarRoot.style.setProperty("--sidebar-offset-percent", mobileMode ? "50%" : "23%")
}

function clearAutoHideTimer() {
    if (autoHideTimer != null) {
        window.clearTimeout(autoHideTimer)
        autoHideTimer = null
    }
}

function clearSuppressedRevealTimer() {
    if (suppressedRevealTimer != null) {
        window.clearTimeout(suppressedRevealTimer)
        suppressedRevealTimer = null
    }
}

function isUiOverlayLocked(): boolean {
    if (uiOverlayActive) {
        return true
    }

    const body = document.body
    if (!body) {
        return false
    }

    return body.classList.contains("vc-edit-mode-active") || body.classList.contains("vc-modal-open")
}

function isSuppressedRevealActive(): boolean {
    return sidebarSuppressed && Date.now() <= suppressedRevealUntil
}

function isDocumentFullscreen(): boolean {
    return typeof document !== "undefined" && "fullscreenElement" in document && !!document.fullscreenElement
}

function setSidebarAutoHidden(hidden: boolean) {
    if (!sidebarRoot) {
        return
    }

    sidebarRoot.classList.toggle("sidebar-auto-hidden", hidden && !sidebarExtended)
    syncSidebarChromeVisibilityClasses()
}

function hideSuppressedSidebarNow() {
    if (!sidebarRoot) {
        return
    }

    sidebarPinnedByInteraction = false
    suppressedRevealUntil = 0
    sidebarExtended = false
    setSidebarAutoHidden(false)
    sidebarRoot.classList.remove("sidebar-show")
    sidebarRoot.classList.remove("sidebar-auto-hidden")
    sidebarRoot.classList.add("sidebar-force-hidden")
    syncSidebarChromeVisibilityClasses()
}

function scheduleSuppressedSidebarHide() {
    clearSuppressedRevealTimer()
    if (!sidebarSuppressed) {
        return
    }

    suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
    if (sidebarExtended || sidebarPinnedByInteraction || isUiOverlayLocked()) {
        return
    }

    suppressedRevealTimer = window.setTimeout(() => {
        suppressedRevealTimer = null
        if (!sidebarSuppressed) {
            return
        }
        if (sidebarExtended || sidebarPinnedByInteraction || isUiOverlayLocked()) {
            scheduleSuppressedSidebarHide()
            return
        }
        if (Date.now() < suppressedRevealUntil) {
            scheduleSuppressedSidebarHide()
            return
        }
        hideSuppressedSidebarNow()
    }, SUPPRESSED_REVEAL_DURATION_MS)
}

function revealSuppressedSidebar(): boolean {
    if (!sidebarRoot || !sidebarComponent || sidebarRoot.style.visibility === "hidden") {
        return false
    }

    suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
    sidebarRoot.classList.remove("sidebar-force-hidden")
    setSidebarAutoHidden(false)
    syncSidebarChromeVisibilityClasses()
    if (sidebarExtended || sidebarPinnedByInteraction || isUiOverlayLocked()) {
        clearSuppressedRevealTimer()
    } else {
        scheduleSuppressedSidebarHide()
    }
    return true
}

function emitSidebarState() {
    window.dispatchEvent(new CustomEvent(STREAM_SIDEBAR_STATE_EVENT, {
        detail: { open: sidebarExtended }
    }))
}

function isEventInsideSidebar(target: EventTarget | null): boolean {
    if (!sidebarRoot || !(target instanceof Node)) {
        return false
    }

    if (sidebarRoot.contains(target)) {
        return true
    }

    return !!(sidebarButton && sidebarButton.contains(target))
}

function isSidebarContentTarget(target: EventTarget | null): target is HTMLElement {
    if (!(target instanceof HTMLElement)) {
        return false
    }

    return !!target.closest(".stream-sidebar-stage.sidebar-content, #sidebar-parent.sidebar-content, #sidebar-parent")
}

function isSidebarScrollableInteractiveTarget(target: EventTarget | null): target is HTMLElement {
    if (!(target instanceof HTMLElement) || !isSidebarContentTarget(target)) {
        return false
    }

    return !!target.closest("button, summary, [role='button'], .sidebar-v2-line, .sidebar-v2-action, .sidebar-v2-summary-action, .sidebar-stream-section-tab, .sidebar-stream-group-toggle")
}

function isSidebarSuppressed(): boolean {
    if (!sidebarRoot || sidebarRoot.style.visibility === "hidden") {
        return true
    }

    if (!sidebarSuppressed) {
        return false
    }

    if (sidebarExtended || sidebarPinnedByInteraction || isUiOverlayLocked()) {
        return false
    }

    return !isSuppressedRevealActive()
}

function pinSidebarByInteraction() {
    if (!sidebarRoot || isSidebarSuppressed()) {
        return
    }

    sidebarPinnedByInteraction = true
    clearAutoHideTimer()
    setSidebarAutoHidden(false)
    if (sidebarSuppressed) {
        clearSuppressedRevealTimer()
        suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
    }
}

function bumpSidebarAutoHideFromActivity() {
    if (!sidebarRoot) {
        return
    }

    if (sidebarSuppressed) {
        suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
        if (sidebarExtended || sidebarPinnedByInteraction || isUiOverlayLocked()) {
            clearSuppressedRevealTimer()
        } else {
            scheduleSuppressedSidebarHide()
        }
    } else if (isSidebarSuppressed()) {
        return
    }

    setSidebarAutoHidden(false)
    clearAutoHideTimer()

    if (!sidebarExtended && !sidebarPinnedByInteraction && !isUiOverlayLocked()) {
        scheduleSidebarAutoHide()
    }
}

function unpinSidebarByInteraction() {
    sidebarPinnedByInteraction = false
}

function hideSidebarFromOutsideInteraction() {
    if (!sidebarRoot) {
        return
    }

    unpinSidebarByInteraction()
    if (sidebarExtended) {
        setSidebarExtended(false)
    }

    clearAutoHideTimer()
    setSidebarAutoHidden(true)
}

function scheduleSidebarAutoHide() {
    clearAutoHideTimer()
    autoHideTimer = window.setTimeout(() => {
        autoHideTimer = null
        if (!sidebarExtended && !sidebarPinnedByInteraction && !isSidebarSuppressed()) {
            setSidebarAutoHidden(true)
        }
    }, AUTO_HIDE_DELAY_MS)
}

function revealSidebar() {
    if (!sidebarRoot || isSidebarSuppressed()) {
        return
    }

    bumpSidebarAutoHideFromActivity()
}

function isNearEdgeAxisWindow(x: number, y: number): boolean {
    const width = Math.max(window.innerWidth || 0, document.documentElement.clientWidth || 0)
    const height = Math.max(window.innerHeight || 0, document.documentElement.clientHeight || 0)

    if (isAndroidTouchDevice()) {
        const sidebarHandleRect = getSidebarTouchHandleRect()
        if (sidebarHandleRect) {
            return isPointWithinRect(x, y, sidebarHandleRect, 4, 10)
        }

        if (isMobileStreamGesturePriorityMode()) {
            return false
        }

        const threshold = getAndroidEdgeRevealThreshold(preferredEdge)
        if (preferredEdge === "left") {
            return x <= threshold
        }
        if (preferredEdge === "right") {
            return x >= (width - threshold)
        }
        if (preferredEdge === "up") {
            return y <= threshold
        }
        return y >= (height - threshold)
    }

    const streamDesktopAnchor = getStreamDesktopRevealAnchor()
    const anchorX = streamDesktopAnchor?.x ?? (width * 0.5)
    const anchorY = streamDesktopAnchor?.y ?? (height * 0.5)

    if (preferredEdge === "left") {
        return x <= EDGE_REVEAL_THRESHOLD_PX && Math.abs(y - anchorY) <= EDGE_REVEAL_AXIS_WINDOW_PX
    }
    if (preferredEdge === "right") {
        return x >= (width - EDGE_REVEAL_THRESHOLD_PX) && Math.abs(y - anchorY) <= EDGE_REVEAL_AXIS_WINDOW_PX
    }
    if (preferredEdge === "up") {
        return y <= EDGE_REVEAL_THRESHOLD_PX && Math.abs(x - anchorX) <= EDGE_REVEAL_AXIS_WINDOW_PX
    }
    return y >= (height - EDGE_REVEAL_THRESHOLD_PX) && Math.abs(x - anchorX) <= EDGE_REVEAL_AXIS_WINDOW_PX
}

function isRevealSwipe(edge: SidebarEdge, dx: number, dy: number): boolean {
    const absX = Math.abs(dx)
    const absY = Math.abs(dy)

    if (edge === "left") {
        return dx >= SWIPE_REVEAL_DISTANCE_PX && absX >= absY
    }
    if (edge === "right") {
        return dx <= -SWIPE_REVEAL_DISTANCE_PX && absX >= absY
    }
    if (edge === "up") {
        return dy >= SWIPE_REVEAL_DISTANCE_PX && absY >= absX
    }
    return dy <= -SWIPE_REVEAL_DISTANCE_PX && absY >= absX
}

function isFloatingMenuRevealSwipe(dx: number, dy: number): boolean {
    const absX = Math.abs(dx)
    const absY = Math.abs(dy)
    const horizontalReveal = dx <= -SWIPE_REVEAL_DISTANCE_PX && absX >= (absY * 0.6)
    const upwardReveal = dy <= -SWIPE_REVEAL_DISTANCE_PX && absY >= (absX * 0.6)
    return horizontalReveal || upwardReveal
}

function onDocumentPointerMove(event: PointerEvent) {
    if (!sidebarRoot || isSidebarSuppressed() || isUiOverlayLocked() || !shouldUseLegacyBrowserHandles()) {
        return
    }

    const now = Date.now()
    if (now - lastProbeAt < 80) {
        return
    }
    lastProbeAt = now

    if (isNearEdgeAxisWindow(event.clientX, event.clientY)) {
        revealSidebar()
    }
}

function onDocumentTouchStart(event: TouchEvent) {
    if (!event.touches.length) {
        return
    }

    const touch = event.touches[0]
    const legacyHandlesEnabled = shouldUseLegacyBrowserHandles()
    if (legacyHandlesEnabled && isAndroidTouchDevice() && isNearEdgeAxisWindow(touch.clientX, touch.clientY)) {
        activeTouchReveal = {
            startX: touch.clientX,
            startY: touch.clientY,
            mode: isSidebarSuppressed() ? "suppressedSidebar" : "sidebar"
        }
        return
    }

    if (isAndroidTouchDevice() && isNearAndroidFloatingRevealZone(touch.clientX, touch.clientY)) {
        activeTouchReveal = {
            startX: touch.clientX,
            startY: touch.clientY,
            mode: "parentFloating"
        }
        return
    }

    if (isSidebarSuppressed()) {
        if (legacyHandlesEnabled && isAndroidTouchDevice() && isNearEdgeAxisWindow(touch.clientX, touch.clientY)) {
            activeTouchReveal = {
                startX: touch.clientX,
                startY: touch.clientY,
                mode: "suppressedSidebar"
            }
            return
        }

        activeTouchReveal = null
        return
    }

    if (!legacyHandlesEnabled || !isNearEdgeAxisWindow(touch.clientX, touch.clientY)) {
        activeTouchReveal = null
        return
    }

    activeTouchReveal = {
        startX: touch.clientX,
        startY: touch.clientY,
        mode: "sidebar"
    }
}

function onDocumentTouchMove(event: TouchEvent) {
    if (!activeTouchReveal || !event.touches.length) {
        return
    }

    const touch = event.touches[0]
    const dx = touch.clientX - activeTouchReveal.startX
    const dy = touch.clientY - activeTouchReveal.startY

    if (activeTouchReveal.mode === "parentFloating") {
        if (isFloatingMenuRevealSwipe(dx, dy)) {
            activeTouchReveal = null
            notifyParentRevealFloatingMenu()
        }
        return
    }

    if (activeTouchReveal.mode === "suppressedSidebar") {
        if (isRevealSwipe(preferredEdge, dx, dy)) {
            const intercepted = !window.dispatchEvent(new CustomEvent(STREAM_EDGE_REVEAL_EVENT, {
                cancelable: true,
                detail: {
                    edge: preferredEdge,
                    dx,
                    dy,
                    suppressed: true
                }
            }))
            if (intercepted) {
                activeTouchReveal = null
                return
            }
            activeTouchReveal = null
            revealSuppressedSidebar()
        }
        return
    }

    if (isSidebarSuppressed()) {
        activeTouchReveal = null
        return
    }

    if (!isRevealSwipe(preferredEdge, dx, dy)) {
        return
    }

    const intercepted = !window.dispatchEvent(new CustomEvent(STREAM_EDGE_REVEAL_EVENT, {
        cancelable: true,
        detail: {
            edge: preferredEdge,
            dx,
            dy,
            suppressed: false
        }
    }))
    if (intercepted) {
        activeTouchReveal = null
        return
    }

    activeTouchReveal = null
    revealSidebar()
}

function onDocumentTouchEnd() {
    activeTouchReveal = null
}

function onUiOverlayStateEvent(event: Event) {
    const detail = event instanceof CustomEvent ? event.detail as { active?: boolean } | null : null
    uiOverlayActive = !!(detail && detail.active)

    if (!sidebarRoot || isSidebarSuppressed()) {
        return
    }

    if (uiOverlayActive) {
        clearAutoHideTimer()
        setSidebarAutoHidden(false)
        clearSuppressedRevealTimer()
        return
    }

    scheduleSidebarAutoHide()
    if (sidebarSuppressed) {
        scheduleSuppressedSidebarHide()
    }
}

function onSidebarButtonClick() {
    if (!shouldUseLegacyBrowserHandles()) {
        return
    }

    pinSidebarByInteraction()
    toggleSidebar()
}

function onSidebarButtonTouchStart(event: TouchEvent) {
    if (!shouldUseLegacyBrowserHandles()) {
        sidebarHandleTouchReveal = null
        return
    }

    bumpSidebarAutoHideFromActivity()
    if (!event.touches.length) {
        sidebarHandleTouchReveal = null
        return
    }

    const touch = event.touches[0]
    sidebarHandleTouchReveal = {
        startX: touch.clientX,
        startY: touch.clientY
    }
}

function onSidebarButtonTouchMove(event: TouchEvent) {
    if (!shouldUseLegacyBrowserHandles()) {
        sidebarHandleTouchReveal = null
        return
    }

    bumpSidebarAutoHideFromActivity()
    if (!sidebarHandleTouchReveal || !event.touches.length) {
        return
    }

    const touch = event.touches[0]
    const dx = touch.clientX - sidebarHandleTouchReveal.startX
    const dy = touch.clientY - sidebarHandleTouchReveal.startY

    if (!isRevealSwipe(preferredEdge, dx, dy)) {
        return
    }

    sidebarHandleTouchReveal = null
    pinSidebarByInteraction()
    revealSidebar()
}

function onSidebarButtonTouchEnd() {
    sidebarHandleTouchReveal = null
    if (shouldUseLegacyBrowserHandles()) {
        bumpSidebarAutoHideFromActivity()
    }
}

function onSidebarRootInteraction(event: Event) {
    if (!sidebarRoot || isSidebarSuppressed()) {
        return
    }
    if (!isEventInsideSidebar(event.target)) {
        return
    }

    pinSidebarByInteraction()
}

function onSidebarRootActivity() {
    bumpSidebarAutoHideFromActivity()
}

function onSidebarRootTouchStart(event: TouchEvent) {
    bumpSidebarAutoHideFromActivity()
    if (!event.changedTouches.length) {
        sidebarContentTouchGesture = null
        return
    }

    const touch = event.changedTouches[0]
    if (!isSidebarScrollableInteractiveTarget(event.target)) {
        sidebarContentTouchGesture = null
        return
    }

    sidebarContentTouchGesture = {
        identifier: touch.identifier,
        startX: touch.clientX,
        startY: touch.clientY,
        moved: false
    }
}

function onSidebarRootTouchMove(event: TouchEvent) {
    bumpSidebarAutoHideFromActivity()
    if (!sidebarContentTouchGesture) {
        return
    }

    const touch = Array.from(event.changedTouches).find((item) => item.identifier == sidebarContentTouchGesture?.identifier)
    if (!touch) {
        return
    }

    const dx = touch.clientX - sidebarContentTouchGesture.startX
    const dy = touch.clientY - sidebarContentTouchGesture.startY
    if (sidebarContentTouchGesture.moved || Math.abs(dy) >= 8 || Math.hypot(dx, dy) >= 10) {
        sidebarContentTouchGesture.moved = true
        sidebarContentSuppressClickUntil = Date.now() + 420
    }
}

function clearSidebarRootTouchGesture(event?: TouchEvent) {
    bumpSidebarAutoHideFromActivity()
    if (!sidebarContentTouchGesture) {
        return
    }

    if (event) {
        const touched = Array.from(event.changedTouches).some((item) => item.identifier == sidebarContentTouchGesture?.identifier)
        if (!touched) {
            return
        }
    }

    if (sidebarContentTouchGesture.moved) {
        sidebarContentSuppressClickUntil = Date.now() + 420
    }
    sidebarContentTouchGesture = null
}

function onSidebarRootClickCapture(event: MouseEvent) {
    bumpSidebarAutoHideFromActivity()
    if (Date.now() > sidebarContentSuppressClickUntil) {
        return
    }

    if (!isSidebarScrollableInteractiveTarget(event.target)) {
        return
    }

    event.preventDefault()
    event.stopPropagation()
}

function onDocumentOutsideInteractionStart(event: Event) {
    if (!sidebarRoot || isSidebarSuppressed() || !sidebarPinnedByInteraction) {
        return
    }

    const cleanFullscreen = document.documentElement.classList.contains("stream-clean-fullscreen")
        || !!document.body?.classList.contains("stream-clean-fullscreen")
    if (isDocumentFullscreen() && sidebarExtended && !cleanFullscreen) {
        return
    }

    if (String(event.type || "").startsWith("touch")) {
        return
    }

    if (event instanceof PointerEvent && event.pointerType === "touch") {
        return
    }

    if (isEventInsideSidebar(event.target)) {
        return
    }

    hideSidebarFromOutsideInteraction()
}

function registerSidebarInteractions() {
    if (!sidebarRoot) {
        return
    }

    document.addEventListener("pointerdown", onDocumentOutsideInteractionStart, { passive: true, capture: true })
    document.addEventListener("pointermove", onDocumentPointerMove, { passive: true })
    document.addEventListener("touchstart", onDocumentTouchStart, { passive: true, capture: true })
    document.addEventListener("touchmove", onDocumentTouchMove, { passive: true, capture: true })
    document.addEventListener("touchend", onDocumentTouchEnd, { passive: true, capture: true })
    document.addEventListener("touchcancel", onDocumentTouchEnd, { passive: true, capture: true })
    window.addEventListener("pccloud:vc-edit-mode", onUiOverlayStateEvent as EventListener)
    window.addEventListener("pccloud:vc-modal-state", onUiOverlayStateEvent as EventListener)
    window.addEventListener("resize", applySidebarPlacement)
    sidebarRoot.addEventListener("pointerdown", onSidebarRootInteraction, { passive: true, capture: true })
    sidebarRoot.addEventListener("pointermove", onSidebarRootActivity, { passive: true, capture: true })
    sidebarRoot.addEventListener("touchstart", onSidebarRootInteraction, { passive: true, capture: true })
    sidebarRoot.addEventListener("touchstart", onSidebarRootTouchStart, { passive: true, capture: true })
    sidebarRoot.addEventListener("touchmove", onSidebarRootTouchMove, { passive: true, capture: true })
    sidebarRoot.addEventListener("touchend", clearSidebarRootTouchGesture, { passive: true, capture: true })
    sidebarRoot.addEventListener("touchcancel", clearSidebarRootTouchGesture, { passive: true, capture: true })
    sidebarRoot.addEventListener("click", onSidebarRootClickCapture, { passive: false, capture: true })
    sidebarRoot.addEventListener("wheel", onSidebarRootActivity, { passive: true, capture: true })
    sidebarParent?.addEventListener("scroll", onSidebarRootActivity, { passive: true })
    sidebarRoot.addEventListener("pointerenter", () => revealSidebar(), { passive: true })
    sidebarButton?.addEventListener("click", onSidebarButtonClick)
    sidebarButton?.addEventListener("pointerdown", onSidebarRootActivity, { passive: true })
    sidebarButton?.addEventListener("pointermove", onSidebarRootActivity, { passive: true })
    sidebarButton?.addEventListener("touchstart", onSidebarButtonTouchStart, { passive: true })
    sidebarButton?.addEventListener("touchmove", onSidebarButtonTouchMove, { passive: true })
    sidebarButton?.addEventListener("touchend", onSidebarButtonTouchEnd, { passive: true })
    sidebarButton?.addEventListener("touchcancel", onSidebarButtonTouchEnd, { passive: true })
}

export function setSidebarStyle(style: SidebarStyle) {
    const edge = String(style.edge ?? DEFAULT_EDGE).toLowerCase()
    preferredEdge = (edge === "left" || edge === "right" || edge === "up" || edge === "down") ? edge : DEFAULT_EDGE
    applySidebarPlacement()
    applyBrowserNativeBridgeModeClasses()
}

export function setSidebarSuppressed(suppressed: boolean) {
    sidebarSuppressed = !!suppressed
    applyBrowserNativeBridgeModeClasses()
    if (!sidebarRoot) {
        return
    }

    clearSuppressedRevealTimer()
    suppressedRevealUntil = 0

    if (sidebarSuppressed) {
        activeTouchReveal = null
        unpinSidebarByInteraction()
        clearAutoHideTimer()
        hideSuppressedSidebarNow()
        return
    }

    sidebarRoot.classList.remove("sidebar-force-hidden")
    syncSidebarChromeVisibilityClasses()
    unpinSidebarByInteraction()
    if (!sidebarComponent || sidebarRoot.style.visibility === "hidden") {
        return
    }

    revealSidebar()
}

export function toggleSidebar() {
    setSidebarExtended(!isSidebarExtended())
}

export function setSidebarExtended(extended: boolean) {
    if (sidebarSuppressed && extended && !isSuppressedRevealActive()) {
        if (!revealSuppressedSidebar()) {
            return
        }
    }

    if (extended === sidebarExtended) {
        return
    }

    if (extended) {
        sidebarRoot?.classList.add("sidebar-show")
        setSidebarAutoHidden(false)
        clearAutoHideTimer()
        if (sidebarSuppressed) {
            clearSuppressedRevealTimer()
            suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
        }
    } else {
        unpinSidebarByInteraction()
        sidebarRoot?.classList.remove("sidebar-show")
        revealSidebar()
        if (sidebarSuppressed) {
            scheduleSuppressedSidebarHide()
        }
    }
    sidebarExtended = extended
    syncSidebarOpenClass()
    syncSidebarChromeVisibilityClasses()
    emitSidebarState()
}

export function isSidebarExtended(): boolean {
    return sidebarExtended
}

export function revealSidebarFromAction() {
    if (!sidebarRoot || !sidebarComponent || sidebarRoot.style.visibility === "hidden") {
        return
    }

    if (sidebarSuppressed) {
        revealSuppressedSidebar()
        clearSuppressedRevealTimer()
        suppressedRevealUntil = Date.now() + SUPPRESSED_REVEAL_DURATION_MS
    }

    sidebarRoot.classList.remove("sidebar-force-hidden")
    setSidebarAutoHidden(false)
    clearAutoHideTimer()
    sidebarRoot.classList.add("sidebar-show")
    sidebarExtended = true
    syncSidebarOpenClass()
    syncSidebarChromeVisibilityClasses()
    emitSidebarState()
}

export function setSidebar(sidebar: Sidebar | null) {
    if (sidebarParent == null || sidebarRoot == null) {
        showErrorPopup("failed to get sidebar")
        return
    }

    if (sidebarComponent) {
        sidebarComponent.unmount(sidebarParent)
        sidebarComponent = null
        sidebarRoot.style.visibility = "hidden"
        unpinSidebarByInteraction()
        clearAutoHideTimer()
        setSidebarAutoHidden(false)
        sidebarExtended = false
        syncSidebarOpenClass()
        syncSidebarChromeVisibilityClasses()
    }

    if (sidebar) {
        sidebarComponent = sidebar
        sidebar.mount(sidebarParent)
        sidebarRoot.style.visibility = "visible"
        applySidebarPlacement()
        applyBrowserNativeBridgeModeClasses()
        if (sidebarSuppressed) {
            sidebarRoot.classList.add("sidebar-force-hidden")
            syncSidebarChromeVisibilityClasses()
        } else {
            revealSidebar()
        }
        syncSidebarOpenClass()
        syncSidebarChromeVisibilityClasses()
    }
}

export function getSidebarRoot(): HTMLElement | null {
    return sidebarRoot
}

registerSidebarInteractions()
applySidebarPlacement()
applyBrowserNativeBridgeModeClasses()
setSidebar(null)
syncSidebarChromeVisibilityClasses()
