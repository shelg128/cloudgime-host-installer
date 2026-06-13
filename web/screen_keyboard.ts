export type TextEvent = CustomEvent<{ text: string }>
export type ScreenKeyboardVisibilityEvent = CustomEvent<{ visible: boolean }>

type ScreenKeyboardShowOptions = {
    persistent?: boolean
    explicitTrigger?: boolean
}

type ScreenKeyboardHideOptions = {
    explicitClose?: boolean
}

export class ScreenKeyboard {

    private eventTarget = new EventTarget()
    private fakeElement = document.createElement("input")
    private dismissSink = document.createElement("button")
    private virtualKeyboardApi: any = typeof navigator != "undefined" ? (navigator as any).virtualKeyboard ?? null : null

    private visible = false
    private persistent = false
    private isKeyboardExplicitlyClosed = false
    private suppressDocumentHideUntil = 0
    private refocusTimer: number | null = null
    private blurDecisionTimer: number | null = null
    private lastStreamPointerDownAt = 0
    private suppressBlurUntil = 0
    private maxViewportHeightSeen = 0
    private persistentViewportWasCompressed = false
    private virtualKeyboardWasVisible = false

    constructor() {
        this.fakeElement.classList.add("hiddeninput")
        this.fakeElement.type = "text"
        this.fakeElement.name = "keyboard"
        this.fakeElement.inputMode = "none"
        this.fakeElement.enterKeyHint = "done"
        this.fakeElement.autocomplete = "off"
        this.fakeElement.autocapitalize = "off"
        this.fakeElement.spellcheck = false
        this.fakeElement.readOnly = true
        this.fakeElement.setAttribute("aria-label", "Hidden screen keyboard input")
        if ("autocorrect" in this.fakeElement) {
            this.fakeElement.autocorrect = false
        }
        this.dismissSink.classList.add("hiddeninput")
        this.dismissSink.type = "button"
        this.dismissSink.tabIndex = -1
        this.dismissSink.setAttribute("aria-hidden", "true")
        this.dismissSink.disabled = true
        try {
            ;(this.fakeElement as any).virtualKeyboardPolicy = "manual"
        } catch {
            // Ignore browsers that expose the property but reject writes.
        }

        this.fakeElement.addEventListener("input", this.onKeyInput.bind(this))

        document.addEventListener("pointerdown", this.onDocumentPointerDown.bind(this), true)
        this.fakeElement.addEventListener("blur", this.onInputBlur.bind(this))
        window.addEventListener("resize", this.onViewportMaybeChanged.bind(this), { passive: true })
        window.visualViewport?.addEventListener("resize", this.onViewportMaybeChanged.bind(this), { passive: true })
        this.virtualKeyboardApi?.addEventListener?.("geometrychange", this.onVirtualKeyboardGeometryChange.bind(this))
        if (this.virtualKeyboardApi && "overlaysContent" in this.virtualKeyboardApi) {
            try {
                this.virtualKeyboardApi.overlaysContent = true
            } catch {
                // Ignore browsers that expose the API but reject writes.
            }
        }

        // Seed the value once so delete/backspace can fire even before any text was typed.
        this.updateViewportMetrics()
        this.resetInputBuffer()
    }

    getHiddenElement() {
        return this.fakeElement
    }

    show(options?: ScreenKeyboardShowOptions): boolean {
        const explicitTrigger = !!options?.explicitTrigger
        if (this.isKeyboardExplicitlyClosed && !explicitTrigger) {
            return false
        }

        const wasVisible = this.visible
        this.visible = true
        this.persistent = !!options?.persistent
        this.persistentViewportWasCompressed = false
        this.virtualKeyboardWasVisible = this.getVirtualKeyboardHeight() > 0
        this.updateViewportMetrics()
        if (explicitTrigger) {
            this.isKeyboardExplicitlyClosed = false
        }

        this.clearBlurDecisionTimer()
        this.clearRefocusTimer()
        this.ensureMounted()
        this.suppressDocumentHideUntil = Date.now() + 480
        this.suppressBlurUntil = Date.now() + 640
        this.fakeElement.disabled = false
        this.dismissSink.disabled = true
        this.fakeElement.readOnly = false
        this.fakeElement.inputMode = "text"
        this.resetInputBuffer()
        this.focusInput()
        this.requestVirtualKeyboardShow()
        if (!wasVisible) {
            this.emitVisibilityChange()
        }
        return true
    }

