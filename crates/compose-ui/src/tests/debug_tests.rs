use super::*;
use crate::layout::LayoutBox;
use crate::modifier::Rect;

#[test]
fn test_count_nodes() {
    let empty_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    let root = LayoutBox {
        node_id: 0,
        rect: empty_rect,
        children: vec![
            LayoutBox {
                node_id: 1,
                rect: empty_rect,
                children: vec![],
            },
            LayoutBox {
                node_id: 2,
                rect: empty_rect,
                children: vec![],
            },
        ],
    };

    assert_eq!(count_nodes(&root), 3);
}
