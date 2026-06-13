import { Component, ComponentEvent } from "../index.js";
import { Api, FetchError, apiGetAppImage, apiHostCancel, fetchApi } from "../../api.js";
import { App, PostAndroidNativeLaunchTokenResponse } from "../../api_bindings.js";
import { setContextMenu } from "../context_menu.js";
import { Modal, showMessage, showModal } from "../modal/index.js";
import { APP_NO_IMAGE } from "../../resources/index.js";
import { buildUrl } from "../../config_.js";

export type GameCache = App & { activeApp: number | null }

export type GameEventListener = (event: ComponentEvent<Game>) => void

const MOBILE_PERFORMANCE_PROFILE_STORAGE_KEY = "mlMobilePerformanceProfileV1"
const ANDROID_NATIVE_HANDOFF_FLAG_KEY = "ML_ANDROID_NATIVE_HANDOFF"
const KEEPER_TUNNEL_PATH_MARKER = "/api/v1/keeper-tunnel/"
const ANDROID_NATIVE_PACKAGE = "id.cloudgime.moonlightnative.legacycursor"
const ANDROID_NATIVE_COMPONENT = `${ANDROID_NATIVE_PACKAGE}/com.limelight.cloudgime.NativeLaunchActivity`
type AndroidLaunchChoice = "app" | "web"
export type StreamLaunchPreference = AndroidLaunchChoice | "choose" | null
type SharedSessionRoleRequest = "viewer" | "helper" | "admin_assist" | "player2"
type SharedSessionInviteCapabilities = {
    display_authority?: boolean
    allow_primary_input?: boolean
    allow_gamepad_slot?: number | null
    can_end_session?: boolean
}
type SharedSessionInvitePayload = {
    invite_token: string
    shared_session_id: string
    owner_session_id: string
    host_id: number
    app_id: number
    role: SharedSessionRoleRequest | string
    issued_at_unix_ms: number
    expires_at_unix_ms: number
    share_url: string
    attach_available: boolean
    status_message: string
    capabilities: SharedSessionInviteCapabilities
}
type SharedSessionInviteResponse = {
    invite: SharedSessionInvitePayload
}

function detectActiveBuildQueryToken(): string | null {
    const extractVersion = (source: string): string | null => {
        const match = source.match(/(?:\d{8})?(q\d+)/i) ?? source.match(/[?&]v=([^&]+)/i)
        return match?.[1]?.toLowerCase() ?? null
    }

    const sources: string[] = []
    for (const script of Array.from(document.scripts)) {
        if (script.src) {
            sources.push(script.src)
        }
    }

    for (const source of sources) {
        const version = extractVersion(source)
        if (version) {
            const datedMatch = source.match(/(\d{8}q\d+)/i)
            return datedMatch?.[1]?.toLowerCase() ?? version
        }
    }

    return null
}

function shouldAutoAppendLowMemoryProfile(): boolean {
    const mobileLike = Number(navigator.maxTouchPoints || 0) > 0
        || window.matchMedia?.("(pointer: coarse)").matches
        || window.matchMedia?.("(max-width: 900px)").matches
    if (!mobileLike) {
        return false
    }

    try {
        return window.localStorage.getItem(MOBILE_PERFORMANCE_PROFILE_STORAGE_KEY) == null
    } catch {
        return true
    }
}

function isAndroidBrowserClient(): boolean {
    const touchPoints = Number(navigator.maxTouchPoints || 0)
    const userAgent = String(navigator.userAgent || "")
    return touchPoints > 0 && /Android/i.test(userAgent)
}

function shouldPreferAndroidNativeHandoff(): boolean {
    if (!isAndroidBrowserClient()) {
        return false
    }

    try {
        return window.localStorage.getItem(ANDROID_NATIVE_HANDOFF_FLAG_KEY) != "0"
    } catch {
        return true
    }
}

function buildAndroidNativeHandoffUrl(query: URLSearchParams): string {
    const handoffUrl = buildUrl("/open-native.html")
    const url = new URL(handoffUrl)
    for (const [key, value] of query.entries()) {
        url.searchParams.set(key, value)
    }
    return url.toString()
}

