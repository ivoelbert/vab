// The page's connection to its bar room (the Room Durable Object in server/src/lib.rs): where
// the other players are, who sits at which cabinet, and a link to the other player at ours.

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

export class Room {
  /** This player's id in the room, from its welcome. */
  id;
  /** Who sits at each cabinet: cabinet ("x,y") -> [player id or null, player id or null]. */
  seats = new Map();

  #url;
  #events;
  #ws;
  #position;
  #positionSent = 0;
  #positionTimer;
  #cabinet;
  #links = new Map();
  #waitingSignals = new Map();

  /**
   * @param url the room's WebSocket, e.g. wss://host/ws/main
   * @param events welcome(), moved(id, x, y, flip), left(id), seats(cabinet, players), full(cabinet)
   */
  constructor(url, events) {
    this.#url = url;
    this.#events = events;
    this.#connect(1000);
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

  sit(cabinet) {
    this.#cabinet = cabinet;
    this.#send({ type: "sit", cabinet });
  }

  stand() {
    this.#cabinet = undefined;
    this.#send({ type: "stand" });
  }

  /**
   * A link to another player for game packets. Packets go through the room at first and
   * straight to the other browser (WebRTC) once that connects. Seat 0 makes the WebRTC offer.
   * `match` names this game (both players pass the same), so WebRTC messages left over from
   * an earlier one are ignored.
   */
  link(partner, seat, match) {
    this.#links.get(partner)?.close();
    const link = new Link(this, partner, seat === 0, match);
    this.#links.set(partner, link);
    for (const data of this.#waitingSignals.get(partner) ?? []) link.signal(data);
    this.#waitingSignals.delete(partner);
    return link;
  }

  unlink(link) {
    link.close();
    if (this.#links.get(link.partner) === link) this.#links.delete(link.partner);
  }

  signal(to, data) {
    this.#send({ type: "signal", to, data });
  }

  relay(to, packet) {
    if (this.#ws?.readyState !== WebSocket.OPEN) return;
    const message = new Uint8Array(4 + packet.byteLength);
    new DataView(message.buffer).setUint32(0, to, true);
    message.set(new Uint8Array(packet), 4);
    this.#ws.send(message);
  }

  #send(message) {
    if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(JSON.stringify(message));
  }

  #connect(retryMs) {
    const ws = (this.#ws = new WebSocket(this.#url));
    ws.binaryType = "arraybuffer";
    ws.onmessage = ({ data }) => (typeof data === "string" ? this.#receive(JSON.parse(data)) : this.#packet(data));
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
        events.welcome();
        for (const { id, x, y, flip } of message.players) events.moved(id, x, y, flip);
        for (const [cabinet, players] of this.seats) events.seats(cabinet, players);
        // Back again after a lost connection, with a new id: say where we are and sit back down.
        if (this.#position) this.#send(this.#position);
        if (this.#cabinet) this.sit(this.#cabinet);
        break;
      case "moved":
        if (message.id !== this.id) events.moved(message.id, message.x, message.y, message.flip);
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
        const link = this.#links.get(message.from);
        if (link) link.signal(message.data);
        else this.#waitingSignals.set(message.from, [...(this.#waitingSignals.get(message.from) ?? []), message.data]);
        break;
      }
    }
  }

  #packet(message) {
    const from = new DataView(message).getUint32(0, true);
    this.#links.get(from)?.onpacket(message.slice(4));
  }
}

/** Game packets to and from the other player at a cabinet. */
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

  async signal({ match, description, candidate }) {
    if (match !== this.#match) return;
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
    this.#room.signal(this.partner, { match: this.#match, ...data });
  }

  #use(channel) {
    channel.binaryType = "arraybuffer";
    channel.onmessage = ({ data }) => this.onpacket(data);
    this.#channel = channel;
  }
}
