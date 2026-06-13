import "./polyfill/index.js"
import { Api, getApi, apiPostHost, FetchError, apiGetHostOperationsStatus, apiLogout, apiGetUser, tryLogin, apiGetHost, apiGetApps } from "./api.js";
import { AddHostModal } from "./component/host/add_modal.js";
import { HostList } from "./component/host/list.js";
import { Component, ComponentEvent } from "./component/index.js";
import { showErrorPopup } from "./component/error.js";
import { showModal } from "./component/modal/index.js";
import { setContextMenu } from "./component/context_menu.js";
import { GameList } from "./component/game/list.js";
import { Host } from "./component/host/index.js";
import { App, DetailedHost, DetailedUser, UndetailedHost } from "./api_bindings.js";
import type { HostOperationsStatus } from "./api_bindings.js";
import { getLocalStreamSettings, setLocalStreamSettings, StreamSettingsComponent } from "./component/settings_menu.js";
import { setTouchContextMenuEnabled } from "./polyfill/ios_right_click.js";
import { buildUrl } from "./config_.js";
import { setStyle as setPageStyle } from "./styles/index.js";
import { launchGameStream, type StreamLaunchPreference } from "./component/game/index.js";

function readLaunchPreferenceFromQuery(): StreamLaunchPreference {
    const launch = new URLSearchParams(window.location.search).get("launch")?.trim().toLowerCase()
    if (launch == "app" || launch == "web" || launch == "choose") {
        return launch
    }
    return null
}

function clearLaunchPreferenceFromQuery() {
    const url = new URL(window.location.href)
    url.searchParams.delete("launch")
    history.replaceState(history.state, "", url.toString())
}

async function startApp() {
    setTouchContextMenuEnabled(true)

    const api = await getApi()

    const rootElement = document.getElementById("root");
    if (rootElement == null) {
        showErrorPopup("couldn't find root element", true)
        return;
    }

    let lastAppState: AppState | null = null
    if (sessionStorage) {
        const lastStateText = sessionStorage.getItem("mlState")
        if (lastStateText) {
            lastAppState = JSON.parse(lastStateText)
        }
    }

    const app = new MainApp(api)
    app.mount(rootElement)

    window.addEventListener("popstate", event => {
        app.setAppState(event.state, false)
    })

    app.forceFetch()

    if (lastAppState) {
        app.setAppState(lastAppState)
    }
}

startApp()

type DisplayStates = "hosts" | "games" | "settings"

type AppState = { display: DisplayStates, hostId?: number }
function setAppState(state: AppState, pushHistory: boolean) {
    if (pushHistory) {
        history.pushState(state, "")
    }

    if (sessionStorage) {
        sessionStorage.setItem("mlState", JSON.stringify(state))
    }
}
function backAppState() {
    history.back()
}

class MainApp implements Component {
    private api: Api
    private user: DetailedUser | null = null
    private pendingLaunchPreference: StreamLaunchPreference = readLaunchPreferenceFromQuery()
    private launchIntentHandled = false

    private divElement = document.createElement("div")

    // Top Line
    private topLine = document.createElement("div")

    private moonlightTextElement = document.createElement("h1")

    private topLineActions = document.createElement("div")
    private logoutButton = document.createElement("button")
    // This is for the default user
    private loginButton = document.createElement("button")
    private adminButton = document.createElement("button")

    // Actions
    private actionElement = document.createElement("div")
    private hostOperationsBanner = document.createElement("div")
    private hostOperationsBadge = document.createElement("span")
    private hostOperationsSummary = document.createElement("span")

    private backButton: HTMLButtonElement = document.createElement("button")

    private hostAddButton: HTMLButtonElement = document.createElement("button")
    private settingsButton: HTMLButtonElement = document.createElement("button")

    // Different submenus
    private currentDisplay: DisplayStates | null = null

    private hostList: HostList
    private gameList: GameList | null = null
    private settings: StreamSettingsComponent
    private hostOperationsStatus: HostOperationsStatus | null = null

