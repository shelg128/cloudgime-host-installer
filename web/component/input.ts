import { Component, ComponentEvent } from "./index.js"
import { getLocalStreamSettings } from "./settings_menu.js"

let helpModeActive = false;
const activeTooltipInstances: Set<ElementWithLabel> = new Set();
const helpCheckboxes: Set<HTMLInputElement> = new Set();

export function setHelpModeActive(active: boolean) {
    helpModeActive = active;
    if (typeof document !== "undefined") {
        if (active) {
            document.body.classList.add("help-mode-active");
        } else {
            document.body.classList.remove("help-mode-active");
        }
    }
    for (const cb of helpCheckboxes) {
        cb.checked = active;
    }
    for (const instance of activeTooltipInstances) {
        instance.updateHelpModeState();
    }
}

export function registerHelpCheckbox(cb: HTMLInputElement) {
    helpCheckboxes.add(cb);
    cb.checked = helpModeActive;
}

export function unregisterHelpCheckbox(cb: HTMLInputElement) {
    helpCheckboxes.delete(cb);
}

export function getHelpModeActive(): boolean {
    return helpModeActive;
}

export function hideAllTooltips() {
    for (const instance of activeTooltipInstances) {
        instance.hideTooltip();
    }
    if (typeof document !== "undefined") {
        document.querySelectorAll(".help-tooltip-container").forEach(el => {
            el.classList.remove("visible");
            setTimeout(() => el.remove(), 150);
        });
    }
}

export class ElementWithLabel implements Component {
    protected div: HTMLDivElement = document.createElement("div")
    protected label: HTMLLabelElement = document.createElement("label")
    protected helpTextEn: string | null = null
    protected helpTextId: string | null = null
    private tooltipElement: HTMLDivElement | null = null

    constructor(internalName: string, displayName?: string) {
        if (displayName) {
            this.label.htmlFor = internalName
            this.label.innerText = displayName
            this.div.appendChild(this.label)
        }
    }

    setHelpText(en: string, id: string) {
        this.helpTextEn = en;
        this.helpTextId = id;
        activeTooltipInstances.add(this);
        this.updateHelpModeState();
    }

    updateHelpModeState() {
        if (!this.helpTextEn) return;
        
        if (helpModeActive) {
            this.label.classList.add("help-indicator");
            this.bindHelpEvents();
        } else {
            this.label.classList.remove("help-indicator");
            this.unbindHelpEvents();
            this.destroyTooltip();
        }
    }

    private enterListener = (e: MouseEvent) => this.showTooltip();
    private leaveListener = (e: MouseEvent) => this.hideTooltip();
    private clickListener = (e: Event) => {
        if (!helpModeActive) return;
        e.stopPropagation();
        if (this.tooltipElement) {
            this.hideTooltip();
        } else {
            hideAllTooltips();
            this.showTooltip();
        }
    };

    private eventsBound = false;

    private bindHelpEvents() {
        if (this.eventsBound) return;
        this.eventsBound = true;
        this.label.addEventListener("mouseenter", this.enterListener);
        this.label.addEventListener("mouseleave", this.leaveListener);
        this.label.addEventListener("click", this.clickListener);
    }

    private unbindHelpEvents() {
        if (!this.eventsBound) return;
        this.eventsBound = false;
        this.label.removeEventListener("mouseenter", this.enterListener);
        this.label.removeEventListener("mouseleave", this.leaveListener);
        this.label.removeEventListener("click", this.clickListener);
    }

    showTooltip() {
        if (!helpModeActive || !this.helpTextEn) return;
        if (this.tooltipElement) return;

        let uiLanguage = "en";
        try {
            uiLanguage = getLocalStreamSettings()?.uiLanguage ?? "en";
        } catch {
            // fallback
        }
        const text = uiLanguage === "id" ? this.helpTextId : this.helpTextEn;
        if (!text) return;

        const tooltip = document.createElement("div");
        tooltip.classList.add("help-tooltip-container");
        tooltip.innerText = text;
        document.body.appendChild(tooltip);
        this.tooltipElement = tooltip;

        const rect = this.label.getBoundingClientRect();
        const tooltipRect = tooltip.getBoundingClientRect();
        
        let left = rect.left + (rect.width - tooltipRect.width) / 2;
        left = Math.max(10, Math.min(window.innerWidth - tooltipRect.width - 10, left));

        let top = rect.top - tooltipRect.height - 8 + window.scrollY;
        if (rect.top - tooltipRect.height - 8 < 10) {
            top = rect.bottom + 8 + window.scrollY;
        }

        tooltip.style.left = `${left}px`;
        tooltip.style.top = `${top}px`;

        requestAnimationFrame(() => {
            tooltip.classList.add("visible");
        });
    }

