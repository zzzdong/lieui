use super::*;
use crate::event::EventKind;
use crate::layout::{layout, rect_of};
use crate::render::scene::{SceneBuilder, SceneOptions};
use crate::track::{Kind, KindDesc, Layer, Track};
use lieui_geom::Size;

/// B+C 档示例：自绘进度条 + 点击计数 + 生命周期计数
struct Bar {
    value: f32,
    taps: u32,
    attached: u32,
    detached: u32,
}

impl CustomNode for Bar {
    fn intrinsic_size(&self) -> Size {
        Size::new(120.0, 24.0)
    }

    fn draw(&self, out: &mut Scene, rect: Rect, tr: Affine) {
        let w = rect.width * self.value.clamp(0.0, 1.0);
        // 用一个独一无二的色值方便在场景里认出来
        fill_rect(
            out,
            Rect::new(rect.x, rect.y, w, rect.height),
            3.0,
            Color::new(10, 20, 30),
            tr,
        );
    }

    fn on_event(&mut self, id: crate::track::NodeId, ev: &EventView, cmd: &mut CmdBuf) -> bool {
        if ev.kind == EventKind::Tapped {
            self.taps += 1;
            cmd.damage(id); // 状态变了 ⇒ 自己标脏重绘
            true
        } else {
            false
        }
    }

    fn on_attached(&mut self) {
        self.attached += 1;
    }

    fn on_detached(&mut self) {
        self.detached += 1;
    }
}

fn bar(v: f32) -> CustomCell {
    cell(Bar {
        value: v,
        taps: 0,
        attached: 0,
        detached: 0,
    })
}

fn scene_of(t: &Track) -> crate::render::scene::Scene {
    SceneBuilder::new().build(
        t,
        &SceneOptions {
            window: Size::new(300.0, 200.0),
            background: Color::new(255, 255, 255),
            focus_ring: false,
            theme: crate::theme::Theme::light(),
        },
        &[],
        true,
    )
}

/// 内容根 + 一个 Custom 子节点
fn tree(v: f32) -> (Track, crate::track::NodeId) {
    let cell = bar(v);
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let node = t.create(Kind::Custom(cell), None);
    t.append_child(root, node);
    (t, node)
}

