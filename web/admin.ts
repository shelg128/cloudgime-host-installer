import "./polyfill/index.js"
import "./styles/index.js"
import { Api, apiGetHostCapabilityProfile, apiGetHostOperationsStatus, apiGetUser, apiLogout, apiPostUser, apiRefreshHostCapabilityProfile, FetchError, getApi } from "./api.js";
import type { HostOperationsStatus } from "./api_bindings.js";
import { Component, ComponentEvent } from "./component/index.js";
import { showErrorPopup } from "./component/error.js";
import { setTouchContextMenuEnabled } from "./polyfill/ios_right_click.js";
import { UserList } from "./component/user/list.js";
import { AddUserModal } from "./component/user/add_modal.js";
import { showMessage, showModal } from "./component/modal/index.js";
import { buildUrl } from "./config_.js";
import { DetailedUserPage } from "./component/user/detailed_page.js";
import { User } from "./component/user/index.js";
import { DetailedUser, HostCapabilityProfile } from "./api_bindings.js";

async function startApp() {
    setTouchContextMenuEnabled(true)

    const api = await getApi()

    checkPermissions(api)

    const rootElement = document.getElementById("root")
    if (rootElement == null) {
        showErrorPopup("couldn't find root element", true)
        return;
    }

    const app = new AdminApp(api)
    app.mount(rootElement)

    app.forceFetch()
}

async function checkPermissions(api: Api) {
    const user = await apiGetUser(api)

    if (user.role != "Admin") {
        await showMessage("You are not authorized to view this page!")

        window.location.href = buildUrl("/")
    }
}

startApp()

class AdminApp implements Component {

    private api: Api

    private root = document.createElement("div")

    // Top Line
    private topLine = document.createElement("div")

    private moonlightTextElement = document.createElement("h1")

    private topLineActions = document.createElement("div")
    private logoutButton = document.createElement("button")
    private userButton = document.createElement("button")

    // Content
    private content = document.createElement("div")

    private hostCapabilityPanel = document.createElement("div")
    private hostCapabilityActions = document.createElement("div")
    private hostCapabilityTitle = document.createElement("h2")
    private hostCapabilityRefreshButton = document.createElement("button")
    private hostCapabilityCopyButton = document.createElement("button")
    private hostCapabilityStatus = document.createElement("div")
    private hostCapabilityDetails = document.createElement("pre")
    private hostCapabilityProfile: HostCapabilityProfile | null = null
    private hostOperationsStatus: HostOperationsStatus | null = null

    // User Panel
    private userPanel = document.createElement("div")
    private addUserButton = document.createElement("button")
    private userSearch = document.createElement("input")
    private userList: UserList

    // User Info
    private userInfoPage: DetailedUserPage | null = null