    hideTooltip() {
        if (!this.tooltipElement) return;
        const el = this.tooltipElement;
        this.tooltipElement = null;
        el.classList.remove("visible");
        setTimeout(() => {
            el.remove();
        }, 150);
    }

    destroyTooltip() {
        if (this.tooltipElement) {
            this.tooltipElement.remove();
            this.tooltipElement = null;
        }
    }

    mount(parent: HTMLElement): void {
        parent.appendChild(this.div)
    }
    unmount(parent: HTMLElement): void {
        parent.removeChild(this.div)
    }
}

export type InputInit = {
    defaultValue?: string
    value?: string
    checked?: boolean
    step?: string
    accept?: string
    inputMode?: string
    hasEnableCheckbox?: boolean
    placeholer?: string
    formRequired?: boolean
    // Only allowed with type == "number"
    numberSlider?: {
        range_min: number,
        range_max: number
        // Use step to set the step
    }
}

export type InputChangeListener = (event: ComponentEvent<InputComponent>) => void

export class InputComponent extends ElementWithLabel {

    private fileLabel: HTMLDivElement | null = null
    private numberSlider: HTMLInputElement | null = null

    private inputEnabled: HTMLInputElement | null = null
    private input: HTMLInputElement = document.createElement("input")

    constructor(internalName: string, type: string, displayName?: string, init?: InputInit) {
        super(internalName, displayName)

        this.div.classList.add("input-div")

        this.input.id = internalName
        this.input.type = type
        if (init?.defaultValue != null) {
            this.input.defaultValue = init.defaultValue
        }
        if (init?.value != null) {
            this.input.value = init.value
        }
        if (init && init.checked != null) {
            this.input.checked = init.checked
        }
        if (init && init.step != null) {
            this.input.step = init.step
        }
        if (init && init.accept != null) {
            this.input.accept = init.accept
        }
        if (init && init.inputMode != null) {
            this.input.inputMode = init.inputMode
        }
        if (init && init.formRequired != null) {
            this.input.required = init.formRequired
        }
        if (init && init.placeholer != null) {
            this.input.placeholder = init.placeholer
        }

        if (type == "file") {
            this.fileLabel = document.createElement("div")
            this.fileLabel.innerText = this.label.innerText
            this.fileLabel.classList.add("file-label")

            this.label.innerText = "Open File"
            this.label.classList.add("file-button")

            this.div.insertBefore(this.fileLabel, this.label)
        }

        if (init?.hasEnableCheckbox) {
            this.inputEnabled = document.createElement("input")
            this.inputEnabled.type = "checkbox"
            this.inputEnabled.defaultChecked = false

            this.inputEnabled.addEventListener("change", () => {
                this.setEnabled(
                    this.inputEnabled?.checked ?? (() => { throw "inputEnabled is null" })()
                )

                this.div.dispatchEvent(new ComponentEvent("ml-change", this))
            })

            this.div.appendChild(this.inputEnabled)
        }

        this.div.appendChild(this.input)

        this.input.addEventListener("change", () => {
            if (this.numberSlider) {
                this.numberSlider.value = this.input.value
            }

            this.div.dispatchEvent(new ComponentEvent("ml-change", this))
        })

        if (init?.numberSlider && type != "number") {
            throw "tried to create InputComponent with number slider but type wasn't number"
        }
        if (type == "number" && init?.numberSlider) {
            this.numberSlider = document.createElement("input")
            this.numberSlider.type = "range"
            this.numberSlider.min = `${init.numberSlider.range_min}`
            this.numberSlider.max = `${init.numberSlider.range_max}`
            this.numberSlider.step = init.step?.toString() ?? ""

            this.numberSlider.addEventListener("change", () => {
                if (this.numberSlider) {
                    this.input.value = this.numberSlider.value
                } else {
                    throw "failed to get value of number slider because it wasn't created"
                }

                this.div.dispatchEvent(new ComponentEvent("ml-change", this))
            })

            this.div.appendChild(this.numberSlider)
        }

        if (init?.hasEnableCheckbox) {
            // The main logic is further up
            this.setEnabled(false)
        }
    }