function buildAndroidNativeIntentUrl(query: URLSearchParams, fallbackUrl: string): string {
    const intentUrl = new URL("intent://stream")
    for (const [key, value] of query.entries()) {
        intentUrl.searchParams.set(key, value)
    }
    intentUrl.hash = [
        "Intent",
        "scheme=moonlightnative",
        `package=${ANDROID_NATIVE_PACKAGE}`,
        `component=${ANDROID_NATIVE_COMPONENT}`,
        "action=android.intent.action.VIEW",
        `S.browser_fallback_url=${encodeURIComponent(fallbackUrl)}`,
        "end",
    ].join(";")
    return intentUrl.toString()
}

function buildAbsoluteUrl(path: string): string {
    return new URL(path, window.location.href).toString()
}

function appendCurrentKeeperTunnelSession(rawUrl: string): string {
    const ktSession = readCurrentKeeperTunnelSession()
    if (!ktSession) {
        return rawUrl
    }

    try {
        const url = new URL(rawUrl, window.location.href)
        if (!url.searchParams.has("kt_session")) {
            url.searchParams.set("kt_session", ktSession)
        }
        return url.toString()
    } catch {
        return rawUrl
    }
}

function readCurrentKeeperTunnelSession(): string | null {
    try {
        const value = new URL(window.location.href).searchParams.get("kt_session")?.trim()
        return value ? value : null
    } catch {
        return null
    }
}

function isKeeperTunnelScopedPage(): boolean {
    try {
        const currentUrl = new URL(window.location.href)
        return currentUrl.pathname.includes(KEEPER_TUNNEL_PATH_MARKER)
            || readCurrentKeeperTunnelSession() != null
    } catch {
        return readCurrentKeeperTunnelSession() != null
    }
}

async function issueAndroidNativeLaunchToken(api: Api, hostId: number, appId: number): Promise<PostAndroidNativeLaunchTokenResponse> {
    const endpoint = isKeeperTunnelScopedPage()
        ? "/android-native/launch-token-from-tunnel"
        : "/android-native/launch-token"
    const ktSession = readCurrentKeeperTunnelSession()
    return await fetchApi(api, endpoint, "POST", {
        json: {
            host_id: hostId,
            app_id: appId,
            native_shell: "android",
            client_os: "android",
            client_platform: "android",
        },
        query: isKeeperTunnelScopedPage() && ktSession ? { kt_session: ktSession } : undefined,
    })
}

async function issueSharedSessionInvite(
    api: Api,
    hostId: number,
    appId: number,
    role: SharedSessionRoleRequest,
): Promise<SharedSessionInvitePayload> {
    const response = await fetchApi(api, "/android-native/shared-session/invite", "POST", {
        json: {
            host_id: hostId,
            app_id: appId,
            role,
        },
    }) as SharedSessionInviteResponse

    return response.invite
}

function sharedSessionRoleLabel(role: SharedSessionRoleRequest | string): string {
    switch (role) {
        case "player2":
            return "Player 2"
        case "helper":
            return "Helper"
        case "admin_assist":
            return "Admin Assist"
        case "viewer":
            return "Viewer"
        default:
            return String(role || "Shared")
    }
}

function describeSharedSessionCapabilities(capabilities: SharedSessionInviteCapabilities): string {
    const items: string[] = []
    items.push(capabilities.display_authority ? "Display authority ikut invite" : "Display tetap dikunci ke owner")
    if (capabilities.allow_gamepad_slot != null) {
        items.push(`Gamepad slot ${capabilities.allow_gamepad_slot + 1} disiapkan untuk joiner`)
    } else if (capabilities.allow_primary_input) {
        items.push("Input utama diizinkan")
    } else {
        items.push("Input utama tidak diizinkan")
    }
    if (capabilities.can_end_session) {
        items.push("Boleh mengakhiri sesi")
    }
    return items.join(" · ")
}

