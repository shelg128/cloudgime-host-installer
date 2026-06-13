import { globalObject } from "../../util.js";
import { Logger } from "../log.js";
import { PipeInfo } from "../pipeline/index.js";
import { AudioContextBasePipe } from "./audio_context_base.js";
import { AudioPlayer, AudioPlayerSetup } from "./index.js";

export class ContextDestinationNodeAudioPlayer extends AudioContextBasePipe implements AudioPlayer {

    static async getInfo(): Promise<PipeInfo> {
        return {
            environmentSupported: "AudioContext" in globalObject()
        }
    }

    static readonly type = "audionode"

    private destination: AudioNode | null = null
    private currentSource: AudioNode | null = null
    private gainNode: GainNode | null = null
    private muted = false

    constructor(logger?: Logger) {
        super("node_audio_element", null, logger)

        this.addPipePassthrough()
    }

    setup(setup: AudioPlayerSetup) {
        const result = super.setup(setup)

        this.destination = this.getAudioContext().destination;
        this.gainNode = this.getAudioContext().createGain()
        this.gainNode.gain.value = this.muted ? 0 : 1
        this.gainNode.connect(this.destination)

        if (this.currentSource) {
            this.currentSource.connect(this.gainNode)
        }

        return result
    }

    cleanup(): void {
        if (this.currentSource && this.gainNode) {
            this.currentSource.disconnect(this.gainNode)
        }
        if (this.gainNode && this.destination) {
            this.gainNode.disconnect(this.destination)
        }

        this.currentSource = null
        this.gainNode = null
        this.destination = null

        super.cleanup()
    }

    setSource(source: AudioNode): void {
        if (this.currentSource && this.gainNode) {
            this.currentSource.disconnect(this.gainNode)
        }

        this.currentSource = source

        if (this.gainNode) {
            source.connect(this.gainNode)
        }
    }

    setMuted(muted: boolean): void {
        this.muted = muted

        if (this.gainNode) {
            this.gainNode.gain.value = muted ? 0 : 1
        }
    }

    isMuted(): boolean {
        return this.muted
    }

    mount(_parent: HTMLElement): void { }
    unmount(_parent: HTMLElement): void { }

}
