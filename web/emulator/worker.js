// Runs an FBNeo core off the main thread. Posts each frame to the page (which hands it to
// Bevy) and streams audio to the AudioWorklet (audio.js).
// Messages in: { type: "start", core, rom, files, state } | { type: "input", port, mask } |
// { type: "audio", port }. `files` (a BIOS) and `state` are optional: skipped if missing.
import { Core } from "./libretro.js";

let audioPort;
const input = new Uint16Array(4);

onmessage = ({ data: msg }) => {
  if (msg.type === "input") input[msg.port] = msg.mask;
  if (msg.type === "audio") audioPort = msg.port;
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

async function start({ core: coreUrl, rom: romUrl, files = [], state: stateUrl }) {
  const { default: createFBNeo } = await import(coreUrl);
  const [rom, state, ...extras] = await Promise.all([
    download(romUrl),
    downloadIfPresent(stateUrl),
    ...files.map(downloadIfPresent),
  ]);
  let frame;
  const core = await Core.create(createFBNeo, {
    onFrame: (rgba, width, height) => (frame = { rgba, width, height }),
    onAudio: (samples) => audioPort?.postMessage(samples, [samples.buffer]),
    onLog: (level, text) => level >= 2 && console.warn(text),
  });
  files.forEach((url, i) => extras[i] && core.addFile(url.split("/").pop(), extras[i]));
  const { fps } = core.loadGame(romUrl.split("/").pop(), rom);
  // A start-up state (emulator/snapshot.mjs) skips the boot screens and adds credits. States
  // from an older core build don't load; the game then just boots normally.
  try {
    if (state) core.unserialize(state);
  } catch (error) {
    console.warn(`${stateUrl}: ${error.message}`);
  }

  // Run at the game's own rate (MK II: 54.71 Hz) and send only the newest frame.
  const frameMs = 1000 / fps;
  let next = performance.now();
  const tick = () => {
    core.inputs.set(input);
    for (let i = 0; i < 4 && performance.now() >= next; i++) {
      core.run();
      next += frameMs;
    }
    // After a long pause (hidden tab), carry on from now instead of fast-forwarding.
    if (performance.now() - next > 250) next = performance.now();
    if (frame) {
      postMessage(frame, [frame.rgba.buffer]);
      frame = undefined;
    }
    setTimeout(tick, Math.max(0, next - performance.now()));
  };
  tick();
}
