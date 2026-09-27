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
        // FBNeo cores (/fbneo/<core>/fbneo.wasm) and ROM sets (/roms/mk2.zip) live in R2.
        .get_async("/fbneo/*file", |req, ctx| serve_from_r2(req, ctx, "fbneo"))
        .get_async("/roms/*file", |req, ctx| serve_from_r2(req, ctx, "roms"))
        .run(req, env)
        .await
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

/// A bar room. Accepts players' WebSockets through the Hibernation API, so an idle room
/// is evicted from memory while its connections stay open.
#[durable_object]
pub struct Room {
    state: State,
}

impl DurableObject for Room {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        if req.headers().get("Upgrade")?.as_deref() != Some("websocket") {
            return Response::error("Expected a WebSocket upgrade", 426);
        }
        let pair = WebSocketPair::new()?;
        self.state.accept_web_socket(&pair.server);
        Response::from_websocket(pair.client)
    }

    async fn websocket_message(
        &self,
        _ws: WebSocket,
        _message: WebSocketIncomingMessage,
    ) -> Result<()> {
        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        code: usize,
        reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        // Complete the close handshake, as in Cloudflare's hibernation example.
        ws.close(Some(code as u16), Some(reason))
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: Error) -> Result<()> {
        Ok(())
    }
}
