export type ExplicitStreamRoute = "direct" | "relay"

export function parseExplicitStreamRoute(value: unknown): ExplicitStreamRoute | null {
    const normalized = typeof value == "string" ? value.trim().toLowerCase() : ""
    return normalized == "direct" || normalized == "relay" ? normalized : null
}

export function readExplicitStreamRoute(search: string = window.location.search): ExplicitStreamRoute | null {
    return parseExplicitStreamRoute(new URLSearchParams(search).get("autoRoute"))
}

function urls(server: RTCIceServer): string[] {
    return Array.isArray(server.urls) ? server.urls : [server.urls]
}

function withUrls(server: RTCIceServer, selected: string[]): RTCIceServer {
    return { ...server, urls: selected }
}

export function candidateAllowed(route: ExplicitStreamRoute | null, candidate: string): boolean {
    if (!route) return false
    if (!candidate.trim()) return true // end-of-candidates
    const type = /(?:^|\s)typ\s+(\S+)/i.exec(candidate)?.[1]?.toLowerCase()
    if (!type || !["host", "srflx", "prflx", "relay"].includes(type)) return false
    // Relay-only on the local peer guarantees VPS transit; its remote peer may be direct.
    return route == "relay" || type != "relay"
}

export function filterRouteSdp(route: ExplicitStreamRoute | null, sdp: string): string {
    return sdp.split(/\r?\n/).filter(line => !line.startsWith("a=candidate:")
        || candidateAllowed(route, line.slice(2))).join("\r\n")
}

export function routeIceConfiguration(
    iceServers: RTCIceServer[],
    route: ExplicitStreamRoute
): RTCConfiguration {
    const selectedServers: RTCIceServer[] = []
    for (const server of iceServers) {
        const selected = urls(server).filter(url => route == "relay"
            ? /^turns?:/i.test(url)
            : !/^turns?:/i.test(url))
        if (selected.length > 0) selectedServers.push(withUrls(server, selected))
    }
    return {
        iceServers: selectedServers,
        iceTransportPolicy: route == "relay" ? "relay" : "all"
    }
}
