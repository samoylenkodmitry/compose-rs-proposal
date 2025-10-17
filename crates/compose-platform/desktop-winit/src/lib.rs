use compose_foundation::{PointerButtons, PointerEvent, PointerEventKind, PointerPhase};
use compose_ui_graphics::Point;
use winit::dpi::PhysicalPosition;

pub struct DesktopWinitPlatform {
    scale_factor: f64,
}

impl DesktopWinitPlatform {
    pub fn new(scale_factor: f64) -> Self {
        Self { scale_factor }
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    pub fn pointer_position(&self, position: PhysicalPosition<f64>) -> Point {
        Point {
            x: (position.x / self.scale_factor) as f32,
            y: (position.y / self.scale_factor) as f32,
        }
    }

    pub fn pointer_event(
        &self,
        kind: PointerEventKind,
        position: PhysicalPosition<f64>,
    ) -> PointerEvent {
        let logical = self.pointer_position(position);
        PointerEvent {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position: logical,
            global_position: logical,
            buttons: PointerButtons::NONE,
        }
    }
}

impl Default for DesktopWinitPlatform {
    fn default() -> Self {
        Self::new(1.0)
    }
}
