// Minimal libretro frontend for the FBNeo Emscripten cores (emulator/build.sh).
// Runs wherever the core runs: the Web Worker (worker.js) or Node.

// https://github.com/libretro/RetroArch/blob/master/libretro-common/include/libretro.h
const ENV = {
  SET_ROTATION: 1,
  GET_CAN_DUPE: 3,
  GET_SYSTEM_DIRECTORY: 9,
  SET_PIXEL_FORMAT: 10,
  SET_INPUT_DESCRIPTORS: 11,
  GET_VARIABLE: 15,
  GET_VARIABLE_UPDATE: 17,
  GET_LOG_INTERFACE: 27,
  GET_SAVE_DIRECTORY: 31,
  GET_CORE_OPTIONS_VERSION: 52,
  GET_AUDIO_VIDEO_ENABLE: 47 | 0x10000,
  GET_SAVESTATE_CONTEXT: 72 | 0x10000,
};
const SAVESTATE_CONTEXT = { NORMAL: 0, ROLLBACK_NETPLAY: 3 };
const MEMORY_SYSTEM_RAM = 2;

// Core options we override; the rest keep FBNeo's defaults.
const OPTIONS = {
  // The default opens the service menu when Start is held for a second, counting frames outside
  // the savestate, so a rollback could open it on one player's machine only.
  "fbneo-diagnostic-input": "None",
};
const PIXEL_FORMAT = { RGB1555: 0, XRGB8888: 1, RGB565: 2 };
const DEVICE_JOYPAD = 1;

export class Core {
  /** Per-port RetroPad masks: bit (1 << id) per held button, ids from libretro.h. */
  inputs = new Uint16Array(4);
  /**
   * Rollback netplay: FBNeo then keeps its states free of anything machine-local
   * (hiscores, the host clock) so both players' machines stay identical.
   */
  netplay = false;
  /**
   * False while re-running frames after a rollback: FBNeo skips drawing and keeps the sound to
   * itself (it still emulates it, so the machine stays exact).
   */
  present = true;

  /**
   * What the game calls player 1's RetroPad buttons, from the core: RetroPad id -> name, e.g.
   * 0 -> "Fire" (B). Known once the game is loaded.
   */
  buttons = new Map();

  #m;
  #slots = [];
  #slotSize = 0;
  #pixelFormat = PIXEL_FORMAT.RGB1555;
  #rotation = 0; // quarter turns counter-clockwise, for vertical games like Pac-Man
  #onFrame;
  #onAudio;
  #onLog;
  #strings = new Map();

  /**
   * @param createModule the core's default export (createFBNeo from fbneo.mjs)
   * @param callbacks onFrame(rgba, width, height), onAudio(Int16Array stereo), onLog(level, text)
   */
  static async create(createModule, callbacks) {
    return new Core(await createModule(), callbacks);
  }

