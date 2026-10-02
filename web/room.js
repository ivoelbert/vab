// The page's connection to its bar room (the Room Durable Object in server/src/lib.rs): where
// the other players are, who sits at or watches which cabinet, links to the others at ours, and
// the game streamed to whoever watches it.

// WebRTC servers from the Worker (/ice): STUN, and TURN when it is set up. Its credentials last
// a day; fetched again after 12 hours.
let iceServers;
let iceFetchedAt = 0;
function getIceServers() {
  if (!iceServers || performance.now() - iceFetchedAt > 12 * 3600 * 1000) {
    iceFetchedAt = performance.now();
    iceServers = fetch("/ice")
      .then((response) => response.json())
      .then(({ iceServers }) => iceServers)
      .catch(() => [{ urls: "stun:stun.cloudflare.com:3478" }]);
  }
  return iceServers;
}

// Binary messages through the room, after its [player id: u32] address: a kind byte, then a
// game packet, a piece of something bigger, [id: u32][index: u16][count: u16][bytes] (a game
// handed over, its id the epoch, or a watch state, its id the stream), or watch inputs,
// [stream: u32][frame: u32][a u16 mask per controller port per frame].
const PACKET = 0;
const HANDOVER = 1;
const WATCH_STATE = 2;
const WATCH_INPUTS = 3;
const PIECE = 256 * 1024; // the room takes messages up to 1 MiB
/** The address of everyone watching our cabinet. */
const WATCHERS = 0;

export class Room {
  /** This player's id in the room, from its welcome. */
  id;
  /** Who sits at each cabinet: cabinet ("x,y") -> a player id or null per seat. */
  seats = new Map();
  /** Who watches each cabinet's game: cabinet -> player ids. */
  watchers = new Map();

  #url;
  #events;
  #ws;
  #position;
  #positionSent = 0;
  #positionTimer;
  /** Where this player sits or watches: the message saying so, sent again after reconnecting. */
  #place;
  #name;
  #links = new Map();
  #waitingSignals = new Map();
  #pieces = new Map();

  /**
   * @param url the room's WebSocket, e.g. wss://host/ws/main
   * @param events welcome(name), moved(id, x, y, flip, name), left(id), said(id, name, text),
   *   seats(cabinet, players), full(cabinet), watchers(cabinet, players), message(from, data)
   *   from another player at our cabinet, handover(from, epoch, bytes) a game handed over (see
   *   handOver), watchState(from, stream, bytes) and watchInputs(from, stream, frame, inputs)
   *   the game we watch (see watchState and watchInputs)
   * @param name this player's name, if they set one before; else the room gives one
   */
  constructor(url, events, name) {
    this.#url = url;
    this.#events = events;
    this.#name = name;
    this.#connect(1000);
  }

  /** The name shown above this player and next to what they say. */
  setName(name) {
    this.#name = name;
    this.#send({ type: "name", name });
  }

  /** Says something to everyone in the room. False when not connected. */
  say(text) {
    return this.#send({ type: "say", text });
  }

  /** Where this player stands. Sent at most 10 times a second, always ending on the latest. */
  move(x, y, flip) {
    this.#position = { type: "move", x, y, flip };
    if (this.#positionTimer) return;
    const wait = Math.max(0, this.#positionSent + 100 - performance.now());
    this.#positionTimer = setTimeout(() => {
      this.#positionTimer = undefined;
      this.#positionSent = performance.now();
      this.#send(this.#position);
    }, wait);
  }

  /** Takes a free seat at a cabinet whose game takes `seats` players. */
  sit(cabinet, seats) {
    this.#place = { type: "sit", cabinet, seats };
    this.#send(this.#place);
  }

  /** Watches the game at a cabinet: one of its players streams it here. */
  watch(cabinet) {
    this.#place = { type: "watch", cabinet };
    this.#send(this.#place);
  }

  /** Leaves the seat, or stops watching. */
  stand() {
    this.#place = undefined;
    this.#send({ type: "stand" });
  }

  /** A message for another player at our cabinet (it arrives as events.message). */
  message(to, data) {
    this.#send({ type: "signal", to, data });
  }

