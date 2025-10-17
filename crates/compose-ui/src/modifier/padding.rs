use super::{EdgeInsets, ModOp, Modifier};

impl Modifier {
    pub fn padding(p: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::uniform(p)))
    }

    pub fn padding_horizontal(horizontal: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::horizontal(horizontal)))
    }

    pub fn padding_vertical(vertical: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::vertical(vertical)))
    }

    pub fn padding_symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::symmetric(horizontal, vertical)))
    }

    pub fn padding_each(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::with_op(ModOp::Padding(EdgeInsets::from_components(
            left, top, right, bottom,
        )))
    }
}
