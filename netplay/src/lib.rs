//! Rollback netplay for the two players at a cabinet, run by the emulator worker
//! (web/emulator/worker.js) next to the FBNeo core. GGRS decides when to save, load and run
//! frames; the worker does it on the core, through the `Machine` it passes in. Packets go in
//! and out as bytes, and the page carries them to the other player.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use ggrs::{
    Config, DesyncDetection, GgrsError, GgrsEvent, GgrsRequest, Message, NonBlockingSocket,
    P2PSession, PlayerType, PredictRepeatLast, SessionBuilder, SessionState,
};
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

/// Frames between checks that both machines still match (hashes of the game's RAM).
const DESYNC_INTERVAL: i32 = 60;
/// The other player, as GGRS addresses them. There is only ever one.
const PEER: u8 = 1;

struct Cabinet;

impl Config for Cabinet {
    /// A RetroPad mask: bit (1 << id) per held button.
    type Input = u16;
    type InputPredictor = PredictRepeatLast;
    /// The save slot in the core's memory that holds a frame.
    type State = u32;
    type Address = u8;
}

/// Packets between GGRS and the page.
#[derive(Default)]
struct Wire {
    incoming: Vec<Message>,
    outgoing: Vec<Vec<u8>>,
}

struct Socket(Rc<RefCell<Wire>>);

impl NonBlockingSocket<u8> for Socket {
    fn send_to(&mut self, message: &Message, _peer: &u8) {
        if let Ok(bytes) = bincode::serialize(message) {
            self.0.borrow_mut().outgoing.push(bytes);
        }
    }

    fn receive_all_messages(&mut self) -> Vec<(u8, Message)> {
        let incoming = std::mem::take(&mut self.0.borrow_mut().incoming);
        incoming
            .into_iter()
            .map(|message| (PEER, message))
            .collect()
    }
}

#[wasm_bindgen]
extern "C" {
    /// The worker's side, which runs GGRS's requests on the core.
    pub type Machine;

    /// Saves the machine into `slot`. With `checksum`, returns a hash of the game's RAM.
    #[wasm_bindgen(method)]
    fn save(this: &Machine, slot: u32, checksum: bool) -> Option<u32>;

    #[wasm_bindgen(method)]
    fn load(this: &Machine, slot: u32);

    /// Runs one frame with both players' inputs. `present` is false for frames re-run after
    /// a rollback, which aren't shown or heard.
    #[wasm_bindgen(method)]
    fn run(this: &Machine, input0: u16, input1: u16, present: bool);
}

#[wasm_bindgen]
pub struct Session {
    ggrs: P2PSession<Cabinet>,
    wire: Rc<RefCell<Wire>>,
    seat: usize,
    slots: i32,
}

#[wasm_bindgen]
impl Session {
    /// A session for the player in `seat` (0 or 1, also their controller port). Input delay
    /// and the rollback limit are in frames; `fps` is the game's rate.
    #[wasm_bindgen(constructor)]
    pub fn new(
        seat: usize,
        input_delay: usize,
        max_rollback: usize,
        fps: usize,
    ) -> Result<Session, JsError> {
        let wire = Rc::new(RefCell::new(Wire::default()));
        let ggrs = SessionBuilder::<Cabinet>::new()
            .with_num_players(2)?
            .with_input_delay(input_delay)
            .with_max_prediction_window(max_rollback)
            .with_fps(fps)?
            .with_desync_detection_mode(DesyncDetection::On {
                interval: DESYNC_INTERVAL as u32,
            })
            .with_disconnect_timeout(Duration::from_secs(5))
            .with_disconnect_notify_delay(Duration::from_secs(1))
            .add_player(PlayerType::Local, seat)?
            .add_player(PlayerType::Remote(PEER), 1 - seat)?
            .start_p2p_session(Socket(wire.clone()))?;
        Ok(Session {
            ggrs,
            wire,
            seat,
            // GGRS keeps max_rollback + 1 frames; one more slot so a frame it may still load
            // is never overwritten.
            slots: max_rollback as i32 + 2,
        })
    }

    /// A packet from the other player.
    pub fn receive(&self, packet: &[u8]) {
        if let Ok(message) = bincode::deserialize(packet) {
            self.wire.borrow_mut().incoming.push(message);
        }
    }

    /// Packets for the other player, sent since the last call.
    pub fn outgoing(&self) -> Array {
        std::mem::take(&mut self.wire.borrow_mut().outgoing)
            .iter()
            .map(|bytes| Uint8Array::from(bytes.as_slice()))
            .collect()
    }

    /// Handles packets that arrived and resends what the other player hasn't acknowledged.
    pub fn poll(&mut self) {
        self.ggrs.poll_remote_clients();
    }

    /// True once both players are connected and in step.
    pub fn running(&self) -> bool {
        self.ggrs.current_state() == SessionState::Running
    }

    /// How many frames this machine is ahead of the other one; slow down a little when positive.
    #[wasm_bindgen(js_name = framesAhead)]
    pub fn frames_ahead(&self) -> i32 {
        self.ggrs.frames_ahead()
    }

    /// Round trip to the other player in milliseconds, once known.
    pub fn ping(&self) -> Option<u32> {
        let stats = self.ggrs.network_stats(1 - self.seat).ok()?;
        Some(stats.ping.min(u32::MAX as u128) as u32)
    }

    /// Adds the local player's input and runs the frames GGRS asks for on `machine`. False when
    /// the other player is too far behind to keep predicting: try again on the next tick.
    pub fn advance(&mut self, input: u16, machine: &Machine) -> Result<bool, JsError> {
        self.ggrs.add_local_input(self.seat, input)?;
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
                    machine.run(inputs[0].0, inputs[1].0, Some(i) == shown);
                }
            }
        }
        Ok(true)
    }

    /// What happened since the last call, as `{ type, ... }` objects: `synchronizing` (count,
    /// total), `synchronized`, `interrupted`, `resumed`, `disconnected`, `desync` (frame).
    pub fn events(&mut self) -> Array {
        self.ggrs
            .events()
            .filter_map(|event| {
                let (kind, fields): (&str, Vec<(&str, f64)>) = match event {
                    GgrsEvent::Synchronizing { count, total, .. } => (
                        "synchronizing",
                        vec![("count", count as f64), ("total", total as f64)],
                    ),
                    GgrsEvent::Synchronized { .. } => ("synchronized", vec![]),
                    GgrsEvent::NetworkInterrupted { .. } => ("interrupted", vec![]),
                    GgrsEvent::NetworkResumed { .. } => ("resumed", vec![]),
                    GgrsEvent::Disconnected { .. } => ("disconnected", vec![]),
                    GgrsEvent::DesyncDetected { frame, .. } => {
                        ("desync", vec![("frame", frame as f64)])
                    }
                    // Pacing follows framesAhead instead.
                    GgrsEvent::WaitRecommendation { .. } => return None,
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
