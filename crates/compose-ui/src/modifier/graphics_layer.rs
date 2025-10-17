use super::{GraphicsLayer, ModOp, Modifier};

impl Modifier {
    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_op(ModOp::GraphicsLayer(layer))
    }

    pub fn graphics_layer_values(&self) -> Option<GraphicsLayer> {
        self.0.iter().rev().find_map(|op| match op {
            ModOp::GraphicsLayer(layer) => Some(*layer),
            _ => None,
        })
    }
}
