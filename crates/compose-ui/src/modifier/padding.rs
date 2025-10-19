use super::{EdgeInsets, Modifier};
use crate::modifier_nodes::PaddingElement;

impl Modifier {
    pub fn padding(p: f32) -> Self {
        let padding = EdgeInsets::uniform(p);
        Self::with_element(PaddingElement::new(padding), move |state| {
            state.layout.padding += padding;
        })
    }

    pub fn padding_horizontal(horizontal: f32) -> Self {
        let padding = EdgeInsets::horizontal(horizontal);
        Self::with_element(PaddingElement::new(padding), move |state| {
            state.layout.padding += padding;
        })
    }

    pub fn padding_vertical(vertical: f32) -> Self {
        let padding = EdgeInsets::vertical(vertical);
        Self::with_element(PaddingElement::new(padding), move |state| {
            state.layout.padding += padding;
        })
    }

    pub fn padding_symmetric(horizontal: f32, vertical: f32) -> Self {
        let padding = EdgeInsets::symmetric(horizontal, vertical);
        Self::with_element(PaddingElement::new(padding), move |state| {
            state.layout.padding += padding;
        })
    }

    pub fn padding_each(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        let padding = EdgeInsets::from_components(left, top, right, bottom);
        Self::with_element(PaddingElement::new(padding), move |state| {
            state.layout.padding += padding;
        })
    }
}
