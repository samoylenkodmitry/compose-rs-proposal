use super::{Color, ModOp, Modifier, RoundedCornerShape};

impl Modifier {
    pub fn background(color: Color) -> Self {
        Self::with_op(ModOp::Background(color))
    }

    pub fn rounded_corners(radius: f32) -> Self {
        Self::with_op(ModOp::RoundedCorners(RoundedCornerShape::uniform(radius)))
    }

    pub fn rounded_corner_shape(shape: RoundedCornerShape) -> Self {
        Self::with_op(ModOp::RoundedCorners(shape))
    }

    pub fn background_color(&self) -> Option<Color> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::Background(color) => Some(*color),
            _ => None,
        })
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::RoundedCorners(shape) => Some(*shape),
            _ => None,
        })
    }
}