async function copyTextToClipboard(value: string): Promise<boolean> {
    const text = String(value || "")
    if (!text) {
        return false
    }

    if (navigator.clipboard?.writeText) {
        try {
            await navigator.clipboard.writeText(text)
            return true
        } catch {
            // fall through
        }
    }

    const textArea = document.createElement("textarea")
    textArea.value = text
    textArea.setAttribute("readonly", "true")
    textArea.style.position = "fixed"
    textArea.style.opacity = "0"
    document.body.appendChild(textArea)
    textArea.focus()
    textArea.select()
    let copied = false
    try {
        copied = document.execCommand("copy")
    } catch {
        copied = false
    } finally {
        document.body.removeChild(textArea)
    }
    return copied
}

class SharedSessionInviteModal implements Component, Modal<void> {
    private root = document.createElement("div")
    private title = document.createElement("h2")
    private description = document.createElement("p")
    private status = document.createElement("p")
    private actions = document.createElement("div")
    private resultCard = document.createElement("div")
    private resultRole = document.createElement("p")
    private resultStatus = document.createElement("p")
    private resultCapabilities = document.createElement("p")
    private resultLink = document.createElement("textarea")
    private resultMeta = document.createElement("p")
    private footer = document.createElement("div")
    private copyButton = document.createElement("button")
    private closeButton = document.createElement("button")
    private roleButtons: HTMLButtonElement[] = []

    constructor(
        private api: Api,
        private hostId: number,
        private appId: number,
        private appName: string,
    ) {
        this.root.style.display = "grid"
        this.root.style.gap = "0.85rem"
        this.root.style.maxWidth = "36rem"

        this.title.innerText = "Share Session"
        this.title.style.margin = "0"

        this.description.innerText = `Bagikan sesi aktif ${appName} tanpa memindahkan display authority dari owner.`
        this.description.style.margin = "0"
        this.description.style.lineHeight = "1.45"

        this.status.innerText = "Pilih role share yang ingin dibuat."
        this.status.style.margin = "0"
        this.status.style.color = "rgba(255,255,255,0.78)"

        this.actions.style.display = "grid"
        this.actions.style.gap = "0.65rem"
        this.actions.style.gridTemplateColumns = "repeat(auto-fit, minmax(150px, 1fr))"

        const roleOptions: Array<{ role: SharedSessionRoleRequest, label: string, note: string }> = [
            { role: "player2", label: "Share as Player 2", note: "Untuk co-op lokal, tetap tanpa display authority." },
            { role: "viewer", label: "Share as Viewer", note: "Hanya lihat sesi owner." },
            { role: "helper", label: "Share as Helper", note: "Bantuan terarah tanpa rebut display." },
            { role: "admin_assist", label: "Share as Admin Assist", note: "Jalur bantu admin tanpa memutus owner." },
        ]

        for (const option of roleOptions) {
            const button = document.createElement("button")
            button.type = "button"
            button.classList.add("button-fit-content")
            button.style.minHeight = "3.25rem"
            button.style.whiteSpace = "pre-line"
            button.innerText = `${option.label}\n${option.note}`
            button.addEventListener("click", () => {
                void this.createInvite(option.role)
            })
            this.actions.appendChild(button)
            this.roleButtons.push(button)
        }

        this.resultCard.style.display = "none"
        this.resultCard.style.padding = "0.9rem"
        this.resultCard.style.borderRadius = "0.9rem"
        this.resultCard.style.border = "1px solid rgba(255,255,255,0.12)"
        this.resultCard.style.background = "rgba(255,255,255,0.04)"

        for (const element of [this.resultRole, this.resultStatus, this.resultCapabilities, this.resultMeta]) {
            element.style.margin = "0"
            element.style.lineHeight = "1.45"
        }

        this.resultLink.readOnly = true
        this.resultLink.rows = 3
        this.resultLink.style.width = "100%"
        this.resultLink.style.boxSizing = "border-box"
        this.resultLink.style.marginTop = "0.65rem"

        this.resultCard.appendChild(this.resultRole)
        this.resultCard.appendChild(this.resultStatus)
        this.resultCard.appendChild(this.resultCapabilities)
        this.resultCard.appendChild(this.resultMeta)
        this.resultCard.appendChild(this.resultLink)

        this.footer.style.display = "flex"
        this.footer.style.flexWrap = "wrap"
        this.footer.style.gap = "0.75rem"

        this.copyButton.type = "button"
        this.copyButton.innerText = "Copy Link"
        this.copyButton.classList.add("button-fit-content")
        this.copyButton.style.display = "none"
        this.copyButton.addEventListener("click", () => {
            void this.copyInviteLink()
        })

        this.closeButton.type = "button"
        this.closeButton.innerText = "Close"
        this.closeButton.classList.add("button-fit-content")

        this.footer.appendChild(this.copyButton)
        this.footer.appendChild(this.closeButton)

        this.root.appendChild(this.title)
        this.root.appendChild(this.description)
        this.root.appendChild(this.status)
        this.root.appendChild(this.actions)
        this.root.appendChild(this.resultCard)
        this.root.appendChild(this.footer)
    }

