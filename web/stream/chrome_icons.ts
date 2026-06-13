export type StreamChromeVariant = "hero" | "secondary" | "utility"
export type StreamChromeTone =
    | "neutral"
    | "connection"
    | "success"
    | "relay"
    | "danger"
    | "audio"
    | "quality"
    | "input"
    | "desktop"
    | "browser"
    | "files"
    | "coding"
    | "debug"
    | "warning"

export type StreamChromeIconId =
    | "signal"
    | "direct"
    | "relay"
    | "socket"
    | "warning"
    | "audio"
    | "mic"
    | "quality"
    | "menu"
    | "gear"
    | "keyboard"
    | "gamepad"
    | "layout"
    | "touch"
    | "stats"
    | "arrowleft"
    | "arrowright"
    | "arrowup"
    | "arrowdown"
    | "home"
    | "fullscreen"
    | "refresh"
    | "click"
    | "rightclick"
    | "mouse"
    | "pointer"
    | "escape"
    | "copy"
    | "paste"
    | "cut"
    | "save"
    | "trash"
    | "desktop"
    | "switch"
    | "debug"
    | "more"
    | "browser"
    | "folder"
    | "text"
    | "code"
    | "terminal"
    | "search"
    | "clipboard"
    | "window"
    | "media"
    | "panel"
    | "function"
    | "zoom"
    | "lock"
    | "play"
    | "stop"
    | "select"

export type StreamChromeSpec = {
    icon: StreamChromeIconId
    tone: StreamChromeTone
    variant?: StreamChromeVariant
    state?: string
}

const SVG_NS = "http://www.w3.org/2000/svg"

function createSvg(pathMarkup: string): SVGSVGElement {
    const svg = document.createElementNS(SVG_NS, "svg")
    svg.setAttribute("viewBox", "0 0 20 20")
    svg.setAttribute("fill", "none")
    svg.setAttribute("aria-hidden", "true")
    svg.classList.add("stream-chrome-icon-svg")
    svg.innerHTML = pathMarkup
    return svg
}