    constructor(api: Api) {
        this.api = api

        // Top Line
        this.topLine.classList.add("top-line")

        this.moonlightTextElement.innerHTML =
            'Cloudgime Host <span style="color:red; text-shadow: -1px -1px 0 #000, 1px -1px 0 #000, -1px 1px 0 #000, 1px 1px 0 #000; -webkit-text-stroke: 2px #000">Admin</span>'

        this.topLine.appendChild(this.moonlightTextElement)

        this.topLine.appendChild(this.topLineActions)
        this.topLineActions.classList.add("top-line-actions")

        this.logoutButton.addEventListener("click", async () => {
            await apiLogout(this.api)
            window.location.reload()
        })
        this.logoutButton.classList.add("logout-button")
        this.topLineActions.appendChild(this.logoutButton)

        this.userButton.addEventListener("click", async () => {
            window.location.href = buildUrl("/")
        })
        this.userButton.classList.add("user-button")
        this.topLineActions.appendChild(this.userButton)

        this.root.appendChild(this.topLine)

        // Content div
        this.content.classList.add("admin-panel-content")
        this.root.appendChild(this.content)

        this.hostCapabilityPanel.classList.add("user-panel")
        this.hostCapabilityTitle.innerText = "Host Runtime"
        this.hostCapabilityActions.style.display = "flex"
        this.hostCapabilityActions.style.gap = "0.5rem"
        this.hostCapabilityActions.style.marginBottom = "0.75rem"
        this.hostCapabilityRefreshButton.innerText = "Refresh Probe"
        this.hostCapabilityRefreshButton.addEventListener("click", async () => {
            await this.refreshHostCapability(true)
        })
        this.hostCapabilityCopyButton.innerText = "Copy JSON"
        this.hostCapabilityCopyButton.addEventListener("click", async () => {
            if (this.hostCapabilityProfile == null) {
                return
            }

            await navigator.clipboard.writeText(JSON.stringify(this.hostCapabilityProfile, null, 2))
        })
        this.hostCapabilityActions.appendChild(this.hostCapabilityRefreshButton)
        this.hostCapabilityActions.appendChild(this.hostCapabilityCopyButton)
        this.hostCapabilityStatus.innerText = "Loading host capability profile..."
        this.hostCapabilityDetails.style.whiteSpace = "pre-wrap"
        this.hostCapabilityDetails.style.margin = "0"
        this.hostCapabilityDetails.style.fontSize = "0.9rem"
        this.hostCapabilityDetails.style.lineHeight = "1.4"
        this.hostCapabilityPanel.appendChild(this.hostCapabilityTitle)
        this.hostCapabilityPanel.appendChild(this.hostCapabilityActions)
        this.hostCapabilityPanel.appendChild(this.hostCapabilityStatus)
        this.hostCapabilityPanel.appendChild(this.hostCapabilityDetails)
        this.content.appendChild(this.hostCapabilityPanel)

        // Select User Panel
        this.userPanel.classList.add("user-panel")
        this.content.appendChild(this.userPanel)

        this.addUserButton.innerText = "Add User"
        this.addUserButton.addEventListener("click", async () => {
            const addUserModal = new AddUserModal()

            const userRequest = await showModal(addUserModal)

            if (userRequest) {
                try {
                    const newUser = await apiPostUser(this.api, userRequest)

                    this.userList.insertList(newUser.id, newUser)
                } catch (e) {
                    // 409 = Conflict
                    if (e instanceof FetchError && e.getResponse()?.status == 409) {
                        // Name already exists
                        await showMessage(`A user with the name "${userRequest.name}" already exists!`)
                    } else {
                        throw e
                    }
                }
            }
        })
        this.userPanel.appendChild(this.addUserButton)

        this.userSearch.placeholder = "Search User"
        this.userSearch.type = "text"
        this.userSearch.addEventListener("input", this.onUserSearchChange.bind(this))
        this.userPanel.appendChild(this.userSearch)

        this.userList = new UserList(api)
        this.userList.addUserClickedListener(this.onUserClicked.bind(this))
        this.userList.addUserDeletedListener(this.onUserDeleted.bind(this))
        this.userList.mount(this.userPanel)
    }

    async forceFetch() {
        await Promise.all([
            this.userList.forceFetch(),
            this.refreshHostCapability(),
        ])
    }

    private async refreshHostCapability(forceRefresh = false) {
        try {
            this.hostCapabilityRefreshButton.disabled = true
            try {
                this.hostOperationsStatus = await apiGetHostOperationsStatus(this.api)
            } catch (opsError) {
                console.warn("failed to fetch host operations status", opsError)
            }

            try {
                const profile = forceRefresh
                    ? await apiRefreshHostCapabilityProfile(this.api)
                    : await apiGetHostCapabilityProfile(this.api)
                this.setHostCapabilityProfile(profile)
                return
            } catch (profileError) {
                if (profileError instanceof FetchError && profileError.getResponse()?.status == 404) {
                    this.hostCapabilityStatus.innerText = "Host capability profile not found yet"
                    this.hostCapabilityDetails.innerText = this.buildHostOperationsLines().join("\n")
                    return
                }

                throw profileError
            }
        } catch (e) {
            if (e instanceof FetchError && e.getResponse()?.status == 404) {
                this.hostCapabilityStatus.innerText = "Host capability profile not found yet"
                this.hostCapabilityDetails.innerText = this.buildHostOperationsLines().join("\n")
                return
            }

            throw e
        } finally {
            this.hostCapabilityRefreshButton.disabled = false
        }
    }

