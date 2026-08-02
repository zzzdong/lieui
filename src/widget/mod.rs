//! Widget — 位于 ViewNode 之上的组件抽象
//!
//! Widget 是可复用的 UI 配置对象，每次 rebuild 都会重新生成，但状态通过
//! `BuildContext::use_state` 持久化。`Widget::build` 产出 immutable 的 `ViewNode`。

pub mod button;
pub mod card;
pub mod checkbox;
pub mod container;
pub mod divider;
pub mod draggable;
pub mod flex;
pub mod icon;
pub mod image;
pub mod input;
pub mod layout;
pub mod list_view;
pub mod progress;
pub mod radio;
pub mod scroll_bar;
pub mod scroll_view;
pub mod slider;
pub mod switch;
pub mod tab;
pub mod text;
pub mod tooltip;
pub mod virtual_list;

pub use button::{Button, ButtonSize, ButtonVariant};
pub use card::{Card, CardVariant};
pub use checkbox::Checkbox;
pub use container::Container;
pub use divider::Divider;
pub use draggable::Draggable;
pub use flex::{Column, Row};
pub use icon::{Icon, IconButton, IconButtonVariant, IconName};
pub use image::Image;
pub use input::{Input, InputSize, InputStatus};
pub use layout::LayoutAttr;
pub use list_view::ListView;
pub use progress::Progress;
pub use radio::Radio;
pub use scroll_bar::{ScrollBar, ScrollOrientation};
pub use scroll_view::ScrollView;
pub use slider::Slider;
pub use switch::Switch;
pub use tab::Tab;
pub use text::Text;
pub use tooltip::Tooltip;
pub use virtual_list::VirtualList;

use crate::view::node::ViewNode;
use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

/// Widget 层状态存储。
pub type StateMap = HashMap<String, Box<dyn Any>>;

/// Widget trait。
///
/// 所有组件（内置和自定义）都实现此 trait，通过 `build()` 把自身描述转换为
/// `ViewNode` 原语组合。`key` 用于在 rebuild 时稳定识别 StatefulWidget 实例。
pub trait Widget {
    /// 可选的稳定 key。
    fn key(&self) -> Option<&str> {
        None
    }

    /// 在指定构建上下文中生成 `ViewNode`。
    fn build(&self, ctx: &mut BuildContext) -> ViewNode;

    /// 无状态快捷构建（不经过 BuildContext 的状态管理）。
    ///
    /// 适用于纯展示型组件或由外部 `State<T>` 驱动的 builder 闭包。
    fn build_node(&self) -> ViewNode {
        self.build(&mut BuildContext::empty())
    }

    /// Inspector 支持：返回该 widget 的文本表示（用于 DevTools 显示）。
    ///
    /// 默认返回 `None`；文本类 widget（如 `Text`）可 override 暴露内容。
    /// 仅在 `inspector` feature 下有意义，但作为 trait 方法常驻以保证 API 稳定。
    fn inspect_text(&self) -> Option<String> {
        None
    }

    /// Inspector 支持：返回该 widget 的**具体类型名**（用于 DevTools 显示）。
    ///
    /// 默认基于 `std::any::type_name_of_val(self)` 截短末段（如 `my_module::Button`
    /// -> `Button`）。由于 `self` 是具体类型，`type_name_of_val` 能拿到真实类型名，
    /// 不会退化成 `dyn Widget`。绝大多数 widget 无需 override；如需在 DevTools 中显示
    /// 自定义别名（如匿名组合组件）可重写此方法。
    fn inspect_name(&self) -> String {
        let full = std::any::type_name_of_val(self);
        match full.rsplit("::").next() {
            Some(s) => s.to_string(),
            None => full.to_string(),
        }
    }
}

/// Widget 构建上下文。
///
/// 负责维护当前 Widget 在树中的路径，以及 `use_state` 的持久化。
pub struct BuildContext {
    path: Vec<String>,
    hook_index: usize,
    state: Rc<RefCell<StateMap>>,
    /// Inspector 描述树收集器（仅在开启 inspector feature 时 Some）
    #[cfg(feature = "inspector")]
    inspect: Option<crate::inspector::WidgetDescBuilder>,
}

