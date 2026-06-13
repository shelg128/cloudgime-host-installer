import { defaultSettings, getLocalStreamSettings } from "../component/settings_menu.js"

// old doesn't exist anymore and is always replaced with moonlight when loading the settings
export type PageStyle = "standard" | "old" | "moonlight"

let currentStyle: PageStyle | null = null
const styleLink = document.getElementById("style") as HTMLLinkElement
const styleVersionSuffix = (() => {
    const rawHref = styleLink?.getAttribute("href") ?? ""
    const queryIndex = rawHref.indexOf("?")
    return queryIndex >= 0 ? rawHref.slice(queryIndex) : ""
})()

function toAbsolute(path: string) {
    return new URL(path, document.baseURI).href
}

export function setStyle(style: PageStyle) {
    if (!currentStyle) {
        document.head.appendChild(styleLink)
    }

    const path = `styles/${style}.css${styleVersionSuffix}`
    const absolute = toAbsolute(path)

    if (styleLink.href !== absolute) {
        styleLink.href = absolute
    }

    currentStyle = style
}

export function getStyle(): PageStyle {
    return currentStyle as PageStyle
}

const settings = getLocalStreamSettings()
const defaultSettings_ = defaultSettings()

setStyle(settings?.pageStyle ?? defaultSettings_.pageStyle)