  constructor(module, { onFrame, onAudio, onLog = () => {} }) {
    const m = (this.#m = module);
    this.#onFrame = onFrame;
    this.#onAudio = onAudio;
    this.#onLog = onLog;
    m.FS.mkdirTree("/system");
    m.FS.mkdirTree("/save");
    m.FS.mkdirTree("/roms");

    m._retro_set_environment(m.addFunction((cmd, data) => this.#environment(cmd, data), "iii"));
    m._retro_set_video_refresh(m.addFunction((...args) => this.#video(...args), "viiii"));
    m._retro_set_audio_sample(m.addFunction((l, r) => this.#onAudio(Int16Array.of(l, r)), "vii"));
    m._retro_set_audio_sample_batch(m.addFunction((data, frames) => this.#audio(data, frames), "iii"));
    m._retro_set_input_poll(m.addFunction(() => {}, "v"));
    m._retro_set_input_state(m.addFunction((port, device, _index, id) =>
      device === DEVICE_JOYPAD ? (this.inputs[port] >> id) & 1 : 0, "iiiii"));
    m._retro_init();
  }

  /** Puts a file next to the ROMs, e.g. a BIOS set like neogeo.zip. */
  addFile(fileName, bytes) {
    this.#m.FS.writeFile(`/roms/${fileName}`, bytes);
  }

  /** Loads a ROM set (FBNeo picks the game from the file name, e.g. mk2.zip). */
  loadGame(fileName, bytes) {
    const m = this.#m;
    const path = `/roms/${fileName}`;
    this.addFile(fileName, bytes);
    // struct retro_game_info { const char *path; const void *data; size_t size; const char *meta; }
    const info = m._malloc(16);
    m.HEAPU32.fill(0, info >> 2, (info >> 2) + 4);
    m.setValue(info, this.#cString(path), "i32");
    const loaded = m._retro_load_game(info);
    m._free(info);
    if (!loaded) throw new Error(`The core could not load ${fileName}`);
    for (let port = 0; port < this.inputs.length; port++) {
      m._retro_set_controller_port_device(port, DEVICE_JOYPAD);
    }

    // struct retro_system_av_info { geometry { u32 w, h, max_w, max_h; f32 aspect } timing { f64 fps, sample_rate } }
    const av = m._malloc(40);
    m._retro_get_system_av_info(av);
    const result = {
      width: m.getValue(av, "i32"),
      height: m.getValue(av + 4, "i32"),
      aspectRatio: m.getValue(av + 16, "float"),
      fps: m.getValue(av + 24, "double"),
      sampleRate: m.getValue(av + 32, "double"),
    };
    m._free(av);
    return result;
  }

  /** Runs one emulated frame. */
  run() {
    this.#m._retro_run();
  }

  /** Snapshot of the whole machine. Only loads into the same core build and game. */
  serialize() {
    const m = this.#m;
    const size = m._retro_serialize_size();
    const ptr = m._malloc(size);
    const ok = m._retro_serialize(ptr, size);
    const state = m.HEAPU8.slice(ptr, ptr + size);
    m._free(ptr);
    if (!ok) throw new Error("The core could not save its state");
    return state;
  }

  unserialize(state) {
    const m = this.#m;
    const ptr = m._malloc(state.length);
    m.HEAPU8.set(state, ptr);
    const ok = m._retro_unserialize(ptr, state.length);
    m._free(ptr);
    if (!ok) throw new Error("The core could not load the state");
  }

  /**
   * Allocates `count` save-state slots inside the core's memory. Rollback saves every frame,
   * so saveSlot / loadSlot copy within wasm memory and never into JavaScript.
   */
  allocSlots(count) {
    const m = this.#m;
    this.#slots.forEach((ptr) => m._free(ptr));
    this.#slotSize = m._retro_serialize_size();
    this.#slots = Array.from({ length: count }, () => m._malloc(this.#slotSize));
  }

  saveSlot(slot) {
    if (!this.#m._retro_serialize(this.#slots[slot], this.#slotSize)) {
      throw new Error("The core could not save its state");
    }
  }

  loadSlot(slot) {
    if (!this.#m._retro_unserialize(this.#slots[slot], this.#slotSize)) {
      throw new Error("The core could not load the state");
    }
  }

  /**
   * The game's main RAM (what RetroAchievements reads). Both players' copies match while in
   * sync, unlike whole savestates, which also hold sound and drawing caches.
   * A view valid until the next allocation in the core.
   */
  systemRam() {
    const m = this.#m;
    const ptr = m._retro_get_memory_data(MEMORY_SYSTEM_RAM);
    return m.HEAPU8.subarray(ptr, ptr + m._retro_get_memory_size(MEMORY_SYSTEM_RAM));
  }

  /** A view of a slot's bytes, valid until the next allocation in the core. */
  slotBytes(slot) {
    const ptr = this.#slots[slot];
    return this.#m.HEAPU8.subarray(ptr, ptr + this.#slotSize);
  }

  #environment(cmd, data) {
    const m = this.#m;
    switch (cmd) {
      case ENV.SET_ROTATION:
        this.#rotation = m.getValue(data, "i32") & 3;
        return 1;
      case ENV.GET_CAN_DUPE:
        m.setValue(data, 1, "i8");
        return 1;
      case ENV.GET_SYSTEM_DIRECTORY:
        m.setValue(data, this.#cString("/system"), "i32");
        return 1;
      case ENV.GET_SAVE_DIRECTORY:
        m.setValue(data, this.#cString("/save"), "i32");
        return 1;
      case ENV.SET_INPUT_DESCRIPTORS: {
        // struct retro_input_descriptor { unsigned port, device, index, id; const char *description; }[],
        // ended by one without a description.
        this.buttons.clear();
        for (let at = data; ; at += 20) {
          const description = m.getValue(at + 16, "i32");
          if (!description) break;
          const [port, device, index, id] = [0, 4, 8, 12].map((offset) => m.getValue(at + offset, "i32"));
          if (port === 0 && device === DEVICE_JOYPAD && index === 0) this.buttons.set(id, m.UTF8ToString(description));
        }
        return 1;
      }
      case ENV.SET_PIXEL_FORMAT: {
        const format = m.getValue(data, "i32");
        if (!Object.values(PIXEL_FORMAT).includes(format)) return 0;
        this.#pixelFormat = format;
        return 1;
      }
      case ENV.GET_LOG_INTERFACE:
        m.setValue(data, m.addFunction((level, fmt, args) =>
          this.#onLog(level, this.#format(fmt, args)), "viii"), "i32");
        return 1;
      case ENV.GET_CORE_OPTIONS_VERSION:
        // Version 0: the core sets plain variables and asks for each (GET_VARIABLE below).
        m.setValue(data, 0, "i32");
        return 1;
      case ENV.GET_VARIABLE: {
        // struct retro_variable { const char *key; const char *value; }
        const value = OPTIONS[m.UTF8ToString(m.getValue(data, "i32"))];
        if (value === undefined) return 0;
        m.setValue(data + 4, this.#cString(value), "i32");
        return 1;
      }
      case ENV.GET_VARIABLE_UPDATE:
        m.setValue(data, 0, "i8");
        return 1;
      case ENV.GET_AUDIO_VIDEO_ENABLE:
        m.setValue(data, this.present ? 1 | 2 : 0, "i32"); // bit 0 video, bit 1 audio
        return 1;
      case ENV.GET_SAVESTATE_CONTEXT:
        // The core first asks with no pointer, only to learn that we answer.
        if (data) {
          const context = this.netplay ? SAVESTATE_CONTEXT.ROLLBACK_NETPLAY : SAVESTATE_CONTEXT.NORMAL;
          m.setValue(data, context, "i32");
        }
        return 1;
      default:
        return 0;
    }
  }

  #video(data, width, height, pitch) {
    if (!data) return; // duplicated frame
    const heap = this.#m.HEAPU8;
    const rgba = new Uint8ClampedArray(width * height * 4);
    const out = new Uint32Array(rgba.buffer);
    for (let y = 0; y < height; y++) {
      const row = data + y * pitch;
      const o = y * width;
      if (this.#pixelFormat === PIXEL_FORMAT.XRGB8888) {
        const src = new Uint32Array(heap.buffer, row, width);
        for (let x = 0; x < width; x++) {
          const v = src[x];
          out[o + x] = 0xff000000 | ((v & 0xff) << 16) | (v & 0xff00) | ((v >> 16) & 0xff);
        }
      } else {
        const src = new Uint16Array(heap.buffer, row, width);
        const is565 = this.#pixelFormat === PIXEL_FORMAT.RGB565;
        for (let x = 0; x < width; x++) {
          const v = src[x];
          const r = (v >> (is565 ? 11 : 10)) & 0x1f;
          const g = is565 ? (v >> 5) & 0x3f : (v >> 5) & 0x1f;
          const b = v & 0x1f;
          const g8 = is565 ? (g << 2) | (g >> 4) : (g << 3) | (g >> 2);
          out[o + x] = 0xff000000 | (((b << 3) | (b >> 2)) << 16) | (g8 << 8) | ((r << 3) | (r >> 2));
        }
      }
    }
    if (this.#rotation === 0) {
      this.#onFrame(rgba, width, height);
    } else {
      const turned = rotate(out, width, height, this.#rotation);
      const sideways = this.#rotation % 2 === 1;
      this.#onFrame(new Uint8ClampedArray(turned.buffer), sideways ? height : width, sideways ? width : height);
    }
  }

  #audio(data, frames) {
    this.#onAudio(this.#m.HEAP16.slice(data >> 1, (data >> 1) + frames * 2));
    return frames;
  }

  // Strings handed to the core must stay alive, so each one is allocated once.
  #cString(text) {
    let ptr = this.#strings.get(text);
    if (!ptr) {
      const size = this.#m.lengthBytesUTF8(text) + 1;
      ptr = this.#m._malloc(size);
      this.#m.stringToUTF8(text, ptr, size);
      this.#strings.set(text, ptr);
    }
    return ptr;
  }

  // Enough printf for the core's log lines (%s %d %i %u %x %c %f); wasm32 passes varargs
  // as a pointer to a buffer with each argument aligned to its size.
  #format(fmtPtr, argsPtr) {
    const m = this.#m;
    let offset = argsPtr;
    const next = (size) => {
      offset = Math.ceil(offset / size) * size;
      const at = offset;
      offset += size;
      return at;
    };
    return m.UTF8ToString(fmtPtr).replace(/%[-+ #0-9.]*(l{0,2}|z|h{0,2})([sdiuxXcf%])/g, (_, len, type) => {
      if (type === "%") return "%";
      if (type === "f") return m.getValue(next(8), "double").toFixed(2);
      if (len === "ll") return String(m.getValue(next(8), "i64"));
      const value = m.getValue(next(4), "i32");
      if (type === "s") return m.UTF8ToString(value);
      if (type === "c") return String.fromCharCode(value);
      if (type === "u") return String(value >>> 0);
      if (type === "x" || type === "X") return (value >>> 0).toString(16);
      return String(value);
    }).trimEnd();
  }
}

/** Turns a width x height image `quarterTurns` times counter-clockwise. */
function rotate(pixels, width, height, quarterTurns) {
  const out = new Uint32Array(pixels.length);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const pixel = pixels[y * width + x];
      if (quarterTurns === 1) out[(width - 1 - x) * height + y] = pixel;
      else if (quarterTurns === 2) out[(height - 1 - y) * width + (width - 1 - x)] = pixel;
      else out[x * height + (height - 1 - y)] = pixel;
    }
  }
  return out;
}
