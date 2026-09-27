// AudioWorklet that plays the emulator's sound. 16-bit stereo chunks arrive from the worker
// on a MessagePort and go into a ~100 ms ring buffer that the audio thread drains.
class EmulatorAudio extends AudioWorkletProcessor {
  ring = new Float32Array(Math.ceil(sampleRate * 0.1) * 2);
  read = 0;
  size = 0;

  constructor() {
    super();
    // The page sends one MessagePort; the worker writes samples into its other end.
    this.port.onmessage = ({ data: workerPort }) => {
      workerPort.onmessage = ({ data: samples }) => this.push(samples);
    };
  }

  push(samples) {
    const ring = this.ring;
    for (let i = 0; i < samples.length; i++) {
      if (this.size === ring.length) {
        // Full: drop the oldest left/right pair to keep latency bounded.
        this.read = (this.read + 2) % ring.length;
        this.size -= 2;
      }
      ring[(this.read + this.size) % ring.length] = samples[i] / 32768;
      this.size++;
    }
  }

  process(_inputs, [[left, right]]) {
    const ring = this.ring;
    for (let i = 0; i < left.length; i++) {
      if (this.size < 2) {
        left[i] = right[i] = 0;
        continue;
      }
      left[i] = ring[this.read];
      right[i] = ring[(this.read + 1) % ring.length];
      this.read = (this.read + 2) % ring.length;
      this.size -= 2;
    }
    return true;
  }
}

registerProcessor("emulator-audio", EmulatorAudio);
