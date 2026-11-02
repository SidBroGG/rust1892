use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

pub fn is_overlay_toggle(event: &KeyEvent) -> bool {
    is_overlay_key(event.state, event.repeat, event.physical_key)
}

fn is_overlay_key(state: ElementState, repeat: bool, physical_key: PhysicalKey) -> bool {
    state == ElementState::Pressed
        && !repeat
        && matches!(physical_key, PhysicalKey::Code(KeyCode::F3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::KeyCode;

    #[test]
    fn f3_press_toggles() {
        assert!(is_overlay_key(
            ElementState::Pressed,
            false,
            PhysicalKey::Code(KeyCode::F3)
        ));
    }

    #[test]
    fn f3_repeat_does_not_toggle() {
        assert!(!is_overlay_key(
            ElementState::Pressed,
            true,
            PhysicalKey::Code(KeyCode::F3)
        ));
    }

    #[test]
    fn other_keys_do_not_toggle() {
        assert!(!is_overlay_key(
            ElementState::Pressed,
            false,
            PhysicalKey::Code(KeyCode::F4)
        ));
        assert!(!is_overlay_key(
            ElementState::Released,
            false,
            PhysicalKey::Code(KeyCode::F3)
        ));
    }
}
