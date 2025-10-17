use compose_ui_graphics::Point;

pub type PointerId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    Cancel,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary = 0,
    Secondary = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerButtons(u8);

impl PointerButtons {
    pub const NONE: Self = Self(0);

    pub fn new() -> Self {
        Self::NONE
    }

    pub fn with(mut self, button: PointerButton) -> Self {
        self.insert(button);
        self
    }

    pub fn insert(&mut self, button: PointerButton) {
        self.0 |= 1 << (button as u8);
    }

    pub fn remove(&mut self, button: PointerButton) {
        self.0 &= !(1 << (button as u8));
    }

    pub fn contains(&self, button: PointerButton) -> bool {
        (self.0 & (1 << (button as u8))) != 0
    }
}

impl Default for PointerButtons {
    fn default() -> Self {
        Self::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub id: PointerId,
    pub kind: PointerEventKind,
    pub phase: PointerPhase,
    pub position: Point,
    pub global_position: Point,
    pub buttons: PointerButtons,
}

impl PointerEvent {
    pub fn new(kind: PointerEventKind, position: Point, global_position: Point) -> Self {
        Self {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position,
            global_position,
            buttons: PointerButtons::NONE,
        }
    }
}