#[test]
fn draw_pushes_user_ops_with_the_node_rect() {
    let (mut t, node) = tree(0.5);
    layout(&mut t, Size::new(300.0, 200.0));
    let r = rect_of(&t, node);

    let scene = scene_of(&t);
    let mine = scene
        .ops()
        .iter()
        .filter_map(|op| match op {
            crate::render::scene::Op::Rect { rect, color, .. } if (color.r, color.g, color.b) == (10, 20, 30) => {
                Some(*rect)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(mine.len(), 1, "用户画的那块出现在场景里");
    assert!((mine[0].width - r.width * 0.5).abs() < 0.5, "宽度按 value 截取");
    assert_eq!(mine[0].x, r.x);
}

#[test]
fn intrinsic_size_drives_layout_when_dims_are_undefined() {
    let (mut t, node) = tree(0.5);
    // 关掉父的交叉轴拉伸（align_items 设在**父**上，约束的是孩子），
    // 让固有尺寸真正决定节点的两个轴
    let root = t.roots()[0].node;
    t.get_mut(root).unwrap().layout.align_items = lieui_layout::FlexAlign::Start;
    layout(&mut t, Size::new(300.0, 200.0));
    let r = rect_of(&t, node);
    assert!((r.height - 24.0).abs() < 0.5, "固有高度生效：{r:?}");
    assert!((r.width - 120.0).abs() < 0.5, "固有宽度生效：{r:?}");
}

#[test]
fn same_cell_is_stable_across_aligns_and_shared() {
    let cell = bar(0.5);
    let desc = KindDesc::Custom(cell.clone());

    let mut kind = Kind::Custom(cell.clone());
    assert!(!desc.apply_to(&mut kind), "同一个 cell = 没变（实例跨帧保留的前提）");

    // 两个节点共用一个 cell ⇒ 状态共享：改 value，绘制结果跟着变
    let concrete = std::rc::Rc::new(std::cell::RefCell::new(Bar {
        value: 0.5,
        taps: 0,
        attached: 0,
        detached: 0,
    }));
    let shared = CustomCell::from_rc(concrete.clone());
    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let a = t.create(Kind::Custom(shared.clone()), None);
    let b = t.create(Kind::Custom(shared.clone()), None);
    t.append_child(root, a);
    t.append_child(root, b);
    concrete.borrow_mut().value = 0.25;
    layout(&mut t, Size::new(300.0, 200.0));

    let scene = scene_of(&t);
    let widths: Vec<f32> = scene
        .ops()
        .iter()
        .filter_map(|op| match op {
            crate::render::scene::Op::Rect { rect, color, .. } if (color.r, color.g, color.b) == (10, 20, 30) => {
                Some(rect.width)
            }
            _ => None,
        })
        .collect();
    assert_eq!(widths.len(), 2, "两个节点都画了");
    // 节点在父列里被拉伸到 300 宽（flex 语义），条宽 = 300 × 0.25
    assert!(
        widths.iter().all(|w| (*w - 75.0).abs() < 0.5),
        "状态共享：两个实例都画出 0.25：{widths:?}"
    );
}

#[test]
fn a_different_cell_counts_as_a_change() {
    let cell = bar(0.5);
    let mut kind = Kind::Custom(cell.clone());
    assert!(KindDesc::Custom(bar(0.8)).apply_to(&mut kind), "换 cell = 数据变了");
}

#[test]
fn attach_and_detach_hooks_fire() {
    let concrete = std::rc::Rc::new(std::cell::RefCell::new(Bar {
        value: 1.0,
        taps: 0,
        attached: 0,
        detached: 0,
    }));
    let wrapper = CustomCell::from_rc(concrete.clone());

    let mut t = Track::new();
    let root = t.create(Kind::Box, None);
    t.add_root(Layer::Content, None, root);
    let n = t.create(Kind::Custom(wrapper), None);
    t.append_child(root, n);
    assert_eq!(concrete.borrow().attached, 1, "create ⇒ on_attached");

    t.destroy(n);
    assert_eq!(concrete.borrow().detached, 1, "destroy ⇒ on_detached");
}

#[test]
fn events_reach_the_custom_node_through_dispatch() {
    use crate::app::{App, WindowConfig};
    use crate::reactive::Runtime;
    use std::rc::Rc;

    struct Vm {
        bar: CustomCell,
    }
    impl crate::app::ViewModel for Vm {
        fn view(self: &Rc<Self>, v: &mut crate::view::ViewBuf) {
            v.custom(&self.bar);
        }
    }

    let rt = Runtime::new();
    let concrete = std::rc::Rc::new(std::cell::RefCell::new(Bar {
        value: 0.5,
        taps: 0,
        attached: 0,
        detached: 0,
    }));
    let vm = Rc::new(Vm {
        bar: CustomCell::from_rc(concrete.clone()),
    });
    let mut app = App::new(rt.clone());
    let id = app.window_erased(
        WindowConfig::new().size(300.0, 200.0),
        crate::app::erased(Rc::clone(&vm)),
    );
    app.frame_all();

    let node = app
        .window_ctx(id)
        .unwrap()
        .track()
        .roots_of(Layer::Content)
        .next()
        .unwrap()
        .node;
    app.window_ctx_mut(id)
        .unwrap()
        .dispatch(&rt, &[node], &crate::event::Event::simple(EventKind::Tapped));
    assert_eq!(concrete.borrow().taps, 1, "Tapped 到达自定义状态机");

    // view 重跑（无关状态变化）后实例还在：状态不丢
    app.frame_all();
    assert_eq!(concrete.borrow().taps, 1);
    assert_eq!(concrete.borrow().attached, 1, "实例没有被重建");
}