  /**
   * A link to another player for game packets and voice. Packets go through the room at first
   * and straight to the other browser (WebRTC) once that connects; voice only goes straight.
   * One side `offers` WebRTC.
   * `match` names the link (both sides pass the same), so WebRTC messages left over from an
   * earlier link are ignored.
   */
  link(partner, offers, match) {
    this.#links.get(partner)?.close();
    const link = new Link(this, partner, offers, match);
    this.#links.set(partner, link);
    for (const data of this.#waitingSignals.get(partner) ?? []) link.signal(data);
    this.#waitingSignals.delete(partner);
    return link;
  }

  unlink(link) {
    link.close();
    if (this.#links.get(link.partner) === link) this.#links.delete(link.partner);
  }

  /** Hands a game (a machine's capture) to another player, in pieces through the room. */
  handOver(to, epoch, bytes) {
    this.#sendPieces(to, HANDOVER, epoch, bytes);
  }

  /**
   * Starts a watcher (`to`) on our game, or everyone watching our cabinet without `to`: a
   * state from the emulator worker ([frame][machine]) that its `stream`'s inputs go on from.
   */
  watchState(to, stream, bytes) {
    this.#sendPieces(to ?? WATCHERS, WATCH_STATE, stream, bytes);
  }

  /** Inputs for everyone watching our cabinet: a Uint16Array, a mask per port per frame. */
  watchInputs(stream, frame, inputs) {
    const header = new DataView(new ArrayBuffer(9));
    header.setUint8(0, WATCH_INPUTS);
    header.setUint32(1, stream, true);
    header.setUint32(5, frame, true);
    this.#sendBinary(WATCHERS, new Uint8Array(header.buffer), new Uint8Array(inputs.buffer, inputs.byteOffset, inputs.byteLength));
  }

  #sendPieces(to, kind, id, bytes) {
    const count = Math.max(1, Math.ceil(bytes.length / PIECE));
    for (let index = 0; index < count; index++) {
      const header = new DataView(new ArrayBuffer(9));
      header.setUint8(0, kind);
      header.setUint32(1, id, true);
      header.setUint16(5, index, true);
      header.setUint16(7, count, true);
      const piece = bytes.subarray(index * PIECE, (index + 1) * PIECE);
      this.#sendBinary(to, new Uint8Array(header.buffer), piece);
    }
  }

  /** A game packet (an ArrayBuffer) for another player, through the room. */
  relay(to, packet) {
    this.#sendBinary(to, Uint8Array.of(PACKET), new Uint8Array(packet));
  }

  #sendBinary(to, header, body) {
    if (this.#ws?.readyState !== WebSocket.OPEN) return;
    const message = new Uint8Array(4 + header.length + body.length);
    new DataView(message.buffer).setUint32(0, to, true);
    message.set(header, 4);
    message.set(body, 4 + header.length);
    this.#ws.send(message);
  }

  #send(message) {
    if (this.#ws?.readyState !== WebSocket.OPEN) return false;
    this.#ws.send(JSON.stringify(message));
    return true;
  }

  #connect(retryMs) {
    const ws = (this.#ws = new WebSocket(this.#url));
    ws.binaryType = "arraybuffer";
    ws.onmessage = ({ data }) => (typeof data === "string" ? this.#receive(JSON.parse(data)) : this.#binary(data));
    ws.onopen = () => (retryMs = 1000);
    ws.onclose = () => {
      this.seats.clear();
      this.watchers.clear();
      setTimeout(() => this.#connect(Math.min(retryMs * 2, 10000)), retryMs);
    };
  }

  #receive(message) {
    const events = this.#events;
    switch (message.type) {
      case "welcome":
        this.id = message.id;
        this.seats = new Map(Object.entries(message.seats));
        this.watchers = new Map(Object.entries(message.watchers));
        if (this.#name) this.#send({ type: "name", name: this.#name });
        events.welcome(this.#name ?? message.name);
        for (const { id, x, y, flip, name } of message.players) events.moved(id, x, y, flip, name);
        for (const [cabinet, players] of this.seats) events.seats(cabinet, players);
        for (const [cabinet, players] of this.watchers) events.watchers(cabinet, players);
        // Back again after a lost connection, with a new id: say where we are and sit back down
        // (or watch again).
        if (this.#position) this.#send(this.#position);
        if (this.#place) this.#send(this.#place);
        break;
      case "moved":
        if (message.id !== this.id) events.moved(message.id, message.x, message.y, message.flip, message.name);
        break;
      case "said":
        events.said(message.id, message.name, message.text);
        break;
      case "left":
        events.left(message.id);
        break;
      case "seats":
        this.seats.set(message.cabinet, message.players);
        events.seats(message.cabinet, message.players);
        break;
      case "full":
        events.full(message.cabinet);
        break;
      case "watchers":
        this.watchers.set(message.cabinet, message.players);
        events.watchers(message.cabinet, message.players);
        break;
      case "signal": {
        const { from, data } = message;
        if (!data.link) {
          events.message(from, data);
          break;
        }
        // WebRTC, for a link that may not exist yet.
        const link = this.#links.get(from);
        if (link) link.signal(data);
        else this.#waitingSignals.set(from, [...(this.#waitingSignals.get(from) ?? []), data]);
        break;
      }
    }
  }

  #binary(message) {
    const view = new DataView(message);
    const from = view.getUint32(0, true);
    const kind = view.getUint8(4);
    if (kind === PACKET) {
      this.#links.get(from)?.onpacket(message.slice(5));
      return;
    }
    const id = view.getUint32(5, true);
    if (kind === WATCH_INPUTS) {
      // Copied: a Uint16Array can't start at an odd offset.
      this.#events.watchInputs(from, id, view.getUint32(9, true), new Uint16Array(message.slice(13)));
      return;
    }
    const index = view.getUint16(9, true);
    const count = view.getUint16(11, true);
    const key = `${from}/${kind}/${id}`;
    const pieces = this.#pieces.get(key) ?? [];
    pieces[index] = new Uint8Array(message, 13);
    this.#pieces.set(key, pieces);
    if (pieces.filter(Boolean).length < count) return;
    this.#pieces.delete(key);
    const bytes = new Uint8Array(pieces.reduce((total, piece) => total + piece.length, 0));
    pieces.reduce((offset, piece) => (bytes.set(piece, offset), offset + piece.length), 0);
    if (kind === HANDOVER) this.#events.handover(from, id, bytes);
    if (kind === WATCH_STATE) this.#events.watchState(from, id, bytes);
  }
}

/** Game packets and voice to and from another player at our cabinet. */
class Link {
  /** Called with each packet (an ArrayBuffer) from the other player. */
  onpacket = () => {};
  /** Called with the other player's voice, a MediaStream, once it's set up. */
  onvoice = () => {};
  /** The other player's voice, once it's set up. */
  voice;

  #room;
  #match;
  #pc;
  #ready;
  #closed = false;
  #channel;
  #candidates = [];
  /** The audio both ways, and the microphone track we send on it. */
  #audio;
  #microphone = null;

  constructor(room, partner, offers, match) {
    this.#room = room;
    this.#match = match;
    this.partner = partner;
    this.#ready = getIceServers().then((iceServers) => this.#connect(iceServers, offers));
  }

  /** True once packets go straight to the other browser. */
  get direct() {
    return this.#channel?.readyState === "open";
  }

  send(packet) {
    if (this.direct) this.#channel.send(packet);
    else this.#room.relay(this.partner, packet);
  }

  /** Sends them our microphone (a MediaStreamTrack), or nothing with null. */
  microphone(track) {
    this.#microphone = track;
    this.#audio?.sender.replaceTrack(track).catch((error) => console.warn("Voice:", error));
  }

  async signal({ link, description, candidate }) {
    if (link !== this.#match) return;
    await this.#ready;
    if (this.#closed) return;
    try {
      if (description) {
        await this.#pc.setRemoteDescription(description);
        for (const waiting of this.#candidates.splice(0)) await this.#pc.addIceCandidate(waiting);
        if (description.type === "offer") {
          // Answer their audio with ours. Offers from before voice have none.
          const audio = this.#pc.getTransceivers().find(({ receiver }) => receiver.track.kind === "audio");
          if (audio) {
            audio.direction = "sendrecv";
            this.#useAudio(audio);
          }
          await this.#pc.setLocalDescription(await this.#pc.createAnswer());
          this.#signal({ description: this.#pc.localDescription });
        }
      } else if (candidate) {
        // Candidates can arrive before the offer or answer they belong to.
        if (this.#pc.remoteDescription) await this.#pc.addIceCandidate(candidate);
        else this.#candidates.push(candidate);
      }
    } catch (error) {
      console.warn("WebRTC:", error);
    }
  }

  close() {
    this.onpacket = () => {};
    this.#closed = true;
    this.#pc?.close();
  }

  #connect(iceServers, offers) {
    if (this.#closed) return;
    const pc = (this.#pc = new RTCPeerConnection({ iceServers }));
    pc.onicecandidate = ({ candidate }) => candidate && this.#signal({ candidate });
    pc.ontrack = ({ track, streams }) => {
      this.voice = streams[0] ?? new MediaStream([track]);
      this.onvoice(this.voice);
    };
    if (offers) {
      // Unordered and never resent: GGRS resends what matters itself.
      this.#use(pc.createDataChannel("ggrs", { ordered: false, maxRetransmits: 0 }));
      // Audio both ways from the start, so the microphone can come and go without offering
      // again (replaceTrack).
      this.#useAudio(pc.addTransceiver("audio", { direction: "sendrecv" }));
      pc.createOffer()
        .then((offer) => pc.setLocalDescription(offer))
        .then(() => this.#signal({ description: pc.localDescription }))
        .catch((error) => console.warn("WebRTC offer:", error));
    } else {
      pc.ondatachannel = ({ channel }) => this.#use(channel);
    }
  }

  #signal(data) {
    this.#room.message(this.partner, { link: this.#match, ...data });
  }

  #useAudio(audio) {
    this.#audio = audio;
    if (this.#microphone) this.microphone(this.#microphone);
  }

  #use(channel) {
    channel.binaryType = "arraybuffer";
    channel.onmessage = ({ data }) => this.onpacket(data);
    this.#channel = channel;
  }
}