    reset() {
        this.input.value = ""
        if (this.numberSlider) {
            this.numberSlider.value = ""
        }
    }

    setValue(value: string) {
        this.input.value = value
        if (this.numberSlider) {
            this.numberSlider.value = value
        }
    }
    getValue(): string {
        return this.input.value
    }

    isChecked(): boolean {
        return this.input.checked
    }

    getFiles(): FileList | null {
        return this.input.files
    }

    setEnabled(enabled: boolean) {
        if (this.inputEnabled) {
            this.inputEnabled.checked = enabled
        }

        this.input.disabled = !enabled
        if (this.numberSlider) {
            this.numberSlider.disabled = !enabled
        }
    }
    isEnabled(): boolean {
        return !this.input.disabled
    }

    addChangeListener(listener: InputChangeListener, options?: AddEventListenerOptions) {
        this.div.addEventListener("ml-change", listener as any, options)
    }
    removeChangeListener(listener: InputChangeListener) {
        this.div.removeEventListener("ml-change", listener as any)
    }

    setPlaceholder(newPlaceholder: string) {
        this.input.placeholder = newPlaceholder
    }

    mount(parent: HTMLElement): void {
        super.mount(parent)

        if (this.numberSlider) {
            this.numberSlider.value = this.input.value
        }
    }
}

export type SelectInit = {
    // Only uses datalist if supported
    hasSearch?: boolean
    preSelectedOption?: string
    displayName?: string,
}

type SelectStrategy =
    { name: "select", optionRoot: HTMLSelectElement } |
    { name: "datalist", optionRoot: HTMLDataListElement, inputElement: HTMLInputElement } |
    { name: "polyfill", opened: boolean, wrapper: HTMLDivElement, display: HTMLParagraphElement, list: HTMLDivElement, value: string | null, disabled: Set<string> }

function useSelectElementPolyfill(): boolean {
    return getLocalStreamSettings()?.useSelectElementPolyfill ?? false
}

export class SelectComponent extends ElementWithLabel {

    private preSelectedOption: string = ""
    private options: Array<{ value: string, name: string }>

    private strategy: SelectStrategy

    constructor(internalName: string, options: Array<{ value: string, name: string }>, init?: SelectInit) {
        super(internalName, init?.displayName)

        if (init && init.preSelectedOption) {
            this.preSelectedOption = init.preSelectedOption
        }
        this.options = options

        // Create base
        if (useSelectElementPolyfill() || !isElementSupported("select")) {
            const wrapper = document.createElement("div")

            wrapper.classList.add("select-polyfill-wrapper")

            this.div.appendChild(wrapper)
            this.div.classList.add("input-div")

            const display = document.createElement("p")
            display.classList.add("select-polyfill-display")

            display.addEventListener("click", () => {
                if (this.strategy.name != "polyfill") {
                    throw "SelectComponent strategy is not polyfill"
                }

                this.setStrategyPolyfillOpened(!this.strategy.opened)
            })

            const list = document.createElement("div")

            list.classList.add("select-polyfill-list")

            wrapper.appendChild(display)

            this.strategy = {
                name: "polyfill",
                opened: false,
                wrapper,
                display,
                list,
                value: init?.preSelectedOption ?? "",
                disabled: new Set()
            }
        } else if (init && init.hasSearch && isElementSupported("datalist")) {
            const dataListElement = document.createElement("datalist")

            dataListElement.id = `${internalName}-list`

            const inputElement = document.createElement("input")
            inputElement.type = "text"
            inputElement.id = internalName
            inputElement.setAttribute("list", dataListElement.id)

            if (init && init.preSelectedOption) {
                inputElement.defaultValue = init.preSelectedOption
            }

            this.div.appendChild(inputElement)
            this.div.appendChild(dataListElement)

            this.strategy = {
                name: "datalist",
                optionRoot: dataListElement,
                inputElement,
            }
        } else {
            const selectElement = document.createElement("select")
            selectElement.id = internalName

            this.div.appendChild(selectElement)

            this.strategy = {
                name: "select",
                optionRoot: selectElement,
            }
        }

        // Append values
        if (this.strategy.name == "datalist" || this.strategy.name == "select") {
            const optionRoot = this.strategy.optionRoot

            for (const option of options) {
                const optionElement = document.createElement("option")

                if (this.strategy.name == "datalist") {
                    optionElement.value = option.name
                } else if (this.strategy.name == "select") {
                    optionElement.innerText = option.name
                    optionElement.value = option.value
                }

                if (init && init.preSelectedOption == option.value) {
                    optionElement.selected = true
                }

                optionRoot.appendChild(optionElement)
            }

            optionRoot.addEventListener("change", () => {
                this.dispatchChange()
            })
        } else if (this.strategy.name == "polyfill") {
            const optionRoot = this.strategy.list

            for (const option of options) {
                const optionElement = document.createElement("p")

                optionElement.innerText = option.name

                // @ts-ignore
                optionElement.value = option.value

                optionElement.addEventListener("click", () => {
                    if (this.strategy.name != "polyfill") {
                        throw "SelectComponent strategy is not polyfill even though it was initialized using polyfill strategy"
                    }

                    if (this.strategy.disabled.has(option.value)) {
                        return
                    }

                    this.strategy.value = option.value
                    this.setStrategyPolyfillOpened(false)

                    this.updateStrategyPolyfill()

                    this.dispatchChange()
                })

                optionRoot.appendChild(optionElement)
            }

            this.updateStrategyPolyfill()
        }
    }

