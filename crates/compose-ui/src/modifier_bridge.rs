use compose_foundation::{
    modifier_element, BasicModifierNodeContext, DynModifierElement, ModifierNodeChain,
};

use crate::modifier::{DrawCommand, LayoutProperties, ModOp, Modifier, Point, RoundedCornerShape};
use crate::modifier_nodes::{BackgroundElement, ClickableElement, PaddingElement, SizeElement};

/// Snapshot of layout-relevant modifier state produced by the legacy ModOp system.
pub(crate) struct LegacyLayoutSnapshot {
    pub properties: LayoutProperties,
    pub offset: Point,
}

/// Snapshot of draw-relevant modifier state produced by the legacy ModOp system.
pub(crate) struct LegacyDrawSnapshot {
    pub background: Option<crate::modifier::Color>,
    pub corner_shape: Option<RoundedCornerShape>,
    pub commands: Vec<DrawCommand>,
}

/// Extension trait that bridges value-based [`Modifier`]s with [`ModifierNodeChain`].
pub(crate) trait ModifierNodeChainExt {
    /// Reconcile the chain against the current modifier.
    fn sync_from_modifier(&mut self, context: &mut BasicModifierNodeContext, modifier: &Modifier);

    /// Produce a layout snapshot for the provided modifier while keeping the chain in sync.
    fn measure(
        &mut self,
        context: &mut BasicModifierNodeContext,
        modifier: &Modifier,
    ) -> LegacyLayoutSnapshot;

    /// Produce draw information for the provided modifier while keeping the chain in sync.
    fn draw(
        &mut self,
        context: &mut BasicModifierNodeContext,
        modifier: &Modifier,
    ) -> LegacyDrawSnapshot;
}

impl ModifierNodeChainExt for ModifierNodeChain {
    fn sync_from_modifier(&mut self, context: &mut BasicModifierNodeContext, modifier: &Modifier) {
        let elements = elements_from_modifier(modifier);
        if elements.is_empty() {
            self.detach_all();
            return;
        }
        self.update_from_slice(&elements, context);
    }

    fn measure(
        &mut self,
        context: &mut BasicModifierNodeContext,
        modifier: &Modifier,
    ) -> LegacyLayoutSnapshot {
        self.sync_from_modifier(context, modifier);
        LegacyLayoutSnapshot {
            properties: modifier.layout_properties(),
            offset: modifier.total_offset(),
        }
    }

    fn draw(
        &mut self,
        context: &mut BasicModifierNodeContext,
        modifier: &Modifier,
    ) -> LegacyDrawSnapshot {
        self.sync_from_modifier(context, modifier);
        LegacyDrawSnapshot {
            background: modifier.background_color(),
            corner_shape: modifier.corner_shape(),
            commands: modifier.draw_commands(),
        }
    }
}

/// Build a modifier node chain from a value-based [`Modifier`].
pub(crate) fn build_chain(modifier: &Modifier) -> ModifierNodeChain {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.sync_from_modifier(&mut context, modifier);
    chain
}

fn elements_from_modifier(modifier: &Modifier) -> Vec<DynModifierElement> {
    modifier.ops().iter().filter_map(element_from_op).collect()
}

fn element_from_op(op: &ModOp) -> Option<DynModifierElement> {
    match op {
        ModOp::Padding(padding) => Some(modifier_element(PaddingElement::new(*padding))),
        ModOp::Background(color) => Some(modifier_element(BackgroundElement::new(*color))),
        ModOp::Clickable(handler) => Some(modifier_element(ClickableElement::with_handler(
            handler.clone(),
        ))),
        ModOp::Size(size) => Some(modifier_element(SizeElement::new(
            Some(size.width),
            Some(size.height),
        ))),
        ModOp::Width(width) => Some(modifier_element(SizeElement::new(Some(*width), None))),
        ModOp::Height(height) => Some(modifier_element(SizeElement::new(None, Some(*height)))),
        _ => None,
    }
}