    constructor(api: Api) {
        this.api = api

        // Top Line
        this.topLine.classList.add("top-line")

        this.moonlightTextElement.innerHTML = "Cloudgime Stream"
        this.topLine.appendChild(this.moonlightTextElement)

        this.topLine.appendChild(this.topLineActions)
        this.topLineActions.classList.add("top-line-actions")

        this.logoutButton.addEventListener("click", async () => {
            await apiLogout(this.api)
            window.location.reload()
        })
        this.logoutButton.classList.add("logout-button")

        this.loginButton.addEventListener("click", async () => {
            const success = await tryLogin()
            if (success) {
                window.location.reload()
            }
        })
        this.loginButton.classList.add("login-button")

        this.adminButton.addEventListener("click", async () => {
            window.location.href = buildUrl("/admin.html")
        })
        this.adminButton.classList.add("admin-button")

        // Actions
        this.actionElement.classList.add("actions-list")

        this.hostOperationsBanner.style.display = "none"
        this.hostOperationsBanner.style.margin = "0.75rem 0 1rem"
        this.hostOperationsBanner.style.padding = "0.75rem 1rem"
        this.hostOperationsBanner.style.borderRadius = "0.75rem"
        this.hostOperationsBanner.style.border = "1px solid rgba(255,255,255,0.15)"
        this.hostOperationsBanner.style.background = "rgba(0,0,0,0.28)"
        this.hostOperationsBanner.style.display = "none"
        this.hostOperationsBanner.style.alignItems = "center"
        this.hostOperationsBanner.style.gap = "0.85rem"
        this.hostOperationsBanner.style.fontSize = "0.95rem"
        this.hostOperationsBanner.style.lineHeight = "1.4"
        this.hostOperationsBadge.style.display = "inline-flex"
        this.hostOperationsBadge.style.alignItems = "center"
        this.hostOperationsBadge.style.justifyContent = "center"
        this.hostOperationsBadge.style.minWidth = "7.5rem"
        this.hostOperationsBadge.style.padding = "0.42rem 0.7rem"
        this.hostOperationsBadge.style.borderRadius = "999px"
        this.hostOperationsBadge.style.fontSize = "0.78rem"
        this.hostOperationsBadge.style.fontWeight = "700"
        this.hostOperationsBadge.style.letterSpacing = "0.08em"
        this.hostOperationsBadge.style.textTransform = "uppercase"
        this.hostOperationsSummary.style.flex = "1"
        this.hostOperationsSummary.style.minWidth = "0"
        this.hostOperationsSummary.style.color = "rgba(255,255,255,0.92)"
        this.hostOperationsBanner.appendChild(this.hostOperationsBadge)
        this.hostOperationsBanner.appendChild(this.hostOperationsSummary)

        // Back button
        this.backButton.innerText = "Back"
        this.backButton.classList.add("button-fit-content")
        this.backButton.addEventListener("click", backAppState)

        // Host add button
        this.hostAddButton.classList.add("host-add")
        this.hostAddButton.addEventListener("click", this.addHost.bind(this))

        // Host list
        this.hostList = new HostList(api)
        this.hostList.addHostOpenListener(this.onHostOpen.bind(this))

        // Settings Button
        this.settingsButton.classList.add("open-settings")
        this.settingsButton.addEventListener("click", () => this.setCurrentDisplay("settings"))

        // Settings
        this.settings = new StreamSettingsComponent(getLocalStreamSettings() ?? undefined)
        this.settings.addChangeListener(this.onSettingsChange.bind(this))

        // Append default elements
        this.divElement.appendChild(this.topLine)
        this.divElement.appendChild(this.actionElement)
        this.divElement.appendChild(this.hostOperationsBanner)

        this.setCurrentDisplay("hosts")

        // Context Menu
        document.body.addEventListener("contextmenu", this.onContextMenu.bind(this), { passive: false })
    }

    setAppState(state: AppState, pushIntoHistory?: boolean) {
        if (state.display == "hosts") {
            this.setCurrentDisplay("hosts", null, pushIntoHistory)
        } else if (state.display == "games" && state.hostId != null) {
            this.setCurrentDisplay("games", { hostId: state.hostId }, pushIntoHistory)
        } else if (state.display == "settings") {
            this.setCurrentDisplay("settings", null, pushIntoHistory)
        }
    }

    private async addHost() {
        const modal = new AddHostModal()

        let host = await showModal(modal);

        if (host) {
            let newHost
            try {
                newHost = await apiPostHost(this.api, host)
            } catch (e) {
                if (e instanceof FetchError) {
                    const response = e.getResponse()
                    if (response && response.status == 404) {
                        showErrorPopup(`Host "${host.address}" is not reachable`)
                        return
                    }
                }
                throw e
            }

            this.hostList.insertList(newHost.host_id, newHost)
        }
    }

