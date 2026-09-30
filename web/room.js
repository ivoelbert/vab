// The page's connection to its bar room (the Room Durable Object in server/src/lib.rs): where
// the other players are, who sits at which cabinet, and links to the others at ours.

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
// game packet, or a piece of a game handed over: [epoch: u32][index: u16][count: u16][bytes].
const PACKET = 0;
const HANDOVER = 1;
const HANDOVER_PIECE = 256 * 1024; // the room takes messages up to 1 MiB

export class Room {
  /** This player's id in the room, from its welcome. */
  id;
  /** Who sits at each cabinet: cabinet ("x,y") -> a player id or null per seat. */
  seats = new Map();

  #url;
  #events;
  #ws;
  #position;
  #positionSent = 0;
  #positionTimer;
  #sitting;
  #name;
  #links = new Map();
  #waitingSignals = new Map();
  #handovers = new Map();

  /**
   * @param url the room's WebSocket, e.g. wss://host/ws/main
   * @param events welcome(name), moved(id, x, y, flip, name), left(id), said(id, name, text),
   *   seats(cabinet, players), full(cabinet), message(from, data) from another player at our
   *   cabinet, handover(from, epoch, bytes) a game handed over (see handOver)
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
    this.#sitting = { type: "sit", cabinet, seats };
    this.#send(this.#sitting);
  }

  stand() {
    this.#sitting = undefined;
    this.#send({ type: "stand" });
  }

  /** A message for another player at our cabinet (it arrives as events.message). */
  message(to, data) {
    this.#send({ type: "signal", to, data });
  }

  /**
   * A link to another player for game packets. Packets go through the room at first and
   * straight to the other browser (WebRTC) once that connects. One side `offers` WebRTC.
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
    const count = Math.max(1, Math.ceil(bytes.length / HANDOVER_PIECE));
    for (let index = 0; index < count; index++) {
      const header = new DataView(new ArrayBuffer(9));
      header.setUint8(0, HANDOVER);
      header.setUint32(1, epoch, true);
      header.setUint16(5, index, true);
      header.setUint16(7, count, true);
      const piece = bytes.subarray(index * HANDOVER_PIECE, (index + 1) * HANDOVER_PIECE);
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
      setTimeout(() => this.#connect(Math.min(retryMs * 2, 10000)), retryMs);
    };
  }

  #receive(message) {
    const events = this.#events;
    switch (message.type) {
      case "welcome":
        this.id = message.id;
        this.seats = new Map(Object.entries(message.seats));
        if (this.#name) this.#send({ type: "name", name: this.#name });
        events.welcome(this.#name ?? message.name);
        for (const { id, x, y, flip, name } of message.players) events.moved(id, x, y, flip, name);
        for (const [cabinet, players] of this.seats) events.seats(cabinet, players);
        // Back again after a lost connection, with a new id: say where we are and sit back down.
        if (this.#position) this.#send(this.#position);
        if (this.#sitting) this.#send(this.#sitting);
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
    if (view.getUint8(4) === PACKET) {
      this.#links.get(from)?.onpacket(message.slice(5));
      return;
    }
    const epoch = view.getUint32(5, true);
    const index = view.getUint16(9, true);
    const count = view.getUint16(11, true);
    const key = `${from}/${epoch}`;
    const pieces = this.#handovers.get(key) ?? [];
    pieces[index] = new Uint8Array(message, 13);
    this.#handovers.set(key, pieces);
    if (pieces.filter(Boolean).length < count) return;
    this.#handovers.delete(key);
    const bytes = new Uint8Array(pieces.reduce((total, piece) => total + piece.length, 0));
    pieces.reduce((offset, piece) => (bytes.set(piece, offset), offset + piece.length), 0);
    this.#events.handover(from, epoch, bytes);
  }
}

/** Game packets to and from another player at our cabinet. */
class Link {
  /** Called with each packet (an ArrayBuffer) from the other player. */
  onpacket = () => {};

  #room;
  #match;
  #pc;
  #ready;
  #closed = false;
  #channel;
  #candidates = [];

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

  async signal({ link, description, candidate }) {
    if (link !== this.#match) return;
    await this.#ready;
    if (this.#closed) return;
    try {
      if (description) {
        await this.#pc.setRemoteDescription(description);
        for (const waiting of this.#candidates.splice(0)) await this.#pc.addIceCandidate(waiting);
        if (description.type === "offer") {
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
    if (offers) {
      // Unordered and never resent: GGRS resends what matters itself.
      this.#use(pc.createDataChannel("ggrs", { ordered: false, maxRetransmits: 0 }));
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

  #use(channel) {
    channel.binaryType = "arraybuffer";
    channel.onmessage = ({ data }) => this.onpacket(data);
    this.#channel = channel;
  }
}
