//! Rollback netplay for the players at a cabinet (up to 4), run by the emulator worker
//! (web/emulator/worker.js) next to the FBNeo core. GGRS decides when to save, load and run
//! frames; the worker does it on the core, through the `Machine` it passes in. Packets go in
//! and out as bytes addressed by player (GGRS handles 0..players, in seat order), and the page
//! carries them between the players.
//!
//! A session has a fixed set of players. When someone joins or leaves, the page starts a new
//! session for everyone from one machine's capture, so GGRS never has to drop a player.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use ggrs::{
    Config, DesyncDetection, GgrsError, GgrsEvent, GgrsRequest, Message, NonBlockingSocket,
    P2PSession, PlayerType, PredictRepeatLast, SessionBuilder, SessionState,
};
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

/// Frames between checks that all machines still match (hashes of the game's RAM).
const DESYNC_INTERVAL: i32 = 60;

struct Cabinet;

impl Config for Cabinet {
    /// A RetroPad mask: bit (1 << id) per held button.
    type Input = u16;
    type InputPredictor = PredictRepeatLast;
    /// The save slot in the core's memory that holds a frame.
    type State = u32;
    /// A player's handle.
    type Address = usize;
}

/// Packets between GGRS and the page.
#[derive(Default)]
struct Wire {
    incoming: Vec<(usize, Message)>,
    outgoing: Vec<(usize, Vec<u8>)>,
}

struct Socket(Rc<RefCell<Wire>>);

impl NonBlockingSocket<usize> for Socket {
    fn send_to(&mut self, message: &Message, player: &usize) {
        if let Ok(bytes) = bincode::serialize(message) {
            self.0.borrow_mut().outgoing.push((*player, bytes));
        }
    }

    fn receive_all_messages(&mut self) -> Vec<(usize, Message)> {
        std::mem::take(&mut self.0.borrow_mut().incoming)
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);

    /// The worker's side, which runs GGRS's requests on the core.
    pub type Machine;

    /// Saves the machine into `slot`. With `checksum`, returns a hash of the game's RAM.
    #[wasm_bindgen(method)]
    fn save(this: &Machine, slot: u32, checksum: bool) -> Option<u32>;

    #[wasm_bindgen(method)]
    fn load(this: &Machine, slot: u32);

    /// Runs one frame with every player's input, in handle order. `present` is false for
    /// frames re-run after a rollback, which aren't shown or heard.
    #[wasm_bindgen(method)]
    fn run(this: &Machine, inputs: Vec<u16>, present: bool);
}

#[wasm_bindgen]
pub struct Session {
    ggrs: P2PSession<Cabinet>,
    wire: Rc<RefCell<Wire>>,
    local: usize,
    players: usize,
    slots: i32,
}

#[wasm_bindgen]
impl Session {
    /// A session for `players` players where this machine plays handle `local`. Input delay
    /// and the rollback limit are in frames; `fps` is the game's rate.
    #[wasm_bindgen(constructor)]
    pub fn new(
        players: usize,
        local: usize,
        input_delay: usize,
        max_rollback: usize,
        fps: usize,
    ) -> Result<Session, JsError> {
        // A panic otherwise shows up as "unreachable" with no message.
        std::panic::set_hook(Box::new(|info| error(&info.to_string())));
        let wire = Rc::new(RefCell::new(Wire::default()));
        let mut builder = SessionBuilder::<Cabinet>::new()
            .with_num_players(players)?
            .with_input_delay(input_delay)
            .with_max_prediction_window(max_rollback)
            .with_fps(fps)?
            .with_desync_detection_mode(DesyncDetection::On {
                interval: DESYNC_INTERVAL as u32,
            })
            // Players leave through the room (a new session without them), never through GGRS:
            // dropping a player mid-session can panic in GGRS 0.13 with more than two players.
            .with_disconnect_timeout(Duration::from_secs(24 * 3600))
            .with_disconnect_notify_delay(Duration::from_secs(1));
        for handle in 0..players {
            let kind = if handle == local {
                PlayerType::Local
            } else {
                PlayerType::Remote(handle)
            };
            builder = builder.add_player(kind, handle)?;
        }
        let ggrs = builder.start_p2p_session(Socket(wire.clone()))?;
        Ok(Session {
            ggrs,
            wire,
            local,
            players,
            // GGRS keeps max_rollback + 1 frames; one more slot so a frame it may still load
            // is never overwritten.
            slots: max_rollback as i32 + 2,
        })
    }

