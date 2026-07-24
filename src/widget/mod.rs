//! Widget — 实现 `View` trait 的 UI 组件
//!
//! `ViewNode` 本身已是框架唯一的原语（纯数据枚举）；此处的所有组件
//! （Text、Image、Container、Column、Row、Button、Checkbox、Divider、ListView）
//! 都是基于 `ViewNode` 的上层封装，由 `build()` 产出 `ViewNode`。

pub mod button;
pub mod checkbox;
pub mod container;
pub mod divider;
pub mod flex;
pub mod image;
pub mod list_view;
pub mod text;

pub use button::Button;
pub use checkbox::Checkbox;
pub use container::Container;
pub use divider::Divider;
pub use flex::{Column, Row};
pub use image::Image;
pub use list_view::ListView;
pub use text::Text;