    hide(force = false, options?: ScreenKeyboardHideOptions) {
        const explicitClose = !!options?.explicitClose
        if (this.persistent && !force && !explicitClose) {
            this.scheduleRefocus()
            return
        }

        this.applyHiddenState(true, explicitClose)
    }

    isVisible(): boolean {
        return this.visible
    }

    wasExplicitlyClosed(): boolean {
        return this.isKeyboardExplicitlyClosed
    }

    addVisibilityChangeListener(listener: (event: ScreenKeyboardVisibilityEvent) => void) {
        this.eventTarget.addEventListener("ml-screen-keyboard-visibility", listener as any)
    }

    addKeyDownListener(listener: (event: KeyboardEvent) => void) {
        this.eventTarget.addEventListener("keydown", listener as any)
    }

    addKeyUpListener(listener: (event: KeyboardEvent) => void) {
        this.eventTarget.addEventListener("keyup", listener as any)
    }

    addTextListener(listener: (event: TextEvent) => void) {
        this.eventTarget.addEventListener("ml-text", listener as any)
    }

    private clearRefocusTimer() {
        if (this.refocusTimer != null) {
            window.clearTimeout(this.refocusTimer)
            this.refocusTimer = null
        }
    }

    private clearBlurDecisionTimer() {
        if (this.blurDecisionTimer != null) {
            window.clearTimeout(this.blurDecisionTimer)
            this.blurDecisionTimer = null
        }
    }

    private emitVisibilityChange() {
        const customEvent: ScreenKeyboardVisibilityEvent = new CustomEvent("ml-screen-keyboard-visibility", {
            detail: { visible: this.visible }
        })
        this.eventTarget.dispatchEvent(customEvent)
    }

    private ensureMounted() {
        if (this.fakeElement.isConnected) {
            if (!this.dismissSink.isConnected) {
                if (document.body) {
                    document.body.appendChild(this.dismissSink)
                } else {
                    document.documentElement.appendChild(this.dismissSink)
                }
            }
            return
        }

        if (document.body) {
            document.body.appendChild(this.fakeElement)
            document.body.appendChild(this.dismissSink)
        } else {
            document.documentElement.appendChild(this.fakeElement)
            document.documentElement.appendChild(this.dismissSink)
        }
    }

    private isStreamInteractionTarget(target: EventTarget | null): target is Element {
        if (!(target instanceof Element)) {
            return false
        }

        if (target === this.fakeElement) {
            return false
        }

        const overlaySelector = [
            "#sidebar-root",
            "#modal-overlay",
            ".modal-content",
            ".android-bridge-root",
            ".screen-keyboard",
            "button",
            "input",
            "textarea",
            "select",
            "summary",
            "a",
            "[role='button']"
        ].join(", ")

        if (target.closest(overlaySelector)) {
            return false
        }

        return !!target.closest("#input, #root")
    }

    private getVisibleViewportHeight(): number {
        const viewportHeight = window.visualViewport?.height
        if (viewportHeight != null && Number.isFinite(viewportHeight) && viewportHeight > 0) {
            return viewportHeight
        }

        return Math.max(window.innerHeight || 0, document.documentElement.clientHeight || 0)
    }

    private getVirtualKeyboardHeight(): number {
        const height = Number(this.virtualKeyboardApi?.boundingRect?.height ?? 0)
        return Number.isFinite(height) && height > 0 ? height : 0
    }

