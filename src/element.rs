use crate::{paint::PaintContext, utils::Size};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use taffy::{
    Dimension, Layout as TaffyLayout, NodeId as TaffyId, Size as TaffySize, Style as TaffyStyle,
    TaffyTree,
    prelude::{TaffyMaxContent, length},
};

new_key_type! {
    pub struct ElementId;
}

pub enum Element {
    Div(DivElement),
}

struct DivElement {}


struct ElementNode {
    pub content: Element,
    pub children: Vec<ElementId>,
    pub layout_style: TaffyStyle,
    pub layout: Option<TaffyLayout>,
}

impl ElementNode {
    pub fn new(content: Element) -> Self {
        Self {
            content,
            children: Vec::new(),
            layout_style: TaffyStyle::default(),
            layout: None,
        }
    }

    pub fn with_taffy_style(mut self, style: TaffyStyle) -> Self {
        self.layout_style = style;
        self
    }
}

struct ElementTree {
    nodes: SlotMap<ElementId, ElementNode>,
    taffy: TaffyTree<ElementId>,
    layouts: SecondaryMap<ElementId, TaffyId>,
    root_id: ElementId,
}

impl ElementTree {
    pub fn new(size: Size) -> Self {
        let mut nodes = SlotMap::<ElementId, ElementNode>::with_key();
        let mut taffy = TaffyTree::new();
        let mut layouts = SecondaryMap::new();

        let root = ElementNode::new(Element::Div(DivElement {}));
        let root_id = nodes.insert(root);

        let root_style = TaffyStyle {
            size: TaffySize {
                width: length(size.width as f32),
                height: length(size.height as f32),
            },
            ..Default::default()
        };

        let layout_id = taffy
            .new_leaf(root_style)
            .expect("failed to create root node");
        layouts.insert(root_id, layout_id);

        Self {
            nodes,
            taffy,
            layouts,
            root_id,
        }
    }

    pub fn insert_child(&mut self, parent_id: ElementId, child: ElementNode) -> ElementId {
        let layout_id = self
            .taffy
            .new_leaf(child.layout_style.clone())
            .expect("failed to create child node");
        let child_id = self.nodes.insert(child);
        self.layouts.insert(child_id, layout_id);

        let parent = self.nodes.get_mut(parent_id).expect("parent not found");
        parent.children.push(child_id);
        self.taffy
            .add_child(self.layouts[parent_id], layout_id)
            .expect("add layout child failed");

        child_id
    }

    pub fn do_layout(&mut self) {
        let root = self.layouts.get(self.root_id).copied().expect("root not found");
        self.taffy
            .compute_layout(root, TaffySize::MAX_CONTENT)
            .expect("compute layout failed");

        self.layout_node(self.root_id);

        self.taffy.print_tree(self.layouts[self.root_id]);
    }

    fn layout_node(&mut self, ele: ElementId) {
        let node = self.layouts.get(ele).copied().expect("root not found");

        let layout = self.taffy.layout(node).cloned().expect("get layout failed");

        self.nodes.get_mut(ele).expect("ele not found").layout = Some(layout);

        let children = self.nodes[ele].children.clone();
        for child_id in children {
            self.layout_node(child_id);
        }
    }

    pub fn do_paint(&mut self, ctx: &mut PaintContext) {

    }
}



pub trait IElement {
    fn paint(&mut self, cx: &mut PaintContext) {

    }

    fn layout_style(&mut self) -> TaffyStyle;
}


impl IElement for DivElement {
    fn paint(&mut self, cx: &mut PaintContext) {

    }

    fn layout_style(&mut self) -> TaffyStyle {
        TaffyStyle {
            size: TaffySize {
                width: length(80.0),
                height: length(60.0),
            },
            ..Default::default()
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::Size;

    #[test]
    fn test_layout() {
        let mut tree = ElementTree::new(Size::new(100, 100));

        let child_id = tree.insert_child(
            tree.root_id,
            ElementNode::new(Element::Div(DivElement {})).with_taffy_style(TaffyStyle {
                size: TaffySize {
                    width: length(80.0),
                    height: length(60.0),
                },
                ..Default::default()
            }),
        );

        tree.do_layout();
    }
}