export function createStreamChromeIcon(icon: StreamChromeIconId): SVGSVGElement {
    switch (icon) {
    case "direct":
        return createSvg(`
            <path d="M10 2.6l5 2v4.2c0 3.2-1.9 6.1-5 7.6-3.1-1.5-5-4.4-5-7.6V4.6l5-2z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>
            <path d="M7.2 9.9l1.8 1.8 3.8-4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "relay":
        return createSvg(`
            <path d="M5 5.2h3.2M11.8 5.2H15" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            <path d="M5 14.8h3.2M11.8 14.8H15" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            <circle cx="10" cy="5.2" r="1.8" stroke="currentColor" stroke-width="1.7"/>
            <circle cx="10" cy="14.8" r="1.8" stroke="currentColor" stroke-width="1.7"/>
            <path d="M10 7.2v5.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "socket":
        return createSvg(`
            <rect x="4.4" y="4.4" width="11.2" height="11.2" rx="2.6" stroke="currentColor" stroke-width="1.7"/>
            <path d="M7.1 7.5h5.8M7.1 10h5.8M7.1 12.5h3.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "warning":
        return createSvg(`
            <path d="M10 3.4l6.1 10.8a1.1 1.1 0 01-.9 1.6H4.8a1.1 1.1 0 01-.9-1.6L10 3.4z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>
            <path d="M10 7.2v3.7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
            <circle cx="10" cy="13.7" r="0.9" fill="currentColor"/>
        `)
    case "audio":
        return createSvg(`
            <path d="M4.8 12.2H2.9V7.8h1.9l3.2-2.4v9.2l-3.2-2.4z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>
            <path d="M11.3 7.1a4 4 0 010 5.8M13.6 5a6.7 6.7 0 010 10" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "mic":
        return createSvg(`
            <rect x="7.2" y="3.6" width="5.6" height="8.6" rx="2.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M5.8 9.7a4.2 4.2 0 008.4 0M10 13.9v2.5M7.4 16.4h5.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "quality":
        return createSvg(`
            <path d="M4.2 14.8l3.2-4 2.6 1.9 4-5.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
            <circle cx="4.2" cy="14.8" r="1.2" fill="currentColor"/>
            <circle cx="7.4" cy="10.8" r="1.2" fill="currentColor"/>
            <circle cx="10" cy="12.7" r="1.2" fill="currentColor"/>
            <circle cx="14" cy="7.2" r="1.2" fill="currentColor"/>
        `)
    case "menu":
        return createSvg(`
            <rect x="3.8" y="3.8" width="5.2" height="5.2" rx="1.2" stroke="currentColor" stroke-width="1.6"/>
            <rect x="11" y="3.8" width="5.2" height="5.2" rx="1.2" stroke="currentColor" stroke-width="1.6"/>
            <rect x="3.8" y="11" width="5.2" height="5.2" rx="1.2" stroke="currentColor" stroke-width="1.6"/>
            <rect x="11" y="11" width="5.2" height="5.2" rx="1.2" stroke="currentColor" stroke-width="1.6"/>
        `)
    case "gear":
        return createSvg(`
            <circle cx="10" cy="10" r="2.6" stroke="currentColor" stroke-width="1.7"/>
            <path d="M10 2.9v1.9M10 15.2v1.9M17.1 10h-1.9M4.8 10H2.9M15 5l-1.4 1.4M6.4 13.6L5 15M15 15l-1.4-1.4M6.4 6.4L5 5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            <circle cx="10" cy="10" r="6.2" stroke="currentColor" stroke-width="1.5" stroke-dasharray="1.2 3.4" stroke-linecap="round"/>
        `)
    case "keyboard":
        return createSvg(`
            <rect x="2.8" y="5.2" width="14.4" height="9.6" rx="2" stroke="currentColor" stroke-width="1.6"/>
            <path d="M5.5 8.1h.1M8.1 8.1h.1M10.7 8.1h.1M13.3 8.1h.1M5.5 10.6h.1M8.1 10.6h.1M10.7 10.6h.1M13.3 10.6h.1" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>
            <path d="M6.1 13.1h7.8" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "gamepad":
        return createSvg(`
            <path d="M6.4 6h7.2c1.7 0 3 1.4 3 3.1 0 1-.4 1.9-1.1 2.5l-1.1 3a1.2 1.2 0 01-2.2.2l-1-1.7H8.8l-1 1.7a1.2 1.2 0 01-2.2-.2l-1.1-3A3.3 3.3 0 013.4 9c0-1.7 1.3-3 3-3z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M6.3 9.8h2.4M7.5 8.6v2.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
            <circle cx="12.6" cy="9.1" r="0.95" fill="currentColor"/>
            <circle cx="14.4" cy="10.8" r="0.95" fill="currentColor"/>
        `)
    case "layout":
        return createSvg(`
            <rect x="3.2" y="4.2" width="13.6" height="11.6" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.3 4.8v10.4M8.6 8.1h6M8.6 11.9h3.8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "touch":
        return createSvg(`
            <path d="M8.6 10.2V6.3a1.3 1.3 0 112.6 0v2.9a1.2 1.2 0 112.4 0v.7a1.1 1.1 0 112.2 0v1.7c0 2.8-1.9 4.7-4.8 4.7H9.6c-1.4 0-2.4-.7-3-1.9L4.7 11a1.1 1.1 0 011.8-1.3l2.1 2.8z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
        `)
    case "stats":
        return createSvg(`
            <path d="M4.4 14.8V9.9M10 14.8V6.1M15.6 14.8v-3.4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
            <path d="M3.2 15.8h13.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "arrowleft":
        return createSvg(`
            <path d="M15 10H5.8M9.2 6.6L5.8 10l3.4 3.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "arrowright":
        return createSvg(`
            <path d="M5 10h9.2M10.8 6.6l3.4 3.4-3.4 3.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "arrowup":
        return createSvg(`
            <path d="M10 15V5.8M6.6 9.2L10 5.8l3.4 3.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "arrowdown":
        return createSvg(`
            <path d="M10 5v9.2M6.6 10.8l3.4 3.4 3.4-3.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "home":
        return createSvg(`
            <path d="M4.8 8.6L10 4.2l5.2 4.4v6.4H4.8z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M8.3 15V11h3.4v4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "fullscreen":
        return createSvg(`
            <path d="M7.1 3.9H3.9v3.2M12.9 3.9h3.2v3.2M7.1 16.1H3.9v-3.2M12.9 16.1h3.2v-3.2" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "refresh":
        return createSvg(`
            <path d="M15.2 8.1A5.4 5.4 0 005 7.5l1.1-2.1M4.8 11.9A5.4 5.4 0 0015 12.5l-1.1 2.1" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "mouse":
        return createSvg(`
            <path d="M10 3.4c-2.5 0-4.4 2-4.4 4.5v3.8c0 3 1.8 4.9 4.4 4.9s4.4-1.9 4.4-4.9V7.9c0-2.5-1.9-4.5-4.4-4.5z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M10 3.8v4.1M5.9 8.1h8.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        `)
    case "click":
        return createSvg(`
            <path d="M10 3.4c-2.5 0-4.4 2-4.4 4.5v3.8c0 3 1.8 4.9 4.4 4.9s4.4-1.9 4.4-4.9V7.9c0-2.5-1.9-4.5-4.4-4.5z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M6.1 7.9c0-2 1.6-3.7 3.7-3.7v3.7H6.1z" fill="currentColor" opacity="0.88"/>
            <path d="M4.8 5.2H3.1M5.6 3.9l-1-1.2M7.1 3.3V1.9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        `)
    case "rightclick":
        return createSvg(`
            <path d="M10 3.4c-2.5 0-4.4 2-4.4 4.5v3.8c0 3 1.8 4.9 4.4 4.9s4.4-1.9 4.4-4.9V7.9c0-2.5-1.9-4.5-4.4-4.5z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M10.2 4.2c2.1 0 3.7 1.7 3.7 3.7h-3.7V4.2z" fill="currentColor" opacity="0.88"/>
            <path d="M15.2 5.2h1.7M14.4 3.9l1-1.2M12.9 3.3V1.9" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        `)
    case "pointer":
        return createSvg(`
            <path d="M6 4.2l7 6.3-3.1.8 1.6 3.2-1.8.9-1.6-3.2-2.1 2z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
        `)
    case "escape":
        return createSvg(`
            <rect x="3.2" y="4.4" width="13.6" height="11.2" rx="2" stroke="currentColor" stroke-width="1.6"/>
            <path d="M11.8 8.2H7.4M7.4 8.2l2-2M7.4 8.2l2 2M13.2 11.8H9.8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "copy":
        return createSvg(`
            <rect x="7" y="5.2" width="7.2" height="9.6" rx="1.6" stroke="currentColor" stroke-width="1.6"/>
            <rect x="4" y="8.2" width="7.2" height="7.2" rx="1.4" stroke="currentColor" stroke-width="1.6"/>
        `)
    case "paste":
        return createSvg(`
            <rect x="5.2" y="4.8" width="9.6" height="11" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.4 4.8a1.8 1.8 0 013.6 0h1.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
            <path d="M8 9.6h4M8 12.2h4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "cut":
        return createSvg(`
            <circle cx="6.2" cy="13.5" r="1.7" stroke="currentColor" stroke-width="1.6"/>
            <circle cx="13.8" cy="13.5" r="1.7" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.5 12.2l5-5.6M12.5 12.2l-5-5.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "save":
        return createSvg(`
            <path d="M5 4.4h8.6l1.8 1.8v9.4H5z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M7 4.8v4.2h5.2V4.8M7.6 13h4.8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "trash":
        return createSvg(`
            <path d="M6.2 6.2h7.6M7.4 6.2V4.8h5.2v1.4M7 8.2l.5 6h5l.5-6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "desktop":
        return createSvg(`
            <rect x="3.2" y="4.1" width="13.6" height="9.1" rx="1.6" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.2 15.6h5.6M10 13.2v2.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "switch":
        return createSvg(`
            <path d="M6.2 6.1h9l-2-2M13.8 13.9h-9l2 2" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
            <path d="M15.2 6.1A5.2 5.2 0 016 13.9" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "debug":
        return createSvg(`
            <path d="M7.1 6.1h5.8v2H7.1zM8.1 3.8h3.8M6.5 10.2c0-1.9 1.6-3.5 3.5-3.5s3.5 1.6 3.5 3.5v2.1c0 1.9-1.6 3.5-3.5 3.5s-3.5-1.6-3.5-3.5z" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
            <path d="M4.6 8.6l1.6 1M15.4 8.6l-1.6 1M4.4 12h1.6M14 12h1.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        `)
    case "more":
        return createSvg(`
            <circle cx="5" cy="10" r="1.2" fill="currentColor"/>
            <circle cx="10" cy="10" r="1.2" fill="currentColor"/>
            <circle cx="15" cy="10" r="1.2" fill="currentColor"/>
        `)
    case "browser":
        return createSvg(`
            <rect x="3.1" y="4.1" width="13.8" height="11.8" rx="2" stroke="currentColor" stroke-width="1.6"/>
            <path d="M3.8 7.2h12.4" stroke="currentColor" stroke-width="1.6"/>
            <path d="M6.1 5.7h.1M8.3 5.7h.1" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>
        `)
    case "folder":
        return createSvg(`
            <path d="M3.4 6.2h4.2l1.3 1.4h7.7v6.2a1.6 1.6 0 01-1.6 1.6H5a1.6 1.6 0 01-1.6-1.6z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M3.4 7.6V5.8A1.6 1.6 0 015 4.2h2.5l1.3 1.4h6.6a1.6 1.6 0 011.6 1.6v.4" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
        `)
    case "text":
        return createSvg(`
            <path d="M4.4 5.1h11.2M10 5.1v9.8M7.1 14.9h5.8" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "code":
        return createSvg(`
            <path d="M7.5 6.1L4.2 9.4l3.3 3.3M12.5 6.1l3.3 3.3-3.3 3.3M11.2 4.8L8.8 15.2" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "terminal":
        return createSvg(`
            <rect x="3.1" y="4.2" width="13.8" height="11.6" rx="2" stroke="currentColor" stroke-width="1.6"/>
            <path d="M6.1 8l2.2 2-2.2 2M10.2 13h3.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "search":
        return createSvg(`
            <circle cx="8.4" cy="8.4" r="4" stroke="currentColor" stroke-width="1.7"/>
            <path d="M11.5 11.5l4 4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "clipboard":
        return createSvg(`
            <rect x="5.1" y="4.6" width="9.8" height="11.2" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.4 4.8a1.8 1.8 0 013.6 0h1.4a1 1 0 011 1v.6H6v-.6a1 1 0 011-1h.4z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M7.8 9.4h4.4M7.8 12h4.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "window":
        return createSvg(`
            <rect x="3.3" y="4.2" width="13.4" height="11.6" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M3.9 7.4h12.2M7.5 4.8h.1M9.7 4.8h.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "media":
        return createSvg(`
            <path d="M6.1 6.1v7.8M13.9 6.1v7.8M8.4 8.2l4 1.8-4 1.8z" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "panel":
        return createSvg(`
            <rect x="3.2" y="4.2" width="13.6" height="11.6" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.4 4.8v10.4M8.4 7.2h6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "function":
        return createSvg(`
            <path d="M5.2 5.6h9.6M5.2 10h6.2M5.2 14.4h4.2" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
            <path d="M6 4.3v11.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" opacity="0.7"/>
        `)
    case "zoom":
        return createSvg(`
            <circle cx="8.4" cy="8.4" r="4.2" stroke="currentColor" stroke-width="1.6"/>
            <path d="M11.8 11.8l3.4 3.4M8.4 6.6v3.6M6.6 8.4h3.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
        `)
    case "lock":
        return createSvg(`
            <rect x="5.2" y="8.4" width="9.6" height="7" rx="1.8" stroke="currentColor" stroke-width="1.6"/>
            <path d="M7.2 8.4V6.9a2.8 2.8 0 115.6 0v1.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        `)
    case "play":
        return createSvg(`
            <path d="M6.8 5.8l7 4.2-7 4.2z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>
        `)
    case "stop":
        return createSvg(`
            <rect x="5.5" y="5.5" width="9" height="9" rx="1.4" stroke="currentColor" stroke-width="1.7"/>
        `)
    case "select":
        return createSvg(`
            <path d="M5.2 5.4h9.6M5.2 10h6.8M5.2 14.6h9.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            <path d="M14.8 9l1.6 1.6-1.6 1.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
        `)
    case "signal":
    default:
        return createSvg(`
            <path d="M4.8 13.8a7.4 7.4 0 0110.4 0M7.5 11.1a3.6 3.6 0 015 0" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            <circle cx="10" cy="15.1" r="1.05" fill="currentColor"/>
        `)
    }
}

function normalizeLabel(label: string): string {
    return label.toLowerCase().replace(/\s+/g, " ").trim()
}

const EXACT_LABEL_SPECS: Record<string, StreamChromeSpec> = {
    "connection": { icon: "signal", tone: "connection" },
    "audio": { icon: "audio", tone: "audio" },
    "mic on": { icon: "mic", tone: "audio" },
    "mic off": { icon: "mic", tone: "audio" },
    "mic n/a": { icon: "mic", tone: "warning" },
    "mic blocked": { icon: "mic", tone: "warning" },
    "quick quality": { icon: "quality", tone: "quality" },
    "desktop actions": { icon: "desktop", tone: "desktop" },
    "quick keys": { icon: "keyboard", tone: "desktop" },
    "mouse actions": { icon: "mouse", tone: "input" },
    "navigation": { icon: "select", tone: "desktop" },
    "media keys": { icon: "media", tone: "audio" },
    "browser & system": { icon: "browser", tone: "browser" },
    "function keys": { icon: "function", tone: "desktop" },
    "window controls": { icon: "window", tone: "desktop" },
    "edit shortcuts": { icon: "clipboard", tone: "files" },
    "tab shortcuts": { icon: "browser", tone: "browser" },
    "workspace": { icon: "desktop", tone: "desktop" },
    "virtual desktop": { icon: "switch", tone: "desktop" },
    "windows panels": { icon: "panel", tone: "desktop" },
    "session & utility": { icon: "lock", tone: "desktop" },
    "taskbar": { icon: "window", tone: "desktop" },
    "file actions": { icon: "folder", tone: "files" },
    "text caret": { icon: "text", tone: "files" },
    "view & zoom": { icon: "zoom", tone: "browser" },
    "browser power": { icon: "browser", tone: "browser" },
    "text select": { icon: "select", tone: "files" },
    "editor tools": { icon: "code", tone: "coding" },
    "code nav": { icon: "search", tone: "coding" },
    "debug flow": { icon: "debug", tone: "coding" },
    "terminal ops": { icon: "terminal", tone: "coding" },
    "search flow": { icon: "search", tone: "coding" },
    "code actions": { icon: "code", tone: "coding" },
    "line ops": { icon: "text", tone: "coding" },
    "multi cursor": { icon: "select", tone: "coding" },
    "refactor": { icon: "code", tone: "coding" },
    "symbols": { icon: "search", tone: "coding" },
    "panels": { icon: "panel", tone: "coding" },
    "fold": { icon: "code", tone: "coding" },
    "selection": { icon: "select", tone: "coding" },
    "editor layout": { icon: "window", tone: "coding" },
    "terminal layout": { icon: "terminal", tone: "coding" },
    "indent & comment": { icon: "text", tone: "coding" },
    "replace ops": { icon: "search", tone: "coding" },
    "explorer ops": { icon: "folder", tone: "files" },
    "gamepad": { icon: "gamepad", tone: "input" },
    "quickpad tuning": { icon: "gamepad", tone: "input" },
    "touch tuning": { icon: "touch", tone: "input" },
    "stream quality": { icon: "quality", tone: "quality" },
    "main": { icon: "menu", tone: "connection" },
    "advanced": { icon: "quality", tone: "quality" },
    "stream console": { icon: "menu", tone: "connection" },
    "core": { icon: "pointer", tone: "input" },
    "session": { icon: "quality", tone: "quality" },
    "tools": { icon: "debug", tone: "debug" },
    "touch debug": { icon: "debug", tone: "debug" },
    "close stats": { icon: "stats", tone: "debug" },
    "menu": { icon: "menu", tone: "neutral" },
    "keyboard": { icon: "keyboard", tone: "desktop" },
    "pad on": { icon: "gamepad", tone: "input" },
    "pad off": { icon: "gamepad", tone: "input" },
    "pad cmp": { icon: "layout", tone: "input" },
    "pad bal": { icon: "layout", tone: "input" },
    "pad wide": { icon: "layout", tone: "input" },
    "touch dsk": { icon: "touch", tone: "input" },
    "touch mob": { icon: "touch", tone: "input" },
    "touch pre": { icon: "touch", tone: "input" },
    "stats": { icon: "stats", tone: "quality" },
    "hide stats": { icon: "stats", tone: "quality" },
    "mute": { icon: "audio", tone: "audio" },
    "unmute": { icon: "audio", tone: "audio" },
    "host audio on": { icon: "audio", tone: "audio" },
    "host audio off": { icon: "audio", tone: "audio" },
    "full": { icon: "fullscreen", tone: "desktop" },
    "exit fs": { icon: "fullscreen", tone: "desktop" },
    "reconnect": { icon: "refresh", tone: "warning" },
    "busy...": { icon: "refresh", tone: "warning" },
    "click": { icon: "click", tone: "input" },
    "double": { icon: "click", tone: "input" },
    "left": { icon: "click", tone: "input" },
    "right": { icon: "rightclick", tone: "input" },
    "right click": { icon: "rightclick", tone: "input" },
    "mouse rel": { icon: "mouse", tone: "input" },
    "mouse fol": { icon: "mouse", tone: "input" },
    "dragdrop": { icon: "mouse", tone: "input" },
    "desk": { icon: "desktop", tone: "desktop" },
    "esc": { icon: "escape", tone: "desktop" },
    "task": { icon: "switch", tone: "desktop" },
    "dbg": { icon: "debug", tone: "debug" },
    "dbg on": { icon: "debug", tone: "debug" },
    "more": { icon: "more", tone: "neutral" },
    "less": { icon: "more", tone: "neutral" },
    "low lag": { icon: "quality", tone: "quality" },
    "balanced": { icon: "quality", tone: "quality" },
    "sharp": { icon: "zoom", tone: "quality" },
    "desktop": { icon: "desktop", tone: "desktop" },
    "shooter": { icon: "gamepad", tone: "input" },
    "couch": { icon: "gamepad", tone: "input" },
    "portrait": { icon: "fullscreen", tone: "input" },
    "landscape": { icon: "fullscreen", tone: "input" },
    "apply & reconnect": { icon: "refresh", tone: "warning" },
    "reconnect pad": { icon: "refresh", tone: "input" },
    "paste text": { icon: "paste", tone: "files" },
    "copy": { icon: "copy", tone: "files" },
    "paste": { icon: "paste", tone: "files" },
    "cut": { icon: "cut", tone: "files" },
    "save": { icon: "save", tone: "files" },
    "undo": { icon: "arrowleft", tone: "files" },
    "redo": { icon: "arrowright", tone: "files" },
    "back": { icon: "arrowleft", tone: "browser" },
    "fwd": { icon: "arrowright", tone: "browser" },
    "home": { icon: "home", tone: "browser" },
    "up": { icon: "arrowup", tone: "desktop" },
    "down": { icon: "arrowdown", tone: "desktop" },
    "pgup": { icon: "arrowup", tone: "desktop" },
    "pgdn": { icon: "arrowdown", tone: "desktop" },
    "prev": { icon: "arrowleft", tone: "audio" },
    "next": { icon: "arrowright", tone: "audio" },
    "prev tab": { icon: "arrowleft", tone: "browser" },
    "next tab": { icon: "arrowright", tone: "browser" },
    "rename": { icon: "text", tone: "files" },
    "delete": { icon: "trash", tone: "warning" },
    "del perm": { icon: "trash", tone: "danger" },
    "open": { icon: "folder", tone: "files" },
    "preview": { icon: "search", tone: "files" },
    "run": { icon: "play", tone: "coding" },
    "stop": { icon: "stop", tone: "coding" },
    "build": { icon: "code", tone: "coding" }
}

export function inferStreamChromeSpec(label: string): StreamChromeSpec {
    const key = normalizeLabel(label)

    const exact = EXACT_LABEL_SPECS[key]
    if (exact) {
        return exact
    }

    if (key.includes("direct p2p")) return { icon: "direct", tone: "success" }
    if (key.includes("turn relay")) return { icon: "relay", tone: "relay" }
    if (key.includes("ws relay")) return { icon: "socket", tone: "relay" }
    if (key.includes("failed")) return { icon: "warning", tone: "danger" }
    if (key.includes("negotiating") || key.includes("connecting") || key.includes("pending") || key.includes("busy")) return { icon: "signal", tone: "warning" }

    if (/(mic on|mic off|mic n\/a|mic blocked|microphone|mic\b)/.test(key)) return { icon: "mic", tone: /blocked|n\/a/.test(key) ? "warning" : "audio" }
    if (/(mute|unmute|audio|vol\+|vol-|play|next|prev|media)/.test(key)) return { icon: "audio", tone: "audio" }
    if (/(quality|bitrate|codec|fps|latency|sharp|low lag|balanced|profile|preset)/.test(key)) return { icon: "quality", tone: "quality" }
    if (/(menu|more|less|utilities|main|main ui)/.test(key)) return { icon: key.includes("more") || key.includes("less") ? "more" : "menu", tone: "neutral" }
    if (/(keyboard|ctrl|shift|alt|tab|esc|enter|backsp|space|keycode|f\d+$|function)/.test(key)) return { icon: key.match(/^f\d+$/) ? "function" : "keyboard", tone: "desktop" }
    if (/(pad|gamepad|trigger|stick|turbo|l3|r3|abxy|conflict guard)/.test(key)) return { icon: "gamepad", tone: "input" }
    if (/(touch|track|tp |tp\b|mouse|click|scroll|caret|two-finger|right click|left click|middle|select|^right$)/.test(key)) {
        if (/(right click|^right$)/.test(key)) return { icon: "rightclick", tone: "input" }
        if (/(click)/.test(key)) return { icon: "click", tone: "input" }
        return { icon: /(mouse|scroll)/.test(key) ? "mouse" : /(select|caret)/.test(key) ? "select" : "touch", tone: "input" }
    }
    if (/(stats|rtt|host|hud)/.test(key)) return { icon: "stats", tone: "quality" }
    if (/(fullscreen|full|zoom|view)/.test(key)) return { icon: /(zoom)/.test(key) ? "zoom" : "fullscreen", tone: "desktop" }
    if (/(reconnect|refresh|retry|reset|reload|update|applying)/.test(key)) return { icon: "refresh", tone: "warning" }
    if (/(desktop|desk|task|window|snap|max|min|restore|workspace|virtual desktop|tray|run|taskmgr|explorer|settings|show desk|notify|widgets|game bar|emoji|connect|mail|access|lock|sleep|session)/.test(key)) {
        return { icon: /(lock|sleep|session)/.test(key) ? "lock" : /(task|switch|desk \+|desk -|virtual desktop)/.test(key) ? "switch" : /(window|snap|max|min|restore)/.test(key) ? "window" : "desktop", tone: "desktop" }
    }
    if (/(browser|bookmark|downloads|history|addr|private|new tab|new win|fav|home|search|fwd|back|refresh)/.test(key)) {
        return { icon: /(search)/.test(key) ? "search" : "browser", tone: "browser" }
    }
    if (/(file|folder|rename|delete|open|props|preview|document)/.test(key)) return { icon: /(document|text)/.test(key) ? "text" : "folder", tone: "files" }
    if (/(copy|paste|cut|undo|redo|clipboard|snip)/.test(key)) return { icon: "clipboard", tone: "files" }
    if (/(code|editor|debug|build|step|symbol|outline|peek|refs|problems|terminal|search flow|replace|find|command|quick fix|refactor|fold|selection|multi cursor|line ops|comment|indent|format|workbench|panels|scm)/.test(key)) {
        if (/(terminal|cmd)/.test(key)) return { icon: "terminal", tone: "coding" }
        if (/(search|find|replace|symbol|refs|peek|line|outline)/.test(key)) return { icon: "search", tone: "coding" }
        if (/(debug|build|step|problems|breakpt|run)/.test(key)) return { icon: "debug", tone: "coding" }
        if (/(panel|workbench|sidebar|output|zen)/.test(key)) return { icon: "panel", tone: "coding" }
        return { icon: "code", tone: "coding" }
    }
    if (/(dbg|freeze|live|share|view|clear|auto|snap|logs)/.test(key)) return { icon: "debug", tone: "debug" }

    return { icon: "menu", tone: "neutral" }
}

function buildContent(label: string, spec: StreamChromeSpec, className: string) {
    const fragment = document.createDocumentFragment()
    const content = document.createElement("span")
    const iconWrap = document.createElement("span")
    const labelWrap = document.createElement("span")

    content.className = className
    iconWrap.className = `${className}-iconwrap`
    labelWrap.className = `${className}-label`
    iconWrap.appendChild(createStreamChromeIcon(spec.icon))
    labelWrap.innerText = label
    content.appendChild(iconWrap)
    content.appendChild(labelWrap)
    fragment.appendChild(content)
    return fragment
}

export function applyStreamChromeChip(target: HTMLElement, label: string, spec: StreamChromeSpec) {
    target.classList.add("stream-chrome-chip")
    target.dataset.tone = spec.tone
    target.dataset.variant = spec.variant ?? "secondary"
    if (spec.state) {
        target.dataset.state = spec.state
    } else {
        delete target.dataset.state
    }
    target.replaceChildren(buildContent(label, spec, "stream-chrome-chip-content"))
}

export function applyStreamChromeInlineLabel(target: HTMLElement, label: string, spec: StreamChromeSpec) {
    target.classList.add("stream-inline-label-root")
    target.dataset.tone = spec.tone
    target.replaceChildren(buildContent(label, spec, "stream-inline-label"))
}
