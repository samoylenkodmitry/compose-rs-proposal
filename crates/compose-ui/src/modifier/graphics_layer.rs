use super::{GraphicsLayer, Modifier};

impl Modifier {
    pub fn graphics_layer(layer: GraphicsLayer) -> Self {
        Self::with_state(move |state| {
            state.graphics_layer = Some(layer);
        })
    }
}