    private dispatchChange() {
        this.div.dispatchEvent(new ComponentEvent("ml-change", this))
    }

    reset() {
        if (this.strategy.name == "datalist") {
            const inputElement = this.strategy.inputElement

            inputElement.value = ""
        } else if (this.strategy.name == "select") {
            const selectElement = this.strategy.optionRoot

            selectElement.value = this.preSelectedOption
        } else if (this.strategy.name == "polyfill") {
            this.strategy.value = this.preSelectedOption

            this.updateStrategyPolyfill()
        }
    }

    getValue(): string | null {
        if (this.strategy.name == "datalist") {
            const name = this.strategy.inputElement.value

            return this.options.find(option => option.name == name)?.value ?? ""
        } else if (this.strategy.name == "select") {
            const selectElement = this.strategy.optionRoot

            return selectElement.value
        } else if (this.strategy.name == "polyfill") {
            return this.strategy.value
        }

        throw "Invalid strategy for select input field"
    }

    setValue(value: string) {
        if (this.strategy.name == "datalist") {
            const option = this.options.find(option => option.value == value)
            this.strategy.inputElement.value = option?.name ?? value
        } else if (this.strategy.name == "select") {
            this.strategy.optionRoot.value = value
        } else if (this.strategy.name == "polyfill") {
            this.strategy.value = value
            this.updateStrategyPolyfill()
        }
    }

    setOptionEnabled(value: string, enabled: boolean) {
        if (this.strategy.name == "datalist" || this.strategy.name == "select") {
            const optionRoot = this.strategy.optionRoot

            for (const optionElement of optionRoot.options) {
                if (optionElement.value == value) {
                    optionElement.disabled = !enabled
                }
            }
        } else if (this.strategy.name == "polyfill") {
            const element = this.strategy.list

            for (const optionElement of element.children) {
                // @ts-ignore
                const elementValue = optionElement.value

                if (elementValue != value) {
                    continue
                }

                if (enabled) {
                    this.strategy.disabled.delete(value)

                    optionElement.classList.remove("select-polyfill-option-disabled")
                } else {
                    this.strategy.disabled.add(value)

                    optionElement.classList.add("select-polyfill-option-disabled")
                }
            }
        }
    }