    private buildHostOperationsLines(): string[] {
        const ops = this.hostOperationsStatus
        if (ops == null) {
            return ["Host Operations: unavailable"]
        }

        const lines = [
            `Host Health: ${ops.health_grade}${ops.health_reason ? ` (${ops.health_reason})` : ""}`,
            `Release: ${ops.release_info?.deployment_environment ?? "unknown"} / ${ops.release_info?.release_channel ?? "unknown"}${ops.release_info?.bundle_version ? ` | version=${ops.release_info.bundle_version}` : ""}${ops.release_info?.build_id ? ` | build=${ops.release_info.build_id}` : ""}${ops.release_info?.source_commit_short ? ` | commit=${ops.release_info.source_commit_short}` : ""}${ops.release_info?.built_at_unix_ms != null ? ` | built=${this.formatUnixMs(ops.release_info.built_at_unix_ms)}` : ""}${ops.release_info?.source_dirty ? " | dirty=yes" : ""}`,
            `Release ID: ${ops.current_release_id ?? "unknown"}`,
            `Release Gate: ${ops.release_gate_status}${ops.release_gate_reason ? ` (${ops.release_gate_reason})` : ""}${ops.release_gate_summary?.gate_name ? ` | gate=${ops.release_gate_summary.gate_name}` : ""}${ops.release_gate_summary?.gate_profile ? ` | profile=${ops.release_gate_summary.gate_profile}` : ""}${ops.release_gate_summary?.gate_scenario ? ` | scenario=${ops.release_gate_summary.gate_scenario}` : ""}${ops.release_gate_summary?.checked_at_unix_ms != null ? ` | checked=${this.formatUnixMs(ops.release_gate_summary.checked_at_unix_ms)}` : ""}${ops.release_gate_summary?.support_bundle_id ? ` | support=${ops.release_gate_summary.support_bundle_id}` : ""}`,
            `Promotion: ${ops.promotion_stage}${ops.promotion_reason ? ` (${ops.promotion_reason})` : ""}${ops.promotion_target_environment ? ` | target=${ops.promotion_target_environment}` : ""}`,
            `Promotion Policy: ${ops.promotion_policy_name} | bundle=${ops.promotion_bundle_name} | group=${ops.promotion_group} | rings=${ops.promotion_ring_order.join(" -> ")}`,
            `Next Promotion: ${ops.next_promotion_readiness}${ops.next_promotion_reason ? ` (${ops.next_promotion_reason})` : ""}${ops.next_promotion_target_environment ? ` | target=${ops.next_promotion_target_environment}` : ""}${ops.next_promotion_required_ready_streak_ms != null ? ` | need-ready=${this.formatDurationMs(ops.next_promotion_required_ready_streak_ms)}` : ""}${ops.next_promotion_current_ready_streak_ms != null ? ` | current-ready=${this.formatDurationMs(ops.next_promotion_current_ready_streak_ms)}` : ""}`,
            `Upgrade Readiness: ${ops.migration_readiness}${ops.migration_reason ? ` (${ops.migration_reason})` : ""}`,
            `Upgrade Apply: ${ops.release_upgrade_state != null ? `${ops.release_upgrade_state.last_action}/${ops.release_upgrade_state.last_status}${ops.release_upgrade_state.snapshot_id ? ` | snapshot=${ops.release_upgrade_state.snapshot_id}` : ""}${ops.release_upgrade_state.completed_at_unix_ms != null ? ` | at=${this.formatUnixMs(ops.release_upgrade_state.completed_at_unix_ms)}` : ""}` : "never"}`,
            `Rollback: ${ops.rollback_ready ? "ready" : "not-ready"} | snapshots=${ops.release_snapshot_count}${ops.last_release_snapshot_id ? ` | last=${ops.last_release_snapshot_id}` : ""}${ops.last_release_snapshot_at_unix_ms != null ? ` | at=${this.formatUnixMs(ops.last_release_snapshot_at_unix_ms)}` : ""}`,
            `Config Hygiene: ${ops.config_hygiene_grade}${ops.config_hygiene_warnings.length > 0 ? ` (${ops.config_hygiene_warnings.join(", ")})` : ""}`,
            `Lifecycle: ${ops.lifecycle_phase}${ops.lifecycle_reason ? ` (${ops.lifecycle_reason})` : ""}`,
            `Service: ${ops.service_name} | User Agent: ${ops.user_agent_task_status}`,
            `Runtime Slot: ${ops.selected_runtime_key}${ops.selected_runtime_display_name ? ` (${ops.selected_runtime_display_name}${ops.selected_runtime_version ? ` ${ops.selected_runtime_version}` : ""})` : ""}`,
            `Runtime Recommendation: ${ops.recommended_runtime_key ?? "n/a"}${ops.recommended_runtime_display_name ? ` (${ops.recommended_runtime_display_name}${ops.recommended_runtime_version ? ` ${ops.recommended_runtime_version}` : ""})` : ""}${ops.recommended_runtime_switch_required ? " | switch=yes" : " | switch=no"}${ops.alternate_ready_runtime_count > 0 ? ` | alternates=${ops.alternate_ready_runtime_count}` : ""}${ops.recommended_runtime_reason ? ` | ${ops.recommended_runtime_reason}` : ""}`,
            `Runtime Adoption: ${ops.runtime_adoption_state != null ? `${ops.runtime_adoption_state.last_action}/${ops.runtime_adoption_state.last_status}${ops.runtime_adoption_state.reverted ? " | reverted=yes" : ""}${ops.runtime_adoption_state.completed_at_unix_ms != null ? ` | at=${this.formatUnixMs(ops.runtime_adoption_state.completed_at_unix_ms)}` : ""}${ops.runtime_adoption_state.support_bundle_id ? ` | support=${ops.runtime_adoption_state.support_bundle_id}` : ""}` : "never"}${ops.runtime_adoption_history_count > 0 ? ` | history=${ops.runtime_adoption_history_count}` : ""}`,
            `Universal Bundle: ${ops.universal_bundle_grade}${ops.universal_bundle_reason ? ` (${ops.universal_bundle_reason})` : ""}`,
            `Capability: probe=${ops.capability_probe_mode ?? "n/a"}${ops.capability_updated_at ? ` | updated=${ops.capability_updated_at}` : ""}${ops.selected_encoder ? ` | encoder=${ops.selected_encoder}` : ""}${ops.selected_capture ? ` | capture=${ops.selected_capture}` : ""}${ops.selected_ffmpeg_source ? ` | ffmpeg=${ops.selected_ffmpeg_source}` : ""}`,
            `Startup Checks: processes=${ops.required_processes_ready ? "ready" : "not-ready"} | local-http=${ops.local_http_ready ? "ready" : "not-ready"}${ops.selected_runtime_startup_validation_status ? ` | runtime=${ops.selected_runtime_startup_validation_status}${ops.selected_runtime_startup_validation_reason ? `/${ops.selected_runtime_startup_validation_reason}` : ""}` : ""}`,
            `Recovery Budget: ${ops.failure_recovery_attempt_count} active attempt(s)${ops.last_failure_recovery_strategy ? ` | last=${ops.last_failure_recovery_strategy}` : ""}${ops.last_failure_recovery_escalated ? " | escalated=yes" : ""}`,
            `Recovery History: total=${ops.total_failure_recovery_count} | escalated=${ops.total_failure_recovery_escalation_count} | watchdog=${ops.total_service_watchdog_trigger_count}`,
            `Since Boot: recoveries=${ops.boot_failure_recovery_count} | watchdog=${ops.boot_service_watchdog_trigger_count}${ops.daemon_started_at_unix_ms != null ? ` | daemon=${this.formatUnixMs(ops.daemon_started_at_unix_ms)}` : ""}`,
            `Release Gate History: count=${ops.release_gate_history_count}${ops.recent_release_gate_history.length > 0 ? ` | latest=${ops.recent_release_gate_history[0].gate_name}/${ops.recent_release_gate_history[0].gate_status}` : ""}`,
            `Diagnostic Pack: ${ops.diagnostic_pack_status}${ops.diagnostic_pack_reason ? ` (${ops.diagnostic_pack_reason})` : ""}${ops.diagnostic_pack_summary != null ? ` | latest=${ops.diagnostic_pack_summary.pack_name}/${ops.diagnostic_pack_summary.pack_status}` : ""}${ops.diagnostic_pack_summary?.gate_profile ? ` | profile=${ops.diagnostic_pack_summary.gate_profile}` : ""}${ops.diagnostic_pack_summary?.gate_scenario ? ` | scenario=${ops.diagnostic_pack_summary.gate_scenario}` : ""}${ops.diagnostic_pack_summary?.support_bundle_id ? ` | support=${ops.diagnostic_pack_summary.support_bundle_id}` : ""}${ops.diagnostic_pack_summary?.checked_at_unix_ms != null ? ` | checked=${this.formatUnixMs(ops.diagnostic_pack_summary.checked_at_unix_ms)}` : ""}`,
        ]

        if (ops.capability_selection_reason) {
            lines.push(`Capability Selection Reason: ${ops.capability_selection_reason}`)
        }

        if (ops.daemon_uptime_ms != null || ops.current_ready_streak_ms != null) {
            lines.push(
                `Runtime Trend: daemon-uptime=${ops.daemon_uptime_ms != null ? this.formatDurationMs(ops.daemon_uptime_ms) : "n/a"} | ready-streak=${ops.current_ready_streak_ms != null ? this.formatDurationMs(ops.current_ready_streak_ms) : "n/a"}`
            )
        }

        if (ops.last_failure_recovery_reason) {
            lines.push(`Last Recovery Reason: ${ops.last_failure_recovery_reason}`)
        }

        if (ops.last_service_watchdog_reason) {
            lines.push(`Last Watchdog Reason: ${ops.last_service_watchdog_reason}`)
        }

        if (ops.last_failure_recovery_completed_at_unix_ms != null) {
            lines.push(`Last Recovery Completed At: ${this.formatUnixMs(ops.last_failure_recovery_completed_at_unix_ms)}`)
        }

        if (ops.last_failure_recovery_budget_cleared_at_unix_ms != null) {
            lines.push(`Recovery Budget Cleared At: ${this.formatUnixMs(ops.last_failure_recovery_budget_cleared_at_unix_ms)}`)
        }

        if (ops.last_service_watchdog_at_unix_ms != null) {
            lines.push(`Last Watchdog At: ${this.formatUnixMs(ops.last_service_watchdog_at_unix_ms)}`)
        }

        if (ops.last_incident_kind && ops.last_incident_at_unix_ms != null) {
            lines.push(`Last Incident: ${ops.last_incident_kind} at ${this.formatUnixMs(ops.last_incident_at_unix_ms)}`)
        }

        if (ops.release_gate_summary != null) {
            lines.push(
                `Release Gate Metrics: duration=${this.formatDurationMsNumber(Number(ops.release_gate_summary.duration_ms))} | presented-fps=${ops.release_gate_summary.effective_presented_fps.toFixed(2)} | receiver-fps=${ops.release_gate_summary.avg_receiver_fps.toFixed(2)} | streamer-fps=${ops.release_gate_summary.avg_streamer_output_fps.toFixed(2)} | route-loss=${ops.release_gate_summary.route_lost_count} | reconnect=${ops.release_gate_summary.reconnect_count} | stall=${ops.release_gate_summary.stall_recoveries} | degrade=${ops.release_gate_summary.gameplay_degrade_count}`
            )
        }

        if (ops.recent_release_history.length > 0) {
            lines.push("Recent Releases:")
            for (const releaseEntry of ops.recent_release_history.slice(0, 5)) {
                lines.push(
                    `  ${releaseEntry.action}/${releaseEntry.status} | ${releaseEntry.release_id}${releaseEntry.snapshot_id ? ` | snapshot=${releaseEntry.snapshot_id}` : ""}${releaseEntry.completed_at_unix_ms != null ? ` | at=${this.formatUnixMs(releaseEntry.completed_at_unix_ms)}` : ""}`
                )
            }
        }

        if (ops.recent_incidents.length > 0) {
            lines.push("Recent Incidents:")
            for (const incident of ops.recent_incidents.slice(0, 5)) {
                const strategy = incident.strategy ? ` | strategy=${incident.strategy}` : ""
                const escalated = incident.escalated ? " | escalated=yes" : ""
                lines.push(`  ${this.formatUnixMs(incident.at_unix_ms)} | ${incident.kind} | ${incident.reason}${strategy}${escalated}`)
            }
        }

        lines.push(`Local URL: ${ops.local_url}`)

        return lines
    }

