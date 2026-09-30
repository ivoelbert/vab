use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use worker::*;

/// Files in `web/` are served before this runs (`[assets]` in wrangler.toml), so only
/// requests without a matching file land here.
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        // One Room Durable Object per bar room; the room name picks the instance.
        .get_async("/ws/:room", |req, ctx| async move {
            let Some(room) = ctx.param("room") else {
                return Response::error("Missing room", 400);
            };
            let stub = ctx.durable_object("ROOM")?.id_from_name(room)?.get_stub()?;
            stub.fetch_with_request(req).await
        })
        // WebRTC servers for the two players at a cabinet (web/room.js).
        .get_async(
            "/ice",
            |_req, ctx| async move { ice_servers(&ctx.env).await },
        )
        // FBNeo cores (/fbneo/<core>/fbneo.wasm) and ROM sets (/roms/mk2.zip) live in R2.
        .get_async("/fbneo/*file", |req, ctx| serve_from_r2(req, ctx, "fbneo"))
        .get_async("/roms/*file", |req, ctx| serve_from_r2(req, ctx, "roms"))
        .run(req, env)
        .await
}

/// STUN, plus Cloudflare TURN once a TURN key is set up (the TURN_KEY_ID and TURN_KEY_API_TOKEN
/// secrets). TURN relays through the nearest Cloudflare location when two browsers can't reach
/// each other directly (strict NATs); without it those games go through the room instead,
/// wherever its Durable Object runs.
async fn ice_servers(env: &Env) -> Result<Response> {
    const STUN: &str = r#"{"iceServers":[{"urls":"stun:stun.cloudflare.com:3478"}]}"#;
    let json = |body: String| -> Result<Response> {
        let headers = Headers::new();
        headers.set("content-type", "application/json")?;
        headers.set("cache-control", "no-store")?;
        Ok(Response::ok(body)?.with_headers(headers))
    };
    let (Ok(key), Ok(token)) = (env.secret("TURN_KEY_ID"), env.secret("TURN_KEY_API_TOKEN")) else {
        return json(STUN.into());
    };
    // https://developers.cloudflare.com/realtime/turn/generate-credentials/
    let url = format!(
        "https://rtc.live.cloudflare.com/v1/turn/keys/{key}/credentials/generate-ice-servers"
    );
    let headers = Headers::new();
    headers.set("authorization", &format!("Bearer {token}"))?;
    headers.set("content-type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(r#"{"ttl":86400}"#.into()));
    let mut response = Fetch::Request(Request::new_with_init(&url, &init)?)
        .send()
        .await?;
    if response.status_code() != 201 {
        console_error!("TURN credentials: {}", response.status_code());
        return json(STUN.into());
    }
    json(response.text().await?)
}

/// Streams `<prefix>/<file>` from the R2 bucket, with the content type it was uploaded with.
/// Answers 304 when the browser's copy (If-None-Match) is still current.
async fn serve_from_r2(req: Request, ctx: RouteContext<()>, prefix: &str) -> Result<Response> {
    let Some(file) = ctx.param("file") else {
        return not_found();
    };
    let cached_etag = req
        .headers()
        .get("if-none-match")?
        .map(|etag| etag.trim_matches('"').to_string());
    let Some(object) = ctx
        .bucket("BUCKET")?
        .get(format!("{prefix}/{file}"))
        .only_if(Conditional {
            etag_does_not_match: cached_etag,
            ..Default::default()
        })
        .execute()
        .await?
    else {
        return not_found();
    };
    // `Headers::clone` copies; share the underlying JS object so the metadata
    // (content-type: application/wasm) lands on the response.
    let headers = Headers::new();
    object.write_http_metadata(Headers(headers.0.clone()))?;
    headers.set("etag", &object.http_etag())?;
    // R2 leaves out the body when the etag matched: the browser's copy is current.
    let Some(body) = object.body() else {
        return Ok(Response::empty()?.with_status(304).with_headers(headers));
    };
    Ok(Response::from_body(body.response_body()?)?.with_headers(headers))
}

/// A 404 browsers must not cache: the file may be uploaded later.
fn not_found() -> Result<Response> {
    let headers = Headers::new();
    headers.set("cache-control", "no-store")?;
    Ok(Response::error("Not found", 404)?.with_headers(headers))
}

/// A bar room: everyone in it sees each other walk around and who plays at which cabinet, and
/// the players at a cabinet (up to 4) find each other here to play online. WebSockets go through
/// the Hibernation API, so an idle room is evicted from memory while its connections stay open;
/// what the room knows about each player lives on their socket (its attachment), and each
/// socket is tagged with its player's id.
///
/// Text messages are JSON (`FromPlayer`, `ToPlayer`). Binary messages are for another player,
/// passed on as they are but for the address: `[to: u32 LE][bytes]` in, `[from: u32 LE][bytes]`
/// out. The page sends game packets this way until WebRTC connects, and hands games over.
#[durable_object]
pub struct Room {
    state: State,
}

/// What the room knows about a player.
#[derive(Serialize, Deserialize, Clone)]
struct Player {
    id: u32,
    /// Where their feet are, once they've said.
    at: Option<Position>,
    /// Where they're playing.
    seat: Option<Seat>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Position {
    x: f32,
    y: f32,
    flip: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Seat {
    /// The cabinet's cell, "x,y".
    cabinet: String,
    /// 0 for player 1, and so on.
    index: usize,
    /// How many players the cabinet's game takes.
    of: usize,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum FromPlayer {
    Move(Position),
    /// Take a free seat at a cabinet whose game takes `seats` players (2 if not said, at most
    /// 4), leaving any other.
    Sit {
        cabinet: String,
        seats: Option<usize>,
    },
    Stand,
    /// Messages between the players at a cabinet (WebRTC offers, answers and ICE candidates,
    /// handing a game over), passed on as they are.
    Signal {
        to: u32,
        data: serde_json::Value,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ToPlayer<'a> {
    /// First message: the player's id, and everyone else.
    Welcome {
        id: u32,
        players: Vec<PlayerAt>,
        seats: BTreeMap<String, Vec<Option<u32>>>,
    },
    /// Also how a newcomer first shows up.
    Moved(PlayerAt),
    Left {
        id: u32,
    },
    /// Who sits at a cabinet now: a slot per seat, or none when nobody does.
    Seats {
        cabinet: &'a str,
        players: Vec<Option<u32>>,
    },
    /// All seats were taken.
    Full {
        cabinet: &'a str,
    },
    Signal {
        from: u32,
        data: serde_json::Value,
    },
}

#[derive(Serialize)]
struct PlayerAt {
    id: u32,
    #[serde(flatten)]
    at: Position,
}

impl Room {
    fn players(&self) -> Vec<(WebSocket, Player)> {
        self.state
            .get_websockets()
            .into_iter()
            .filter_map(|ws| {
                let player = ws.deserialize_attachment::<Player>().ok()??;
                Some((ws, player))
            })
            .collect()
    }

    fn socket_of(&self, id: u32) -> Option<WebSocket> {
        let mut sockets = self.state.get_websockets_with_tag(&id.to_string());
        sockets.pop()
    }

    fn broadcast(&self, message: &ToPlayer) -> Result<()> {
        let text = serde_json::to_string(message)?;
        for ws in self.state.get_websockets() {
            // A socket that is closing can't take it; the others still should.
            let _ = ws.send_with_str(&text);
        }
        Ok(())
    }

    /// Who sits at `cabinet`, leaving out the player `except` (who is leaving).
    fn seats_at(&self, cabinet: &str, except: u32) -> Vec<Option<u32>> {
        let seated: Vec<(Seat, u32)> = self
            .players()
            .into_iter()
            .filter_map(|(_, player)| Some((player.seat?, player.id)))
            .filter(|(seat, id)| seat.cabinet == cabinet && *id != except)
            .collect();
        let size = seated.iter().map(|(seat, _)| seat.of).max().unwrap_or(0);
        let mut seats = vec![None; size];
        for (seat, id) in seated {
            seats[seat.index] = Some(id);
        }
        seats
    }

    /// Frees the player's seat, if they have one, and tells everyone.
    fn stand(&self, ws: &WebSocket, player: &mut Player) -> Result<()> {
        let Some(seat) = player.seat.take() else {
            return Ok(());
        };
        ws.serialize_attachment(&*player)?;
        let players = self.seats_at(&seat.cabinet, player.id);
        let cabinet = &seat.cabinet;
        self.broadcast(&ToPlayer::Seats { cabinet, players })
    }

    fn sit(&self, ws: &WebSocket, player: &mut Player, cabinet: String, of: usize) -> Result<()> {
        if player
            .seat
            .as_ref()
            .is_some_and(|seat| seat.cabinet == cabinet)
        {
            return Ok(());
        }
        self.stand(ws, player)?;
        let mut players = self.seats_at(&cabinet, player.id);
        players.resize(players.len().max(of), None);
        let Some(index) = players.iter().position(Option::is_none) else {
            return ws.send(&ToPlayer::Full { cabinet: &cabinet });
        };
        players[index] = Some(player.id);
        let of = players.len();
        player.seat = Some(Seat {
            cabinet: cabinet.clone(),
            index,
            of,
        });
        ws.serialize_attachment(&*player)?;
        self.broadcast(&ToPlayer::Seats {
            cabinet: &cabinet,
            players,
        })
    }

    /// The player's socket closed: free their seat and tell everyone they left.
    fn leave(&self, ws: &WebSocket) -> Result<()> {
        let Some(player) = ws.deserialize_attachment::<Player>()? else {
            return Ok(());
        };
        if let Some(seat) = &player.seat {
            let players = self.seats_at(&seat.cabinet, player.id);
            let cabinet = &seat.cabinet;
            self.broadcast(&ToPlayer::Seats { cabinet, players })?;
        }
        self.broadcast(&ToPlayer::Left { id: player.id })
    }
}

impl DurableObject for Room {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("Expected a WebSocket upgrade", 426);
        }
        let others = self.players();
        let id = loop {
            let id = (js_sys::Math::random() * u32::MAX as f64) as u32;
            if !others.iter().any(|(_, player)| player.id == id) {
                break id;
            }
        };
        let pair = WebSocketPair::new()?;
        let tag = id.to_string();
        self.state.accept_websocket_with_tags(&pair.server, &[&tag]);
        let player = Player {
            id,
            at: None,
            seat: None,
        };
        pair.server.serialize_attachment(&player)?;

        let mut seats = BTreeMap::<String, Vec<Option<u32>>>::new();
        for (_, other) in &others {
            if let Some(seat) = &other.seat {
                let players = seats.entry(seat.cabinet.clone()).or_default();
                players.resize(players.len().max(seat.of), None);
                players[seat.index] = Some(other.id);
            }
        }
        let players = others
            .iter()
            .filter_map(|(_, other)| {
                Some(PlayerAt {
                    id: other.id,
                    at: other.at?,
                })
            })
            .collect();
        pair.server
            .send(&ToPlayer::Welcome { id, players, seats })?;
        Response::from_websocket(pair.client)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let Some(mut player) = ws.deserialize_attachment::<Player>()? else {
            return Ok(());
        };
        let text = match message {
            WebSocketIncomingMessage::String(text) => text,
            WebSocketIncomingMessage::Binary(bytes) => {
                // For another player: swap the address for the sender's.
                if bytes.len() < 4 {
                    return Ok(());
                }
                let to = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                if let Some(other) = self.socket_of(to) {
                    let mut message = bytes;
                    message[..4].copy_from_slice(&player.id.to_le_bytes());
                    other.send_with_bytes(message)?;
                }
                return Ok(());
            }
        };
        match serde_json::from_str::<FromPlayer>(&text)? {
            FromPlayer::Move(at) => {
                player.at = Some(at);
                ws.serialize_attachment(&player)?;
                self.broadcast(&ToPlayer::Moved(PlayerAt { id: player.id, at }))?;
            }
            FromPlayer::Sit { cabinet, seats } => {
                let of = seats.unwrap_or(2).clamp(1, 4);
                self.sit(&ws, &mut player, cabinet, of)?;
            }
            FromPlayer::Stand => self.stand(&ws, &mut player)?,
            FromPlayer::Signal { to, data } => {
                if let Some(other) = self.socket_of(to) {
                    other.send(&ToPlayer::Signal {
                        from: player.id,
                        data,
                    })?;
                }
            }
        }
        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        self.leave(&ws)?;
        // Complete the close handshake, as in Cloudflare's hibernation example. 1005 and 1006
        // mean the client gave no code, and can't be sent back.
        let code = if matches!(code, 1005 | 1006) {
            1000
        } else {
            code as u16
        };
        ws.close(Some(code), Some(reason))
    }

    async fn websocket_error(&self, ws: WebSocket, _error: Error) -> Result<()> {
        self.leave(&ws)
    }
}
