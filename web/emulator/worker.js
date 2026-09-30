// Runs an FBNeo core off the main thread for one cabinet. Posts each frame to the page (which
// hands it to Bevy) and streams audio to the AudioWorklet (audio.js).
//
// Alone, the local player's buttons drive their seat's controller port. Online, every seated
// player's machine runs the same game in step with rollback (netplay/src/lib.rs): GGRS guesses
// the others' input and re-runs frames once the real input arrives. Its packets go out and come
// in on `port` as [seat, bytes], and the page carries them between the players. Each starts
// with its session's epoch, so packets still in flight from an earlier session are dropped.
//
// When players join or leave, one machine (the page picks it) captures the game as it is and
// every player starts a new session from that capture.
//
// In:  { type: "start", core, rom, files, state, seat, turns, port, hold }
//        Loads the game. `files` (a BIOS) and `state` are optional: skipped if missing. Plays
//        alone right away, or with `hold` waits for "online" (joining a game in progress).
//      { type: "capture", epoch } Stops and captures the machine, for a change of players.
//      { type: "online", epoch, seats, state } Plays in step with the players in `seats` (their
//        seat numbers, ascending) from `state`, or from this machine's capture for `epoch`.
//      { type: "solo" } Everyone else left: play on alone.
//      { type: "input", mask } | { type: "audio", port }
// Out: { type: "frame", rgba, width, height } | { type: "netplay", event, seat, ... } |
//      { type: "captured", epoch, state }
import { Core } from "./libretro.js";

// RetroPad bits (libretro.h).
const SELECT = 1 << 2; // coin
const START = 1 << 3;

let audioPort;
let localMask = 0;
let cabinet;
const waiting = [];

// GGRS, loading while the game downloads.
let Session;
const netplay = import("../netplay/netplay.js").then(async (module) => {
  await module.default();
  Session = module.Session;
});

onmessage = ({ data: msg }) => {
  if (msg.type === "input") localMask = msg.mask;
  else if (msg.type === "audio") audioPort = msg.port;
  else if (msg.type === "start") start(msg);
  // Anything else is for the loaded game; it can arrive while the game still downloads.
  else if (cabinet) cabinet.handle(msg);
  else waiting.push(msg);
};

// "no-cache" checks with the server every time (a 304 when unchanged), so newly uploaded or
// replaced ROMs, BIOS sets and states are picked up.
const download = async (url) => {
  const response = await fetch(url, { cache: "no-cache" });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
};
const downloadIfPresent = (url) => url && download(url).catch(() => undefined);

async function start({ core: coreUrl, rom: romUrl, files = [], state: stateUrl, seat = 0, turns = false, port, hold }) {
  const { default: createFBNeo } = await import(coreUrl);
  const [rom, state, ...extras] = await Promise.all([
    download(romUrl),
    downloadIfPresent(stateUrl),
    ...files.map(downloadIfPresent),
  ]);
  const cab = new Cabinet(await Core.create(createFBNeo, {
    onFrame: (rgba, width, height) => cab.muted || (cab.frame = { type: "frame", rgba, width, height }),
    onAudio: (samples) => cab.muted || audioPort?.postMessage(samples, [samples.buffer]),
    onLog: (level, text) => level >= 2 && console.warn(text),
  }), { seat, turns, port });
  const core = cab.core;
  core.netplay = true;
  files.forEach((url, i) => extras[i] && core.addFile(url.split("/").pop(), extras[i]));
  cab.fps = core.loadGame(romUrl.split("/").pop(), rom).fps;
  // A start-up state (emulator/snapshot.mjs) skips the boot screens and adds credits. States
  // from an older core build don't load; the game then just boots normally.
  try {
    if (state) core.unserialize(state);
  } catch (error) {
    console.warn(`${stateUrl}: ${error.message}`);
  }
  cab.paused = Boolean(hold);
  await netplay;
  cabinet = cab;
  for (const msg of waiting.splice(0)) cab.handle(msg);
  cab.tick();
}

class Cabinet {
  /** The newest frame to post. */
  frame;
  /** True while frames run only to measure, not to show or hear. */
  muted = false;
  /** Not running frames: waiting to join a game, or for the others after a capture. */
  paused = false;
  fps = 60;

  #seat;
  #turns;
  #port;
  /** Online: the GGRS session, its epoch, and the seats of its players in handle order. */
  #session;
  #epoch;
  #seats;
  #tuned;
  /** This machine at the last change of players, until the next session starts from it. */
  #captured;
  #next = performance.now();
  #nextStats = 0;