    private setBusy(busy: boolean) {
        for (const button of this.roleButtons) {
            button.disabled = busy
        }
        this.copyButton.disabled = busy
        this.closeButton.disabled = busy
    }

    private async createInvite(role: SharedSessionRoleRequest) {
        this.setBusy(true)
        this.status.innerText = `Membuat invite ${sharedSessionRoleLabel(role)}...`
        try {
            const invite = await issueSharedSessionInvite(this.api, this.hostId, this.appId, role)
            const shareUrl = new URL(invite.share_url, window.location.href).toString()
            this.resultCard.style.display = "grid"
            this.resultCard.style.gap = "0.45rem"
            this.resultRole.innerText = `Role: ${sharedSessionRoleLabel(invite.role)}`
            this.resultStatus.innerText = `Status: ${invite.status_message}`
            this.resultCapabilities.innerText = `Capabilities: ${describeSharedSessionCapabilities(invite.capabilities || {})}`
            this.resultMeta.innerText = `Invite valid sampai ${new Date(invite.expires_at_unix_ms).toLocaleString()}`
            this.resultLink.value = shareUrl
            this.copyButton.style.display = "inline-flex"
            this.status.innerText = invite.attach_available
                ? "Invite siap dipakai."
                : "Invite sudah dibuat. Lane attach masih mengikuti status host build saat ini."
        } catch (error) {
            if (error instanceof FetchError) {
                const response = error.getResponse()
                if (response?.status === 409) {
                    this.status.innerText = "Belum ada sesi owner aktif yang bisa dishare untuk app ini."
                } else if (response?.status === 400) {
                    this.status.innerText = "Role share tidak valid untuk sesi ini."
                } else {
                    this.status.innerText = "Gagal membuat share invite sekarang."
                }
            } else {
                this.status.innerText = "Gagal membuat share invite sekarang."
            }
            console.error(error)
        } finally {
            this.setBusy(false)
        }
    }

    private async copyInviteLink() {
        const copied = await copyTextToClipboard(this.resultLink.value)
        this.status.innerText = copied
            ? "Link share sudah disalin."
            : "Gagal menyalin link share."
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.root)
    }

    unmount(parent: HTMLElement): void {
        parent.removeChild(this.root)
    }

    onFinish(signal: AbortSignal): Promise<void> {
        return new Promise(resolve => {
            this.closeButton.addEventListener("click", () => resolve(), {
                signal,
            })
            signal.addEventListener("abort", () => resolve(), { once: true })
        })
    }
}

class AndroidLaunchChoiceModal implements Component, Modal<AndroidLaunchChoice | null> {
    private root = document.createElement("div")
    private title = document.createElement("h2")
    private description = document.createElement("p")
    private actions = document.createElement("div")
    private openAppButton = document.createElement("button")
    private cancelButton = document.createElement("button")