    private onContextMenu(event: MouseEvent) {
        if (this.currentDisplay == "hosts" || this.currentDisplay == "games") {
            const elements = [
                {
                    name: "Reload",
                    callback: this.forceFetch.bind(this)
                }
            ]

            setContextMenu(event, {
                elements
            })
        }
    }

    private async onHostOpen(event: ComponentEvent<Host>) {
        const hostId = event.component.getHostId()

        this.setCurrentDisplay("games", { hostId })
    }

    private onSettingsChange() {
        const newSettings = this.settings.getStreamSettings()

        // store settings in localStorage
        setLocalStreamSettings(newSettings)
        // apply style
        setPageStyle(newSettings.pageStyle)
    }

    private pickAutoLaunchApp(hostCache: DetailedHost | UndetailedHost | null, apps: App[], allowFirst: boolean): App | null {
        if (apps.length === 0) {
            return null
        }

        if (hostCache && "current_game" in hostCache && hostCache.current_game && hostCache.current_game != 0) {
            const current = apps.find(app => app.app_id == hostCache.current_game)
            if (current) {
                return current
            }
        }

        if (apps.length == 1) {
            return apps[0]
        }

        const desktopCandidates = apps.filter(app => /desktop|rdp|workspace|windows/i.test(app.title))
        if (desktopCandidates.length == 1) {
            return desktopCandidates[0]
        }

        if (allowFirst) {
            return apps[0]
        }

        return null
    }

    private async maybeHandleLaunchIntent() {
        if (this.launchIntentHandled || this.pendingLaunchPreference == null) {
            return
        }

        const hosts = this.hostList.getHosts()
        if (hosts.length != 1) {
            return
        }

        const hostComponent = hosts[0]
        const hostId = hostComponent.getHostId()
        const hostCache = hostComponent.getCache()
        const apps = await apiGetApps(this.api, {
            host_id: hostId,
        })
        const allowFirst = this.pendingLaunchPreference === "choose"
        const selectedApp = this.pickAutoLaunchApp(hostCache, apps, allowFirst)

        this.launchIntentHandled = true
        clearLaunchPreferenceFromQuery()

        this.setCurrentDisplay("games", {
            hostId,
            hostCache: apps,
        }, false)

        if (!selectedApp) {
            return
        }

        await launchGameStream(this.api, {
            hostId,
            appId: selectedApp.app_id,
            appName: selectedApp.title,
            isHdrSupported: selectedApp.is_hdr_supported,
            preferredLaunch: this.pendingLaunchPreference,
            forceChoice: this.pendingLaunchPreference === "choose",
        })
    }

    private setCurrentDisplay(display: "hosts",
        extraInfo?: null,
        pushIntoHistory?: boolean
    ): void
    private setCurrentDisplay(
        display: "games",
        extraInfo?: {
            hostId?: number | null,
            hostCache?: Array<App>
        },
        pushIntoHistory?: boolean
    ): void
    private setCurrentDisplay(display: "settings", extraInfo?: null, pushIntoHistory?: boolean): void