  constructor(core, { seat, turns, port }) {
    this.core = core;
    this.#seat = seat;
    this.#turns = turns;
    this.#port = port;
    port.onmessage = ({ data: [seat, packet] }) => {
      const handle = this.#seats?.indexOf(seat) ?? -1;
      const bytes = new Uint8Array(packet);
      const epoch = bytes[0] | (bytes[1] << 8);
      if (handle >= 0 && epoch === this.#epoch) this.#session?.receive(handle, bytes.subarray(2));
    };
  }

  handle(msg) {
    if (msg.type === "capture") this.#capture(msg.epoch);
    if (msg.type === "online") this.#online(msg);
    if (msg.type === "solo") this.#leaveSession();
  }

  tick = () => {
    const session = this.#session;
    const frameMs = 1000 / this.fps;
    let wait = 0;
    if (session) {
      session.poll();
      for (const { type, player, ...fields } of session.events()) {
        postMessage({ type: "netplay", event: type, seat: this.#seats[player], ...fields });
      }
      if (session.running() && !this.paused) {
        for (let i = 0; i < 4 && performance.now() >= this.#next; i++) {
          // False: someone is too far behind to keep guessing. Try again shortly.
          if (!session.advance(localMask, this.#machine)) {
            wait = 2;
            break;
          }
          // A little slower while ahead of the others, so all run in step.
          this.#next += session.framesAhead() > 0 ? frameMs * 1.1 : frameMs;
        }
        if (performance.now() >= this.#nextStats) {
          const { delay, rollback } = this.#tuned;
          postMessage({ type: "netplay", event: "stats", ping: session.ping(), delay, rollback });
          this.#nextStats = performance.now() + 1000;
        }
      } else {
        this.#next = performance.now();
        wait = 5;
      }
      for (const [handle, packet] of session.outgoing()) {
        const framed = new Uint8Array(2 + packet.length);
        framed.set([this.#epoch & 0xff, this.#epoch >> 8]);
        framed.set(packet, 2);
        this.#port.postMessage([this.#seats[handle], framed.buffer], [framed.buffer]);
      }
    } else if (this.paused) {
      this.#next = performance.now();
      wait = 5;
    } else {
      for (let i = 0; i < 4 && performance.now() >= this.#next; i++) {
        this.#run([localMask], [this.#seat], true);
        this.#next += frameMs;
      }
    }
    // After a long pause (hidden tab), carry on from now instead of fast-forwarding.
    if (performance.now() - this.#next > 250) this.#next = performance.now();
    if (this.frame) {
      postMessage(this.frame, [this.frame.rgba.buffer]);
      this.frame = undefined;
    }
    setTimeout(this.tick, wait || Math.max(0, this.#next - performance.now()));
  };

  // GGRS's requests, run on the core.
  #machine = {
    save: (slot, checksum) => {
      this.core.saveSlot(slot);
      return checksum ? hashRam(this.core.systemRam()) : undefined;
    },
    load: (slot) => this.core.loadSlot(slot),
    run: (inputs, present) => this.#run(inputs, this.#seats, present),
  };

  /** Runs a frame with `inputs[i]` on seat `seats[i]`'s controller port. */
  #run(inputs, seats, present) {
    const ports = this.core.inputs;
    ports.fill(0);
    inputs.forEach((mask, i) => (ports[seats[i]] = mask));
    // Turn-based games (Pac-Man, Wonder Boy) read player 1's controls on either player's turn,
    // like an upright cabinet, so player 2's stick and buttons go there too; only their Start
    // and Coin stay on port 2.
    if (this.#turns) {
      ports[0] |= ports[1] & ~(START | SELECT);
      ports[1] &= START | SELECT;
    }
    this.core.present = present;
    this.core.run();
  }

  #capture(epoch) {
    const state = this.core.serialize();
    this.#captured = { epoch, state };
    this.paused = true;
    const copy = state.slice();
    postMessage({ type: "captured", epoch, state: copy }, [copy.buffer]);
  }

  #online({ epoch, seats, state }) {
    state ??= this.#captured?.epoch === epoch ? this.#captured.state : undefined;
    if (!state) return;
    this.#leaveSession();
    this.core.unserialize(state);
    this.#captured = undefined;
    if (!this.#tuned) {
      this.muted = true;
      this.#tuned = tune(this.core, this.fps);
      this.muted = false;
      this.core.allocSlots(this.#tuned.rollback + 2);
    }
    const { delay, rollback } = this.#tuned;
    this.#epoch = epoch & 0xffff;
    this.#seats = seats;
    this.#session = new Session(seats.length, seats.indexOf(this.#seat), delay, rollback, Math.round(this.fps));
    this.paused = false;
    this.#next = performance.now();
  }

  #leaveSession() {
    this.#session?.free();
    this.#session = undefined;
    this.#seats = undefined;
    this.paused = false;
  }
}

/**
 * Picks the rollback limit from how fast this machine runs the game: as many re-run frames as
 * fit in 3/4 of a frame next to the shown one, 2 to 8. Slow games (Mortal Kombat II) get fewer
 * and one more frame of input delay, so rollbacks stay short. Leaves the machine as it found it.
 */
function tune(core, fps) {
  const before = core.serialize();
  const msPerFrame = (present, frames) => {
    core.present = present;
    const startedAt = performance.now();
    for (let i = 0; i < frames; i++) core.run();
    return (performance.now() - startedAt) / frames;
  };
  msPerFrame(false, 30); // warm up
  const rerun = msPerFrame(false, 60);
  const shown = msPerFrame(true, 20);
  core.present = true;
  core.unserialize(before);
  const fits = Math.floor(((1000 / fps) * 0.75 - shown) / rerun);
  const rollback = Math.min(8, Math.max(2, fits));
  return { rollback, delay: rollback < 6 ? 3 : 2 };
}

/** FNV-1a over the game's RAM. All machines' hashes match while they're in step. */
function hashRam(bytes) {
  let hash = 0x811c9dc5;
  if (bytes.byteOffset % 4 === 0) {
    const words = new Uint32Array(bytes.buffer, bytes.byteOffset, bytes.length >> 2);
    for (let i = 0; i < words.length; i++) hash = Math.imul(hash ^ words[i], 0x01000193);
    for (let i = words.length * 4; i < bytes.length; i++) hash = Math.imul(hash ^ bytes[i], 0x01000193);
  } else {
    for (let i = 0; i < bytes.length; i++) hash = Math.imul(hash ^ bytes[i], 0x01000193);
  }
  return hash >>> 0;
}