    constructor(appTitle: string) {
        this.root.style.display = "grid"
        this.root.style.gap = "0.9rem"
        this.root.style.maxWidth = "32rem"

        this.title.innerText = "Open the native app"
        this.title.style.margin = "0"

        this.description.innerText = `Open ${appTitle} in the native Android app. Web stream is disabled for this flow.`
        this.description.style.margin = "0"
        this.description.style.lineHeight = "1.45"

        this.actions.style.display = "grid"
        this.actions.style.gap = "0.75rem"
        this.actions.style.gridTemplateColumns = "repeat(auto-fit, minmax(180px, 1fr))"

        this.openAppButton.type = "button"
        this.openAppButton.innerText = "Open App"

        this.cancelButton.type = "button"
        this.cancelButton.innerText = "Cancel"

        for (const button of [this.openAppButton, this.cancelButton]) {
            button.classList.add("button-fit-content")
            button.style.minHeight = "3rem"
        }

        this.actions.appendChild(this.openAppButton)

        this.root.appendChild(this.title)
        this.root.appendChild(this.description)
        this.root.appendChild(this.actions)
        this.root.appendChild(this.cancelButton)
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.root)
    }

    unmount(parent: HTMLElement): void {
        parent.removeChild(this.root)
    }

    onFinish(signal: AbortSignal): Promise<AndroidLaunchChoice | null> {
        const abortController = new AbortController()
        signal.addEventListener("abort", abortController.abort.bind(abortController), { once: true })

        return new Promise(resolve => {
            const finish = (choice: AndroidLaunchChoice | null) => {
                if (!abortController.signal.aborted) {
                    abortController.abort()
                }
                resolve(choice)
            }

            this.openAppButton.addEventListener("click", () => finish("app"), {
                signal: abortController.signal,
            })
            this.cancelButton.addEventListener("click", () => finish(null), {
                signal: abortController.signal,
            })
        })
    }
}

type LaunchGameStreamOptions = {
    hostId: number
    appId: number
    appName: string
    isHdrSupported: boolean
    preferredLaunch?: StreamLaunchPreference
    forceChoice?: boolean
}

export async function launchGameStream(api: Api, options: LaunchGameStreamOptions): Promise<boolean> {
    let query = new URLSearchParams({
        hostId: options.hostId,
        appId: options.appId,
        appName: options.appName,
        appHdr: options.isHdrSupported ? "1" : "0",
    } as any)

    const buildToken = detectActiveBuildQueryToken()
    if (buildToken) {
        query.set("cb", buildToken)
    }

    if (shouldAutoAppendLowMemoryProfile()) {
        query.set("deviceProfile", "low_memory")
    }

    const preferredLaunch = options.preferredLaunch ?? null
    const forceChoice = options.forceChoice ?? false
    const resolvedPreferred = preferredLaunch === "choose" ? null : preferredLaunch

    if (resolvedPreferred === "web") {
        await showMessage("Web stream sedang dinonaktifkan. Pakai Open App untuk flow native.")
        return false
    }

    if (resolvedPreferred === "app" && !isAndroidBrowserClient()) {
        await showMessage("Open App hanya tersedia di Android. Web stream tidak dibuka otomatis.")
        return false
    }

    if (resolvedPreferred === "app" || shouldPreferAndroidNativeHandoff() || forceChoice) {
        const choice = resolvedPreferred ?? await showModal(new AndroidLaunchChoiceModal(options.appName))
        if (choice == null) {
            return false
        }

        try {
            const tokenResponse = await issueAndroidNativeLaunchToken(api, options.hostId, options.appId)
            const nativeSchemeUrl = appendCurrentKeeperTunnelSession(tokenResponse.native_scheme_url)
            const nativeUrl = new URL(nativeSchemeUrl)
            const nativeQuery = new URLSearchParams(nativeUrl.search)

            // Fix: Override loopback/local webBaseUrl with current public keeper tunnel base URL
            const serverWebBaseUrl = nativeQuery.get("webBaseUrl")
            if (serverWebBaseUrl) {
                try {
                    const parsed = new URL(serverWebBaseUrl)
                    const host = parsed.hostname.toLowerCase()
                    const isLocal = host === "127.0.0.1"
                        || host === "localhost"
                        || host === "::1"
                        || host === "[::1]"
                        || host.startsWith("10.")
                        || host.startsWith("192.168.")
                        || host.startsWith("172.16.")
                        || host.startsWith("172.17.")
                        || host.startsWith("172.18.")
                        || host.startsWith("172.19.")
                        || host.startsWith("172.20.")
                        || host.startsWith("172.21.")
                        || host.startsWith("172.22.")
                        || host.startsWith("172.23.")
                        || host.startsWith("172.24.")
                        || host.startsWith("172.25.")
                        || host.startsWith("172.26.")
                        || host.startsWith("172.27.")
                        || host.startsWith("172.28.")
                        || host.startsWith("172.29.")
                        || host.startsWith("172.30.")
                        || host.startsWith("172.31.")
                    if (isLocal) {
                        const publicWebBase = new URL("./", window.location.href).toString()
                        nativeQuery.set("webBaseUrl", publicWebBase)
                    }
                } catch (e) {
                    console.error("Failed to parse webBaseUrl", e)
                }
            }

            nativeQuery.set("nativeShell", "android")
            nativeQuery.set("appName", options.appName)
            nativeQuery.set("appHdr", options.isHdrSupported ? "1" : "0")
            const fallbackUrl = buildAbsoluteUrl(appendCurrentKeeperTunnelSession(tokenResponse.open_native_path))
            const intentUrl = buildAndroidNativeIntentUrl(nativeQuery, fallbackUrl)
            window.location.href = intentUrl
            return true
        } catch (error) {
            console.error(error)
            await showMessage("Native app launch is unavailable right now.")
            return false
        }
    }

    await showMessage("Web stream sedang dinonaktifkan. Pakai Open App untuk flow native.")
    return false
}

