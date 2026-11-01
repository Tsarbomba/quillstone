//! Gamepads (with the `gamepad` feature): buttons become the keys the game
//! already understands, so every screen works with a pad the way it does with
//! a keyboard. Names are Xbox ones; other pads map by position.
//!
//! | Button | Does |
//! | --- | --- |
//! | A | advance / confirm (Enter) |
//! | B, Start | back, or the save screen (with the game-menu tabs) in the story (Esc) |
//! | X | roll back |
//! | Y | history |
//! | LB | auto-forward on/off |
//! | RB | skip on/off |
//! | RT (hold) | skip while held |
//! | LT | load screen |
//! | Back / Select | save screen |
//! | D-pad, left stick | arrows |

// Without the feature only the (tested) mapping is left.
#![cfg_attr(not(feature = "gamepad"), allow(dead_code))]

use quillstone_ui::{Event, Key};

/// The game event for a button going down (`pressed`) or up; `None` for buttons
/// that do nothing. Only the hold-to-skip trigger cares about letting go.
fn button_event(button: Pad, pressed: bool) -> Option<Event> {
    if !pressed {
        return (button == Pad::RightTrigger2).then_some(Event::KeyReleased(Key::Ctrl));
    }
    let key = match button {
        Pad::South => Key::Enter,
        Pad::East | Pad::Start => Key::Escape,
        Pad::West => Key::Backspace,
        Pad::North => Key::Char('h'),
        Pad::LeftTrigger => Key::Char('a'),
        Pad::RightTrigger => Key::Tab,
        Pad::RightTrigger2 => Key::Ctrl,
        Pad::LeftTrigger2 => Key::Char('l'),
        Pad::Select => Key::Char('s'),
        Pad::DPadUp => Key::Up,
        Pad::DPadDown => Key::Down,
        Pad::DPadLeft => Key::Left,
        Pad::DPadRight => Key::Right,
        _ => return None,
    };
    Some(Event::Key(key))
}

/// What an unplugged pad leaves behind: gilrs sends no button releases then, so a held RT would keep
/// skip on. Returns the release to send, and lets go of the stick.
fn pad_gone(stick: &mut (i8, i8)) -> Event {
    *stick = (0, 0);
    Event::KeyReleased(Key::Ctrl)
}

/// How far the stick must lean to count as a press, and back under which it
/// counts as let go (the gap keeps a wobbly stick from sending a stream of presses).
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.35;

/// Where a stick axis is, as -1, 0 or 1, given where it was.
fn stick_step(value: f32, was: i8) -> i8 {
    match was {
        0 if value >= STICK_ON => 1,
        0 if value <= -STICK_ON => -1,
        1 | -1 if value.abs() < STICK_OFF => 0,
        _ => was,
    }
}

#[cfg(feature = "gamepad")]
use gilrs::Button as Pad;

/// Stand-in so the mapping above compiles (and is tested) without gilrs.
#[cfg(not(feature = "gamepad"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pad {
    South, East, North, West, LeftTrigger, LeftTrigger2, RightTrigger, RightTrigger2,
    Select, Start, Mode, DPadUp, DPadDown, DPadLeft, DPadRight,
}

pub struct Gamepads {
    #[cfg(feature = "gamepad")]
    gilrs: Option<gilrs::Gilrs>,
    /// The left stick's x and y as -1, 0 or 1 (see [`stick_step`]).
    stick: (i8, i8),
}

impl Gamepads {
    pub fn new() -> Gamepads {
        #[cfg(feature = "gamepad")]
        let gilrs = gilrs::Gilrs::new().map_err(|e| eprintln!("warning: no gamepad support ({e})")).ok();
        Gamepads {
            #[cfg(feature = "gamepad")]
            gilrs,
            stick: (0, 0),
        }
    }

    /// What the pads did since the last call, as game events.
    #[cfg(feature = "gamepad")]
    pub fn poll(&mut self) -> Vec<Event> {
        use gilrs::{Axis, EventType};
        let mut out = Vec::new();
        let Some(gilrs) = &mut self.gilrs else { return out };
        while let Some(ev) = gilrs.next_event() {
            match ev.event {
                EventType::ButtonPressed(b, _) => out.extend(button_event(b, true)),
                EventType::ButtonReleased(b, _) => out.extend(button_event(b, false)),
                EventType::AxisChanged(Axis::LeftStickX, v, _) => {
                    let now = stick_step(v, self.stick.0);
                    if now != self.stick.0 && now != 0 {
                        out.push(Event::Key(if now > 0 { Key::Right } else { Key::Left }));
                    }
                    self.stick.0 = now;
                }
                EventType::AxisChanged(Axis::LeftStickY, v, _) => {
                    // gilrs: up is positive.
                    let now = stick_step(v, self.stick.1);
                    if now != self.stick.1 && now != 0 {
                        out.push(Event::Key(if now > 0 { Key::Up } else { Key::Down }));
                    }
                    self.stick.1 = now;
                }
                EventType::Disconnected => out.push(pad_gone(&mut self.stick)),
                _ => {}
            }
        }
        out
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn poll(&mut self) -> Vec<Event> {
        Vec::new()
    }

    /// Whether a pad is plugged in (then it is polled often, even when the screen is still).
    pub fn any_connected(&self) -> bool {
        #[cfg(feature = "gamepad")]
        if let Some(gilrs) = &self.gilrs {
            return gilrs.gamepads().next().is_some();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_map_to_keys_and_only_the_skip_trigger_cares_about_release() {
        assert_eq!(button_event(Pad::South, true), Some(Event::Key(Key::Enter)));
        assert_eq!(button_event(Pad::Start, true), Some(Event::Key(Key::Escape)));
        assert_eq!(button_event(Pad::DPadUp, true), Some(Event::Key(Key::Up)));
        assert_eq!(button_event(Pad::RightTrigger2, true), Some(Event::Key(Key::Ctrl)));
        assert_eq!(button_event(Pad::RightTrigger2, false), Some(Event::KeyReleased(Key::Ctrl)));
        assert_eq!(button_event(Pad::South, false), None);
        assert_eq!(button_event(Pad::Mode, true), None);
    }

    #[test]
    fn the_stick_presses_once_per_push() {
        let mut at = 0;
        let mut presses = 0;
        for v in [0.2, 0.7, 0.9, 0.5, 0.8, 0.3, 0.7] {
            let now = stick_step(v, at);
            presses += i32::from(now != at && now != 0);
            at = now;
        }
        assert_eq!(presses, 2, "0.5 isn't back to rest, 0.3 is");
    }

    #[test]
    fn unplugging_a_pad_lets_go_of_skip_and_the_stick() {
        let mut stick = (1, -1);
        assert_eq!(pad_gone(&mut stick), Event::KeyReleased(Key::Ctrl));
        assert_eq!(stick, (0, 0));
    }
}