    private updateViewportMetrics(): number {
        const viewportHeight = this.getVisibleViewportHeight()
        this.maxViewportHeightSeen = Math.max(this.maxViewportHeightSeen, viewportHeight)
        return viewportHeight
    }

    private focusInput() {
        try {
            this.fakeElement.focus({ preventScroll: true })
        } catch {
            this.fakeElement.focus()
        }

        const end = this.fakeElement.value.length
        try {
            this.fakeElement.setSelectionRange(end, end)
        } catch {
            // Some browsers may reject selection updates on hidden inputs.
        }
    }

    private requestVirtualKeyboardShow() {
        try {
            if (this.virtualKeyboardApi && typeof this.virtualKeyboardApi.show == "function") {
                this.virtualKeyboardApi.show()
            }
        } catch {
            // Ignore browsers that gate explicit virtual keyboard control.
        }
    }

    private requestVirtualKeyboardHide() {
        try {
            if (this.virtualKeyboardApi && typeof this.virtualKeyboardApi.hide == "function") {
                this.virtualKeyboardApi.hide()
            }
        } catch {
            // Ignore browsers that gate explicit virtual keyboard control.
        }
    }

    private parkFocusAwayFromInput() {
        this.ensureMounted()
        this.dismissSink.disabled = false
        try {
            this.dismissSink.focus({ preventScroll: true })
        } catch {
            this.dismissSink.focus()
        }
        window.setTimeout(() => {
            if (document.activeElement === this.dismissSink) {
                this.dismissSink.blur()
            }
            this.dismissSink.disabled = true
        }, 220)
    }

    private applyHiddenState(blurInput: boolean, explicitClose = false) {
        const wasVisible = this.visible
        this.visible = false
        this.persistent = false
        this.suppressDocumentHideUntil = 0
        this.suppressBlurUntil = 0
        this.persistentViewportWasCompressed = false
        this.virtualKeyboardWasVisible = false
        this.clearBlurDecisionTimer()
        this.clearRefocusTimer()
        if (explicitClose) {
            this.isKeyboardExplicitlyClosed = true
        }
        try {
            this.fakeElement.disabled = false
            this.fakeElement.readOnly = false
            this.fakeElement.inputMode = "text"
            if (document.activeElement !== this.fakeElement) {
                this.focusInput()
            }
        } catch {
            // Ignore browsers that reject focus juggling before hide.
        }
        this.requestVirtualKeyboardHide()
        this.fakeElement.readOnly = true
        this.fakeElement.inputMode = "none"
        this.parkFocusAwayFromInput()
        if (blurInput) {
            this.fakeElement.blur()
        }
        window.setTimeout(() => {
            if (!this.visible) {
                this.fakeElement.disabled = true
            }
        }, 120)
        if (wasVisible) {
            this.emitVisibilityChange()
        }
    }

    private scheduleRefocus() {
        this.clearBlurDecisionTimer()
        this.clearRefocusTimer()
        if (!this.visible || !this.persistent) {
            return
        }

        this.refocusTimer = window.setTimeout(() => {
            this.refocusTimer = null
            if (!this.visible || !this.persistent || document.visibilityState == "hidden") {
                return
            }

            this.ensureMounted()
            this.fakeElement.readOnly = false
            this.fakeElement.inputMode = "text"
            this.focusInput()
            this.requestVirtualKeyboardShow()
        }, 48)
    }

    private schedulePersistentBlurDecision() {
        this.clearBlurDecisionTimer()
        this.blurDecisionTimer = window.setTimeout(() => {
            this.blurDecisionTimer = null
            this.evaluatePersistentBlur()
        }, 48)
    }