export class Game implements Component {
    private api: Api

    private hostId: number
    private appId: number

    private mounted: number = 0
    private divElement: HTMLDivElement = document.createElement("div")

    private imageBlob: Blob | null = null
    private imageBlobUrl: string | null = null
    private imageElement: HTMLImageElement = document.createElement("img")
    private textElement: HTMLDivElement = document.createElement("div")
    private titleElement: HTMLParagraphElement = document.createElement("p")
    private subtitleElement: HTMLParagraphElement = document.createElement("p")

    private cache: GameCache

    constructor(api: Api, hostId: number, appId: number, cache: GameCache) {
        this.api = api

        this.hostId = hostId
        this.appId = appId

        this.cache = cache

        // Configure image
        this.imageElement.classList.add("app-image")
        this.imageElement.src = APP_NO_IMAGE

        this.forceLoadImage(false)

        // Configure div
        this.divElement.classList.add("app")

        this.divElement.appendChild(this.imageElement)
        this.textElement.classList.add("app-text")
        this.titleElement.classList.add("app-title")
        this.subtitleElement.classList.add("app-subtitle")
        this.textElement.appendChild(this.titleElement)
        this.textElement.appendChild(this.subtitleElement)
        this.divElement.appendChild(this.textElement)

        this.divElement.addEventListener("click", this.onClick.bind(this))
        this.divElement.addEventListener("contextmenu", this.onContextMenu.bind(this))

        this.updateCache(cache)
    }

    async forceLoadImage(forceServerRefresh: boolean) {
        this.imageBlob = await apiGetAppImage(this.api, {
            host_id: this.hostId,
            app_id: this.appId,
            force_refresh: forceServerRefresh
        })

        this.updateImage()
    }
    private updateImage() {
        // generate and set url
        if (this.imageBlob && !this.imageBlobUrl && this.mounted > 0) {
            this.imageBlobUrl = URL.createObjectURL(this.imageBlob)

            this.imageElement.classList.add("app-image-loaded")
            this.imageElement.src = this.imageBlobUrl
        }

        // revoke url
        if (this.imageBlobUrl && this.mounted <= 0) {
            URL.revokeObjectURL(this.imageBlobUrl)
            this.imageBlobUrl = null

            this.imageElement.classList.remove("app-image-loaded")
            this.imageElement.src = ""
        }
    }