impl BuildContext {
    pub fn new(state: Rc<RefCell<StateMap>>) -> Self {
        Self {
            path: Vec::new(),
            hook_index: 0,
            state,
            #[cfg(feature = "inspector")]
            inspect: None,
        }
    }

    /// 创建一个不管理任何状态的空白上下文。
    pub fn empty() -> Self {
        Self {
            path: Vec::new(),
            hook_index: 0,
            state: Rc::new(RefCell::new(StateMap::new())),
            #[cfg(feature = "inspector")]
            inspect: None,
        }
    }

    /// 启用 Inspector 描述树收集：把根 widget 的描述节点挂到收集器。
    #[cfg(feature = "inspector")]
    pub(crate) fn begin_inspect(&mut self, root: Rc<RefCell<crate::inspector::WidgetDescNode>>) {
        self.inspect = Some(crate::inspector::WidgetDescBuilder {
            node: Rc::clone(&root),
            stack: vec![root],
        });
    }

    /// 取回已冻结的 widget 描述树（构建结束后调用）
    #[cfg(feature = "inspector")]
    pub(crate) fn finish_inspect(&mut self) -> Option<crate::inspector::WidgetDesc> {
        let b = self.inspect.take()?;
        Some(b.node.borrow().freeze())
    }

    /// 构建一个子 Widget，并自动维护路径。
    pub fn child(&mut self, index: usize, widget: &dyn Widget) -> ViewNode {
        let saved_hook = self.hook_index;
        self.hook_index = 0;

        let segment = widget
            .key()
            .map(|k| k.to_string())
            .unwrap_or_else(|| index.to_string());
        self.path.push(segment);

        // ---- Inspector：在构建子节点前，把该 widget 挂到描述树 ----
        #[cfg(feature = "inspector")]
        {
            if let Some(b) = self.inspect.as_mut() {
                use crate::inspector::WidgetDescNode;
                let name = widget.inspect_name();
                let path = self.path.join("/");
                let node = Rc::new(RefCell::new(WidgetDescNode {
                    name,
                    path: path.clone(),
                    text: widget.inspect_text(),
                    children: Vec::new(),
                }));
                // 挂到当前栈顶，并把该节点压栈（后续 build 内的 child 归它）
                if let Some(top) = b.stack.last() {
                    top.borrow_mut().children.push(Rc::clone(&node));
                }
                b.stack.push(node);
            }
        }

        let node = widget.build(self);

        // ---- Inspector：子节点构建完毕，弹栈 ----
        #[cfg(feature = "inspector")]
        {
            if let Some(b) = self.inspect.as_mut() {
                b.stack.pop();
            }
        }

        self.path.pop();
        self.hook_index = saved_hook;
        node
    }

    /// 获取或初始化一个状态钩子。
    pub fn use_state<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Stateful<T> {
        let key = format!("{}#{}", self.path.join("/"), self.hook_index);
        self.hook_index += 1;

        if !self.state.borrow().contains_key(&key) {
            self.state
                .borrow_mut()
                .insert(key.clone(), Box::new(init()));
        }

        Stateful {
            key,
            state: Rc::clone(&self.state),
            _marker: PhantomData,
        }
    }
}

/// 通过 `BuildContext::use_state` 获得的持久化状态。
pub struct Stateful<T> {
    key: String,
    state: Rc<RefCell<StateMap>>,
    _marker: PhantomData<T>,
}

impl<T: 'static> Stateful<T> {
    pub fn get(&self) -> Ref<'_, T> {
        Ref::map(self.state.borrow(), |m| {
            m.get(&self.key)
                .expect("Stateful state missing")
                .downcast_ref::<T>()
                .expect("Stateful type mismatch")
        })
    }

    pub fn get_mut(&self) -> RefMut<'_, T> {
        RefMut::map(self.state.borrow_mut(), |m| {
            m.get_mut(&self.key)
                .expect("Stateful state missing")
                .downcast_mut::<T>()
                .expect("Stateful type mismatch")
        })
    }

    pub fn set(&self, value: T) {
        *self.get_mut() = value;
        crate::state::request_rebuild();
    }

    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        f(&mut *self.get_mut());
        crate::state::request_rebuild();
    }
}

impl<T: 'static> Clone for Stateful<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            state: Rc::clone(&self.state),
            _marker: PhantomData,
        }
    }
}