    private evaluatePersistentBlur() {
        if (!this.visible || !this.persistent) {
            return
        }

        if (Date.now() < this.suppressBlurUntil) {
            this.scheduleRefocus()
            return
        }

        const recentStreamPointerDown = (Date.now() - this.lastStreamPointerDownAt) <= 260
        if (recentStreamPointerDown) {
            this.scheduleRefocus()
            return
        }

        // Treat blur without a fresh stream tap as an explicit Android/system dismiss.
        this.applyHiddenState(false, true)
    }

    private onVirtualKeyboardGeometryChange() {
        const height = this.getVirtualKeyboardHeight()
        if (height > 0) {
            this.virtualKeyboardWasVisible = true
            return
        }

        if (!this.visible || !this.persistent || !this.virtualKeyboardWasVisible) {
            return
        }

        const recentStreamPointerDown = (Date.now() - this.lastStreamPointerDownAt) <= 260
        if (recentStreamPointerDown) {
            this.scheduleRefocus()
            return
        }

        this.applyHiddenState(false, true)
    }

    private onViewportMaybeChanged() {
        const viewportHeight = this.updateViewportMetrics()
        if (!this.visible || !this.persistent) {
            return
        }

        const referenceHeight = this.maxViewportHeightSeen
        if (referenceHeight <= 0) {
            return
        }

        if (viewportHeight <= (referenceHeight - 72)) {
            this.persistentViewportWasCompressed = true
            return
        }

        if (!this.persistentViewportWasCompressed) {
            return
        }

        const recentStreamPointerDown = (Date.now() - this.lastStreamPointerDownAt) <= 240
        if (!recentStreamPointerDown && viewportHeight >= (referenceHeight - 28)) {
            this.applyHiddenState(false, true)
        }
    }

    private resetInputBuffer() {
        this.fakeElement.value = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        const end = this.fakeElement.value.length
        try {
            this.fakeElement.setSelectionRange(end, end)
        } catch {
            // Ignore selection failures on browsers that restrict it before focus.
        }
    }

    private onDocumentPointerDown(event: PointerEvent) {
        const target = event.target
        if (this.isStreamInteractionTarget(target)) {
            this.lastStreamPointerDownAt = Date.now()
        }

        if (!this.visible) {
            return
        }

        if (target === this.fakeElement) {
            return
        }

        if (this.persistent) {
            return
        }

        if (Date.now() < this.suppressDocumentHideUntil) {
            return
        }

        this.hide(true)
    }

    private onInputBlur() {
        if (!this.visible) {
            return
        }

        if (this.persistent) {
            if (Date.now() < this.suppressBlurUntil) {
                this.scheduleRefocus()
                return
            }
            this.schedulePersistentBlurDecision()
            return
        }

        this.hide(true)
    }

    // -- Events
    private onKeyInput(event: Event) {
        if (!(event instanceof InputEvent)) {
            return
        }
        if (event.isComposing) {
            return
        }

        if ((event.inputType == "insertText" || event.inputType == "insertFromPaste") && event.data != null) {
            const customEvent: TextEvent = new CustomEvent("ml-text", {
                detail: { text: event.data }
            })

            this.eventTarget.dispatchEvent(customEvent)
        } else if (event.inputType == "deleteContentBackward" || event.inputType == "deleteByCut") {
            const keyDown = new KeyboardEvent("keydown", {
                code: "Backspace"
            })
            const keyUp = new KeyboardEvent("keyup", {
                code: "Backspace"
            })

            this.eventTarget.dispatchEvent(keyDown)
            this.eventTarget.dispatchEvent(keyUp)
        } else if (event.inputType == "deleteContentForward") {
            const keyDown = new KeyboardEvent("keydown", {
                code: "Delete"
            })
            const keyUp = new KeyboardEvent("keyup", {
                code: "Delete"
            })

            this.eventTarget.dispatchEvent(keyDown)
            this.eventTarget.dispatchEvent(keyUp)
        }

        // Repopulate the input so that the deleteContent commands will work
        this.resetInputBuffer()
    }
}
