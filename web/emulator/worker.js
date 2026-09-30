// Runs an FBNeo core off the main thread for one cabinet. Posts each frame to the page (which
// hands it to Bevy) and streams audio to the AudioWorklet (audio.js).
//
// Alone, the local player's buttons drive their seat's controller port. With `netplay`, both
// players' machines run the same game in step with rollback (netplay/src/lib.rs): GGRS guesses
// the other player's input and re-runs frames once the real input arrives. Its packets go out
// and come in on `netplay.port`, and the page carries them to the other player.
//
// In:  { type: "start", core, rom, files, state, seat, turns, netplay } | { type: "input", mask } |
//      { type: "solo" } | { type: "audio", port }
//      `files` (a BIOS) and `state` are optional: skipped if missing. `netplay` is { port } or
//      absent. "solo" ends netplay (the other player left) and keeps the game going.
// Out: { type: "frame", rgba, width, height } | { type: "netplay", event, ... }
import { Core } from "./libretro.js";

// RetroPad bits (libretro.h).
const SELECT = 1 << 2; // coin
const START = 1 << 3;

let audioPort;
let localMask = 0;
let leaveNetplay;

onmessage = ({ data: msg }) => {
  if (msg.type === "input") localMask = msg.mask;
  if (msg.type === "audio") audioPort = msg.port;
  if (msg.type === "solo") leaveNetplay?.();
  if (msg.type === "start") start(msg);
};

// "no-cache" checks with the server every time (a 304 when unchanged), so newly uploaded or
// replaced ROMs, BIOS sets and states are picked up.
const download = async (url) => {
  const response = await fetch(url, { cache: "no-cache" });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
};
const downloadIfPresent = (url) => url && download(url).catch(() => undefined);

async function start({ core: coreUrl, rom: romUrl, files = [], state: stateUrl, seat = 0, turns = false, netplay }) {
  const { default: createFBNeo } = await import(coreUrl);
  const [rom, state, ...extras] = await Promise.all([
    download(romUrl),
    downloadIfPresent(stateUrl),
    ...files.map(downloadIfPresent),
  ]);
  let frame;
  let muted = false;
  const core = await Core.create(createFBNeo, {
    onFrame: (rgba, width, height) => muted || (frame = { type: "frame", rgba, width, height }),
    onAudio: (samples) => muted || audioPort?.postMessage(samples, [samples.buffer]),
    onLog: (level, text) => level >= 2 && console.warn(text),
  });
  core.netplay = Boolean(netplay);
  files.forEach((url, i) => extras[i] && core.addFile(url.split("/").pop(), extras[i]));
  const { fps } = core.loadGame(romUrl.split("/").pop(), rom);
  // A start-up state (emulator/snapshot.mjs) skips the boot screens and adds credits. States
  // from an older core build don't load; the game then just boots normally.
  try {
    if (state) core.unserialize(state);
  } catch (error) {
    console.warn(`${stateUrl}: ${error.message}`);
  }

  // Both players' masks to the core's ports. Turn-based games (Pac-Man, Wonder Boy) read
  // player 1's controls on either player's turn, like an upright cabinet, so player 2's stick
  // and buttons go there too; only their Start and Coin stay on port 2.
  const setPorts = (input0, input1) => {
    core.inputs[0] = turns ? input0 | (input1 & ~(START | SELECT)) : input0;
    core.inputs[1] = turns ? input1 & (START | SELECT) : input1;
  };
  const machine = {
    save(slot, checksum) {
      core.saveSlot(slot);
      return checksum ? hashRam(core.systemRam()) : undefined;
    },
    load: (slot) => core.loadSlot(slot),
    run(input0, input1, present) {
      setPorts(input0, input1);
      core.present = present;
      core.run();
    },
  };

  let session;
  let stats = {};
  if (netplay) {
    muted = true;
    const { default: init, Session } = await import("../netplay/netplay.js");
    await init();
    const { inputDelay, maxRollback } = tune(core, fps);
    muted = false;
    core.allocSlots(maxRollback + 2);
    session = new Session(seat, inputDelay, maxRollback, Math.round(fps));
    stats = { delay: inputDelay, rollback: maxRollback };
    netplay.port.onmessage = ({ data }) => session?.receive(new Uint8Array(data));
    leaveNetplay = () => {
      session?.free();
      session = undefined;
    };
  }
  const flush = () => {
    for (const packet of session.outgoing()) netplay.port.postMessage(packet.buffer, [packet.buffer]);
  };

  // Run at the game's own rate (MK II: 54.71 Hz) and send only the newest frame.
  const frameMs = 1000 / fps;
  let next = performance.now();
  let nextStats = next;
  const tick = () => {
    let wait = 0;
    if (session) {
      session.poll();
      for (const { type: event, ...fields } of session.events()) {
        postMessage({ type: "netplay", event, ...fields });
      }
      if (session.running()) {
        for (let i = 0; i < 4 && performance.now() >= next; i++) {
          // False: the other player is too far behind to keep guessing. Try again shortly.
          if (!session.advance(localMask, machine)) {
            wait = 2;
            break;
          }
          // A little slower while ahead of the other machine, so both run in step.
          next += session.framesAhead() > 0 ? frameMs * 1.1 : frameMs;
        }
        if (performance.now() >= nextStats) {
          postMessage({ type: "netplay", event: "stats", ping: session.ping(), ...stats });
          nextStats = performance.now() + 1000;
        }
      } else {
        next = performance.now();
        wait = 5;
      }
      flush();
    } else {
      for (let i = 0; i < 4 && performance.now() >= next; i++) {
        machine.run(seat === 0 ? localMask : 0, seat === 1 ? localMask : 0, true);
        next += frameMs;
      }
    }
    // After a long pause (hidden tab), carry on from now instead of fast-forwarding.
    if (performance.now() - next > 250) next = performance.now();
    if (frame) {
      postMessage(frame, [frame.rgba.buffer]);
      frame = undefined;
    }
    setTimeout(tick, wait || Math.max(0, next - performance.now()));
  };
  tick();
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
  const maxRollback = Math.min(8, Math.max(2, fits));
  return { maxRollback, inputDelay: maxRollback < 6 ? 3 : 2 };
}

/** FNV-1a over the game's RAM. Both machines' hashes match while they're in step. */
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