    private setCurrentDisplay(
        display: "hosts" | "games" | "settings",
        extraInfo?: {
            hostId?: number | null,
            hostCache?: Array<App>
        } | null,
        pushIntoHistory_?: boolean
    ) {
        const pushIntoHistory = pushIntoHistory_ === undefined ? true : pushIntoHistory_

        if (display == "games" && extraInfo?.hostId == null) {
            // invalid input state
            throw "invalid display state was requested"
        }

        // Check if we need to change
        if (this.currentDisplay == display) {
            if (this.currentDisplay == "games" && this.gameList?.getHostId() != extraInfo?.hostId) {
                // fall through
            } else {
                return
            }
        }

        // Unmount the current display
        if (this.currentDisplay == "hosts") {
            this.actionElement.removeChild(this.hostAddButton)
            this.actionElement.removeChild(this.settingsButton)

            this.hostList.unmount(this.divElement)
        } else if (this.currentDisplay == "games") {
            this.actionElement.removeChild(this.backButton)
            this.actionElement.removeChild(this.settingsButton)

            this.gameList?.unmount(this.divElement)
        } else if (this.currentDisplay == "settings") {
            this.actionElement.removeChild(this.backButton)

            this.settings.unmount(this.divElement)
        }

        // Mount the new display
        if (display == "hosts") {
            this.actionElement.appendChild(this.hostAddButton)
            this.actionElement.appendChild(this.settingsButton)

            this.hostList.mount(this.divElement)

            setAppState({ display: "hosts" }, pushIntoHistory)
        } else if (display == "games" && extraInfo?.hostId != null) {
            this.actionElement.appendChild(this.backButton)
            this.actionElement.appendChild(this.settingsButton)

            if (this.gameList?.getHostId() != extraInfo?.hostId) {
                this.gameList = new GameList(this.api, extraInfo?.hostId, extraInfo?.hostCache ?? null)
                this.gameList.addForceReloadListener(this.forceFetch.bind(this))
            }

            this.gameList.mount(this.divElement)

            this.refreshGameListActiveGame()

            setAppState({ display: "games", hostId: this.gameList?.getHostId() }, pushIntoHistory)
        } else if (display == "settings") {
            this.actionElement.appendChild(this.backButton)

            this.settings.mount(this.divElement)

            setAppState({ display: "settings" }, pushIntoHistory)
        }

        this.currentDisplay = display
    }

    async forceFetch() {
        const promiseUser = this.refreshUserRole()

        await Promise.all([
            this.hostList.forceFetch(),
            this.gameList?.forceFetch()
        ])

        if (this.currentDisplay == "games"
            && this.gameList
            && !this.hostList.getHost(this.gameList.getHostId())) {
            // The newly fetched list doesn't contain the hosts game view we're in -> go to hosts
            this.setCurrentDisplay("hosts")
        }

        await promiseUser

        await Promise.all([
            this.refreshGameListActiveGame(),
            this.refreshHostOperationsSummary()
        ])

        await this.maybeHandleLaunchIntent()
    }
    private async refreshUserRole() {
        this.user = await apiGetUser(this.api)

        if (this.topLineActions.contains(this.logoutButton)) {
            this.topLineActions.removeChild(this.logoutButton)
        }
        if (this.topLineActions.contains(this.loginButton)) {
            this.topLineActions.removeChild(this.loginButton)
        }
        if (this.topLineActions.contains(this.adminButton)) {
            this.topLineActions.removeChild(this.adminButton)
        }

        if (this.user.is_default_user) {
            this.topLineActions.appendChild(this.loginButton)
        } else {
            this.topLineActions.appendChild(this.logoutButton)
        }

        if (this.user.role == "Admin") {
            this.topLineActions.appendChild(this.adminButton)
        }
    }

    private setHostOperationsStatus(status: HostOperationsStatus | null) {
        this.hostOperationsStatus = status

        if (this.user?.role != "Admin" || status == null) {
            this.hostOperationsBanner.style.display = "none"
            this.hostOperationsSummary.innerText = ""
            this.hostOperationsBadge.innerText = ""
            return
        }

        const visuals = this.getHostHealthVisual(status.health_grade)

        this.hostOperationsBanner.style.display = "flex"
        this.hostOperationsBanner.style.borderColor = visuals.border
        this.hostOperationsBanner.style.background = visuals.background
        this.hostOperationsBadge.style.background = visuals.badgeBackground
        this.hostOperationsBadge.style.color = visuals.badgeText
        this.hostOperationsBadge.innerText = status.health_grade
        const parts: string[] = []
        parts.push(`Lifecycle ${status.lifecycle_phase}`)
        parts.push(`Runtime ${status.selected_runtime_display_name ?? status.selected_runtime_key}`)
        if (status.current_ready_streak_ms != null) {
            parts.push(`Ready ${this.formatDurationMs(status.current_ready_streak_ms)}`)
        }
        if (status.recommended_runtime_switch_required && status.recommended_runtime_key) {
            parts.push(`Suggested ${status.recommended_runtime_key}`)
        }
        if (status.last_incident_kind && status.last_incident_at_unix_ms != null) {
            parts.push(`Last ${status.last_incident_kind} ${this.formatDurationAgo(status.last_incident_at_unix_ms)} ago`)
        }
        if (status.selected_encoder != null) {
            parts.push(`Encoder ${status.selected_encoder}/${status.selected_capture ?? "unknown"}`)
        }
        if (status.release_info?.bundle_version) {
            parts.push(`Release ${status.release_info.bundle_version}`)
        } else if (status.release_info?.source_commit_short) {
            parts.push(`Release ${status.release_info.source_commit_short}`)
        }
        if (status.release_gate_status !== "passed") {
            parts.push(`Gate ${status.release_gate_status}`)
        }
        if (status.diagnostic_pack_status !== "passed") {
            parts.push(`Diag ${status.diagnostic_pack_status}`)
        }
        this.hostOperationsSummary.innerText = parts.join(" • ")
    }

