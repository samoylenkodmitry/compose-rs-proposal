//! Focus management stubs for pointer interactions.

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusId(pub(crate) usize);

#[derive(Default)]
pub struct FocusManager;

impl FocusManager {
    pub fn new() -> Self {
        Self
    }

    pub fn request_focus(&mut self, _id: FocusId) {}

    pub fn clear_focus(&mut self) {}
}