    updateCache(cache: GameCache) {
        this.cache = cache

        this.divElement.classList.remove("app-inactive")
        this.divElement.classList.remove("app-active")

        if (this.isActive()) {
            this.divElement.classList.add("app-active")
        } else if (this.cache.activeApp != null) {
            this.divElement.classList.add("app-inactive")
        }

        this.titleElement.innerText = this.cache.title
        this.subtitleElement.innerText = this.isActive()
            ? "Active now"
            : this.cache.is_hdr_supported
                ? "HDR ready"
                : "Cloudgime stream"
    }

    private async onClick(event: MouseEvent) {
        if (this.cache.activeApp != null) {
            const elements = []

            if (this.isActive()) {
                elements.push({
                    name: shouldPreferAndroidNativeHandoff() ? "Resume App" : "Resume Session",
                    callback: async () => {
                        const launched = await this.startStream()
                        if (!launched) {
                            return
                        }

                        const event = new ComponentEvent("ml-gamereload", this)
                        this.divElement.dispatchEvent(event)
                    }
                })
            }

            elements.push({
                name: "Stop Current Session",
                callback: async () => {
                    const response = await apiHostCancel(this.api, { host_id: this.hostId })
                    if (!response.success) {
                        await showMessage("Failed to close app!")
                    }

                    const event = new ComponentEvent("ml-gamereload", this)
                    this.divElement.dispatchEvent(event)
                }
            })

            if (this.isActive()) {
                elements.push({
                    name: "Share Session",
                    callback: async () => {
                        await showModal(new SharedSessionInviteModal(
                            this.api,
                            this.hostId,
                            this.appId,
                            this.cache.title,
                        ))
                    }
                })
            }

            setContextMenu(event, {
                elements
            })
        } else {
            const launched = await this.startStream()
            if (!launched) {
                return
            }

            await new Promise(r => window.setTimeout(r, 6000))

            const event = new ComponentEvent("ml-gamereload", this)
            this.divElement.dispatchEvent(event)
        }
    }
    private async startStream(): Promise<boolean> {
        return await launchGameStream(this.api, {
            hostId: this.getHostId(),
            appId: this.getAppId(),
            appName: this.cache.title,
            isHdrSupported: this.cache.is_hdr_supported,
        })
    }

    private onContextMenu(event: MouseEvent) {
        const elements = []

        elements.push({
            name: "Show Details",
            callback: this.showDetails.bind(this),
        })

        elements.push({
            name: shouldPreferAndroidNativeHandoff() ? "Open App" : "Open",
            callback: async () => {
                const launched = await this.startStream()
                if (!launched) {
                    return
                }

                const event = new ComponentEvent("ml-gamereload", this)
                this.divElement.dispatchEvent(event)
            }
        })

        if (this.isActive()) {
            elements.push({
                name: "Share Session",
                callback: async () => {
                    await showModal(new SharedSessionInviteModal(
                        this.api,
                        this.hostId,
                        this.appId,
                        this.cache.title,
                    ))
                }
            })
        }

        setContextMenu(event, {
            elements
        })
    }

    private async showDetails() {
        const app = this.cache

        await showMessage(
            `Title: ${app.title}\n` +
            `Id: ${app.app_id}\n` +
            `HDR Supported: ${app.is_hdr_supported}\n`
        )
    }

    private isActive(): boolean {
        return this.cache.activeApp == this.appId
    }

    addForceReloadListener(listener: GameEventListener) {
        this.divElement.addEventListener("ml-gamereload", listener as any)
    }
    removeForceReloadListener(listener: GameEventListener) {
        this.divElement.removeEventListener("ml-gamereload", listener as any)
    }

    getHostId(): number {
        return this.hostId
    }
    getAppId(): number {
        return this.appId
    }

    mount(parent: HTMLElement): void {
        this.mounted++
        this.updateImage()

        parent.appendChild(this.divElement)
    }
    unmount(parent: HTMLElement): void {

        parent.removeChild(this.divElement)

        this.mounted--
        this.updateImage()
    }
}
