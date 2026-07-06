//! Translate mouse actions into SGR (1006) mouse-reporting escape sequences.
//!
//! The target application must have enabled mouse reporting (e.g. `ESC[?1000h`
//! / `?1006h`) for these to have an effect. Most full-screen TUIs do.

use crate::keys::Mods;

#[derive(Clone, Copy)]
pub enum MouseAction {
    Left,
    Right,
    Middle,
    ScrollUp,
    ScrollDown,
    /// Button press without release (for building drags manually).
    Down(Button),
    /// Button release.
    Up(Button),
    /// Pointer motion with a button held (drag step).
    Move(Button),
    /// Pointer motion with no button held.
    Hover,
}

#[derive(Clone, Copy)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    fn base(self) -> u8 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
        }
    }
}

pub fn parse_action(name: &str) -> Option<MouseAction> {
    Some(match name.to_ascii_lowercase().as_str() {
        "left" | "click" => MouseAction::Left,
        "right" => MouseAction::Right,
        "middle" => MouseAction::Middle,
        "scroll_up" | "scrollup" | "wheel_up" => MouseAction::ScrollUp,
        "scroll_down" | "scrolldown" | "wheel_down" => MouseAction::ScrollDown,
        "down" | "press" => MouseAction::Down(Button::Left),
        "right_down" => MouseAction::Down(Button::Right),
        "middle_down" => MouseAction::Down(Button::Middle),
        "up" | "release" => MouseAction::Up(Button::Left),
        "right_up" => MouseAction::Up(Button::Right),
        "middle_up" => MouseAction::Up(Button::Middle),
        "move" | "drag" => MouseAction::Move(Button::Left),
        "hover" => MouseAction::Hover,
        _ => return None,
    })
}

fn modifier_bits(mods: Mods) -> u8 {
    let mut b = 0;
    if mods.shift {
        b |= 4;
    }
    if mods.alt {
        b |= 8;
    }
    if mods.ctrl {
        b |= 16;
    }
    b
}

/// Build the SGR sequence(s) for an action at 1-based column/row `(x, y)`.
/// A full left-click is press + release, hence a `Vec` of sequences.
pub fn action_to_bytes(action: MouseAction, x: u16, y: u16, mods: Mods) -> Vec<Vec<u8>> {
    let m = modifier_bits(mods);
    let press = |cb: u8, release: bool| {
        let final_byte = if release { 'm' } else { 'M' };
        format!("\x1b[<{};{};{}{}", cb | m, x, y, final_byte).into_bytes()
    };

    match action {
        MouseAction::Left => vec![press(0, false), press(0, true)],
        MouseAction::Middle => vec![press(1, false), press(1, true)],
        MouseAction::Right => vec![press(2, false), press(2, true)],
        MouseAction::ScrollUp => vec![press(64, false)],
        MouseAction::ScrollDown => vec![press(65, false)],
        MouseAction::Down(b) => vec![press(b.base(), false)],
        MouseAction::Up(b) => vec![press(b.base(), true)],
        // Motion events set bit 5 (32).
        MouseAction::Move(b) => vec![press(32 | b.base(), false)],
        MouseAction::Hover => vec![press(32 | 3, false)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Mods;

    #[test]
    fn left_click_is_press_then_release() {
        let seqs = action_to_bytes(MouseAction::Left, 3, 4, Mods::default());
        assert_eq!(seqs.len(), 2);
        assert_eq!(seqs[0], b"\x1b[<0;3;4M");
        assert_eq!(seqs[1], b"\x1b[<0;3;4m");
    }

    #[test]
    fn right_click_uses_button_2() {
        let seqs = action_to_bytes(MouseAction::Right, 1, 1, Mods::default());
        assert_eq!(seqs[0], b"\x1b[<2;1;1M");
    }

    #[test]
    fn scroll_up_is_button_64() {
        let seqs = action_to_bytes(MouseAction::ScrollUp, 5, 6, Mods::default());
        assert_eq!(seqs, vec![b"\x1b[<64;5;6M".to_vec()]);
    }

    #[test]
    fn modifiers_are_added_to_button_code() {
        // ctrl = 16, so a left press becomes button 0 | 16 = 16.
        let mods = Mods {
            shift: false,
            alt: false,
            ctrl: true,
        };
        let seqs = action_to_bytes(MouseAction::Down(Button::Left), 2, 2, mods);
        assert_eq!(seqs[0], b"\x1b[<16;2;2M");
    }

    #[test]
    fn parses_action_names() {
        assert!(matches!(parse_action("left"), Some(MouseAction::Left)));
        assert!(matches!(
            parse_action("scroll_down"),
            Some(MouseAction::ScrollDown)
        ));
        assert!(matches!(parse_action("drag"), Some(MouseAction::Move(_))));
        assert!(parse_action("nonsense").is_none());
    }
}