    private formatUnixMs(value: bigint): string {
        const unixMs = Number(value)
        if (!Number.isFinite(unixMs)) {
            return value.toString()
        }

        return `${new Date(unixMs).toLocaleString()} (${value.toString()})`
    }

    private formatDurationMs(value: bigint): string {
        const totalMs = Number(value)
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

    private setHostCapabilityProfile(profile: HostCapabilityProfile) {
        this.hostCapabilityProfile = profile
        const gpuSummary = profile.gpu_controllers
            .map((gpu) => `${gpu.name} (${gpu.driver_version})`)
            .join(", ")
        const runtimeSummary = profile.runtime_candidates
            .map((runtime) => {
                const label = runtime.display_name ?? runtime.key
                const version = runtime.runtime_version ? ` ${runtime.runtime_version}` : ""
                const ffmpegSource = runtime.ffmpeg_source ? ` ffmpeg=${runtime.ffmpeg_source}` : ""
                const bundledRequirement = runtime.requires_bundled_ffmpeg ? " [bundled-ffmpeg]" : ""
                const autoSelect = runtime.auto_select ? " auto=on" : " auto=off"
                const status = runtime.runtime_status ? ` status=${runtime.runtime_status}` : ""
                const healthyEncoders = runtime.healthy_encoders.length > 0
                    ? ` healthy=${runtime.healthy_encoders.join(",")}`
                    : ""
                const statusReason = runtime.runtime_status_reason ? ` reason=${runtime.runtime_status_reason}` : ""
                const startupValidation = runtime.startup_validation_status
                    ? ` startup=${runtime.startup_validation_status}${runtime.startup_validation_reason ? `/${runtime.startup_validation_reason}` : ""}`
                    : ""
                return `${label}${version}${runtime.legacy ? " [legacy]" : ""}${bundledRequirement}${autoSelect}${ffmpegSource}${status}${healthyEncoders}${startupValidation}${statusReason}`
            })
            .join(", ")
        const failedProbes = profile.encoder_probes
            .filter((probe) => !probe.ok)
            .map((probe) => `${probe.runtime_key}/${probe.encoder_key}: ${probe.detail}`)
            .slice(0, 3)

        const runtimeLabel = profile.selected_runtime_display_name ?? profile.selected_runtime_key
        const runtimeVersion = profile.selected_runtime_version ? ` ${profile.selected_runtime_version}` : ""
        this.hostCapabilityStatus.innerText =
            `${this.hostOperationsStatus ? `Host ${this.hostOperationsStatus.health_grade} | Bundle ${this.hostOperationsStatus.universal_bundle_grade} | ` : ""}Runtime ${runtimeLabel}${runtimeVersion} | Encoder ${profile.selected_encoder} | Capture ${profile.selected_capture} | Probe ${profile.probe_mode}`

        const lines = [
            ...this.buildHostOperationsLines(),
            `Updated: ${profile.updated_at}`,
            `Reason: ${profile.selection_reason}`,
            `GPUs: ${gpuSummary || "n/a"}`,
            `FFmpeg: ${profile.ffmpeg_path ?? "not found"}${profile.selected_ffmpeg_source ? ` (${profile.selected_ffmpeg_source})` : ""}`,
            `Capture: ${profile.selected_capture}${profile.selected_capture_reason ? ` (${profile.selected_capture_reason})` : ""}`,
            `Runtime Candidates: ${runtimeSummary || "n/a"}`,
        ]

        if (profile.selected_ffmpeg_source === "external-path") {
            lines.push("Warning: host sedang probe memakai ffmpeg luar bundle; runtime belum sepenuhnya self-contained")
        }

        if (profile.warnings.length > 0) {
            lines.push(`Warnings:\n- ${profile.warnings.join("\n- ")}`)
        }

        if (failedProbes.length > 0) {
            lines.push(`Probe Fallbacks:\n- ${failedProbes.join("\n- ")}`)
        }

        this.hostCapabilityDetails.innerText = lines.join("\n")
    }

    private onUserSearchChange() {
        this.userList.setFilter(this.userSearch.value)
    }

    private async onUserClicked(event: ComponentEvent<User>) {
        const user = await apiGetUser(this.api, {
            user_id: event.component.getUserId(),
            name: null
        })

        this.setUserInfo(user)
    }
    private setUserInfo(user: DetailedUser | null) {
        if (this.userInfoPage) {
            this.userInfoPage.unmount(this.content)
            this.userInfoPage.removeDeletedListener(this.onUserDeleted.bind(this))
        }

        this.userInfoPage = null
        if (user) {
            this.userInfoPage = new DetailedUserPage(this.api, user)
            this.userInfoPage.addDeletedListener(this.onUserDeleted.bind(this))
            this.userInfoPage.mount(this.content)
        }
    }

    private onUserDeleted(event: ComponentEvent<User>) {
        if (this.userInfoPage?.getUserId() == event.component.getUserId()) {
            this.setUserInfo(null)
        }
        this.userList.removeUser(event.component.getUserId())
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.root)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.root)
    }
}