    /// A packet from player `from`.
    pub fn receive(&self, from: usize, packet: &[u8]) {
        if let Ok(message) = bincode::deserialize(packet) {
            self.wire.borrow_mut().incoming.push((from, message));
        }
    }

    /// Packets for the other players since the last call, as `[player, bytes]` pairs.
    pub fn outgoing(&self) -> Array {
        std::mem::take(&mut self.wire.borrow_mut().outgoing)
            .into_iter()
            .map(|(to, bytes)| Array::of2(&(to as u32).into(), &Uint8Array::from(bytes.as_slice())))
            .map(JsValue::from)
            .collect()
    }

    /// Handles packets that arrived and resends what others haven't acknowledged.
    pub fn poll(&mut self) {
        self.ggrs.poll_remote_clients();
    }

    /// True once all players are connected and in step.
    pub fn running(&self) -> bool {
        self.ggrs.current_state() == SessionState::Running
    }

    /// How many frames this machine is ahead of the others; slow down a little when positive.
    #[wasm_bindgen(js_name = framesAhead)]
    pub fn frames_ahead(&self) -> i32 {
        self.ggrs.frames_ahead()
    }

    /// The slowest round trip to another player in milliseconds, once known.
    pub fn ping(&self) -> Option<u32> {
        (0..self.players)
            .filter(|&handle| handle != self.local)
            .filter_map(|handle| self.ggrs.network_stats(handle).ok())
            .map(|stats| stats.ping.min(u32::MAX as u128) as u32)
            .max()
    }

    /// Adds the local player's input and runs the frames GGRS asks for on `machine`. False when
    /// another player is too far behind to keep predicting: try again on the next tick.
    pub fn advance(&mut self, input: u16, machine: &Machine) -> Result<bool, JsError> {
        self.ggrs.add_local_input(self.local, input)?;
        let requests = match self.ggrs.advance_frame() {
            Ok(requests) => requests,
            Err(GgrsError::PredictionThreshold) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let shown = requests
            .iter()
            .rposition(|request| matches!(request, GgrsRequest::AdvanceFrame { .. }));
        for (i, request) in requests.into_iter().enumerate() {
            match request {
                GgrsRequest::SaveGameState { cell, frame } => {
                    let slot = frame.rem_euclid(self.slots) as u32;
                    let checksum = machine.save(slot, frame % DESYNC_INTERVAL == 0);
                    cell.save(frame, Some(slot), checksum.map(u128::from));
                }
                GgrsRequest::LoadGameState { cell, .. } => {
                    if let Some(slot) = cell.load() {
                        machine.load(slot);
                    }
                }
                GgrsRequest::AdvanceFrame { inputs } => {
                    let inputs = inputs.iter().map(|(input, _)| *input).collect();
                    machine.run(inputs, Some(i) == shown);
                }
            }
        }
        Ok(true)
    }

    /// What happened since the last call, as `{ type, ... }` objects: `synchronizing` (count,
    /// total), `synchronized`, `interrupted`, `resumed`, `desync` (frame), each about a `player`
    /// (handle) except `desync`.
    pub fn events(&mut self) -> Array {
        self.ggrs
            .events()
            .filter_map(|event| {
                let (kind, fields): (&str, Vec<(&str, f64)>) = match event {
                    GgrsEvent::Synchronizing { addr, count, total } => (
                        "synchronizing",
                        vec![
                            ("player", addr as f64),
                            ("count", count as f64),
                            ("total", total as f64),
                        ],
                    ),
                    GgrsEvent::Synchronized { addr } => {
                        ("synchronized", vec![("player", addr as f64)])
                    }
                    GgrsEvent::NetworkInterrupted { addr, .. } => {
                        ("interrupted", vec![("player", addr as f64)])
                    }
                    GgrsEvent::NetworkResumed { addr } => {
                        ("resumed", vec![("player", addr as f64)])
                    }
                    GgrsEvent::DesyncDetected { frame, .. } => {
                        ("desync", vec![("frame", frame as f64)])
                    }
                    // Pacing follows framesAhead instead, and players leave through the room.
                    GgrsEvent::WaitRecommendation { .. } | GgrsEvent::Disconnected { .. } => {
                        return None;
                    }
                };
                let object = Object::new();
                let _ = Reflect::set(&object, &"type".into(), &kind.into());
                for (key, value) in fields {
                    let _ = Reflect::set(&object, &key.into(), &value.into());
                }
                Some(JsValue::from(object))
            })
            .collect()
    }
}