    private updateStrategyPolyfill() {
        if (this.strategy.name != "polyfill") {
            throw "SelectComponent strategy is not polyfill"
        }

        for (const optionElement of this.strategy.list.children) {
            // @ts-ignore
            const value = optionElement.value

            if (value == this.strategy.value) {
                optionElement.classList.add("select-polyfill-selected")
            } else {
                optionElement.classList.remove("select-polyfill-selected")
            }
        }

        const value = this.strategy.value
        const selectedOption = this.options.find(option => option.value == value)

        this.strategy.display.innerText = selectedOption?.name ?? "(Not Selected)"
    }
    private setStrategyPolyfillOpened(opened: boolean) {
        if (this.strategy.name != "polyfill") {
            throw "SelectComponent strategy is not polyfill"
        }

        if (opened != this.strategy.opened) {
            if (opened) {
                const list = this.strategy.list

                this.strategy.wrapper.appendChild(this.strategy.list)

                if ("screenTop" in window && "innerHeight" in window) {
                    const displayRect = list.getBoundingClientRect()
                    const viewportBottom = window.screenTop + window.innerHeight

                    const spaceBelow = viewportBottom - displayRect.bottom

                    if (spaceBelow < 20) {
                        list.classList.add("top")
                    } else {
                        list.classList.add("bottom")
                    }
                } else {
                    list.classList.add("bottom")
                }
            } else {
                this.strategy.wrapper.removeChild(this.strategy.list)
                this.strategy.list.classList.remove("top")
                this.strategy.list.classList.remove("bottom")
            }
        }
        this.strategy.opened = opened
    }

    addChangeListener(listener: InputChangeListener, options?: AddEventListenerOptions) {
        this.div.addEventListener("ml-change", listener as any, options)
    }
    removeChangeListener(listener: InputChangeListener) {
        this.div.removeEventListener("ml-change", listener as any)
    }
}

export function isElementSupported(tag: string) {
    // Create a test element for the tag
    const element = document.createElement(tag);

    // Check for support of custom elements registered via
    // `document.registerElement`
    if (tag.indexOf('-') > -1) {
        // Registered elements have their own constructor, while unregistered
        // ones use the `HTMLElement` or `HTMLUnknownElement` (if invalid name)
        // constructor (http://stackoverflow.com/a/28210364/1070244)
        return (
            element.constructor !== window.HTMLUnknownElement &&
            element.constructor !== window.HTMLElement
        );
    }

    // Obtain the element's internal [[Class]] property, if it doesn't 
    // match the `HTMLUnknownElement` interface than it must be supported
    return toString.call(element) !== '[object HTMLUnknownElement]';
};

// Inject tooltip styles
(() => {
    if (typeof document === "undefined") return;
    const styleId = "help-mode-styles";
    if (document.getElementById(styleId)) return;

    const style = document.createElement("style");
    style.id = styleId;
    style.innerHTML = `
        .help-indicator, .help-mode-active .has-help-text {
            border-bottom: 1px dotted #3b82f6 !important;
            cursor: help !important;
        }
        .help-tooltip-container {
            position: absolute;
            z-index: 100000;
            max-width: 280px;
            padding: 8px 12px;
            background: rgba(15, 23, 42, 0.88);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            border: 1px solid rgba(255, 255, 255, 0.15);
            border-radius: 8px;
            box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.5), 0 8px 10px -6px rgba(0, 0, 0, 0.5);
            color: #f1f5f9;
            font-size: 0.82rem;
            line-height: 1.4;
            pointer-events: none;
            opacity: 0;
            transform: translateY(4px);
            transition: opacity 0.12s ease-out, transform 0.12s ease-out;
        }
        .help-tooltip-container.visible {
            opacity: 1;
            transform: translateY(0);
        }
        /* Help mode toggle switch alignment */
        .help-toggle-container {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 10px 14px;
            margin-bottom: 1rem;
            background: rgba(255, 255, 255, 0.03);
            border: 1px dashed rgba(255, 255, 255, 0.1);
            border-radius: 8px;
        }
        .help-toggle-label {
            font-size: 0.9rem;
            font-weight: 600;
            color: #3b82f6;
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 6px;
        }
    `;
    document.head.appendChild(style);

    // Global tap-to-dismiss listeners
    window.addEventListener("click", () => hideAllTooltips());
    window.addEventListener("touchstart", () => hideAllTooltips(), { passive: true });
    window.addEventListener("scroll", () => hideAllTooltips(), { passive: true });
    window.addEventListener("resize", () => hideAllTooltips(), { passive: true });
})();