    private formatDurationMs(value: bigint): string {
        const totalMs = Number(value)
        if (!Number.isFinite(totalMs) || totalMs < 0) {
            return value.toString()
        }

        return this.formatDurationMsNumber(totalMs)
    }

    private formatDurationMsNumber(totalMs: number): string {
        if (!Number.isFinite(totalMs) || totalMs < 0) {
            return String(totalMs)
        }

        const totalSeconds = Math.floor(totalMs / 1000)
        const hours = Math.floor(totalSeconds / 3600)
        const minutes = Math.floor((totalSeconds % 3600) / 60)
        const seconds = totalSeconds % 60

        if (hours > 0) {
            return `${hours}h ${minutes}m ${seconds}s`
        }
        if (minutes > 0) {
            return `${minutes}m ${seconds}s`
        }
        return `${seconds}s`
    }

    private formatDurationAgo(value: bigint): string {
        const nowMs = Date.now()
        const thenMs = Number(value)
        if (!Number.isFinite(thenMs) || thenMs > nowMs) {
            return value.toString()
        }

        return this.formatDurationMsNumber(nowMs - thenMs)
    }

    private getHostHealthVisual(healthGrade: string) {
        switch (healthGrade) {
            case "healthy":
                return {
                    border: "rgba(93, 214, 151, 0.42)",
                    background: "linear-gradient(135deg, rgba(16, 62, 36, 0.82), rgba(8, 28, 18, 0.72))",
                    badgeBackground: "rgba(93, 214, 151, 0.2)",
                    badgeText: "#bff5d5",
                }
            case "recovering":
                return {
                    border: "rgba(255, 194, 92, 0.42)",
                    background: "linear-gradient(135deg, rgba(74, 52, 10, 0.82), rgba(36, 24, 8, 0.72))",
                    badgeBackground: "rgba(255, 194, 92, 0.2)",
                    badgeText: "#ffe3a1",
                }
            case "failed":
                return {
                    border: "rgba(255, 112, 112, 0.46)",
                    background: "linear-gradient(135deg, rgba(78, 18, 18, 0.84), rgba(38, 10, 10, 0.76))",
                    badgeBackground: "rgba(255, 112, 112, 0.2)",
                    badgeText: "#ffd0d0",
                }
            default:
                return {
                    border: "rgba(120, 174, 255, 0.38)",
                    background: "linear-gradient(135deg, rgba(18, 38, 72, 0.82), rgba(10, 18, 36, 0.72))",
                    badgeBackground: "rgba(120, 174, 255, 0.2)",
                    badgeText: "#d4e3ff",
                }
        }
    }

    private async refreshHostOperationsSummary() {
        if (this.user?.role != "Admin") {
            this.setHostOperationsStatus(null)
            return
        }

        try {
            const status = await apiGetHostOperationsStatus(this.api)
            this.setHostOperationsStatus(status)
        } catch (error) {
            console.warn("failed to load host operations status", error)
            this.setHostOperationsStatus(null)
        }
    }
    private async refreshGameListActiveGame() {
        const gameList = this.gameList
        const hostId = gameList?.getHostId()
        if (hostId == null) {
            return
        }

        const host = this.hostList.getHost(hostId)

        let currentGame = null
        if (host != null) {
            currentGame = await host.getCurrentGame()
        } else {
            const host = await apiGetHost(this.api, { host_id: hostId })
            if (host.current_game != 0) {
                currentGame = host.current_game
            }
        }

        if (currentGame != null) {
            gameList?.setActiveGame(currentGame)
        } else {
            gameList?.setActiveGame(null)
        }
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.divElement)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.divElement)
    }
}
