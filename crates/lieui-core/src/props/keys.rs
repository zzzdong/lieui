//! 内置属性键常量表
//!
//! 槽位号是**稳定 ABI**：一旦发布不得复用或重排（主题 RON、调试工具都按号寻址）。
//! 设计 §4.5 把这张表放在 `lieui-widgets`；M1 阶段内核的布局/文本阶段就要用，
//! 因此先定义在内核，`lieui-widgets` 建立后从这里重导出。
//!
//! 枚举类属性统一用 `u32` 承载（`PropValue` 无枚举变体），取值常量见各 `pub const`。

use super::key::{PropFlags, PropKey};
use super::value::{Color, Dimension, SharedString};

const L: PropFlags = PropFlags::AFFECT_LAYOUT;
const P: PropFlags = PropFlags::AFFECT_PAINT;
const INH: PropFlags = PropFlags::INHERITABLE;

// ── 盒模型（0..19）──
pub const WIDTH: PropKey<Dimension> = PropKey::new(0, L);
pub const HEIGHT: PropKey<Dimension> = PropKey::new(1, L);
pub const MIN_WIDTH: PropKey<f32> = PropKey::new(2, L);
pub const MIN_HEIGHT: PropKey<f32> = PropKey::new(3, L);
pub const MAX_WIDTH: PropKey<f32> = PropKey::new(4, L);
pub const MAX_HEIGHT: PropKey<f32> = PropKey::new(5, L);
pub const PADDING_L: PropKey<f32> = PropKey::new(6, L);
pub const PADDING_T: PropKey<f32> = PropKey::new(7, L);
pub const PADDING_R: PropKey<f32> = PropKey::new(8, L);
pub const PADDING_B: PropKey<f32> = PropKey::new(9, L);
pub const MARGIN_L: PropKey<f32> = PropKey::new(10, L);
pub const MARGIN_T: PropKey<f32> = PropKey::new(11, L);
pub const MARGIN_R: PropKey<f32> = PropKey::new(12, L);
pub const MARGIN_B: PropKey<f32> = PropKey::new(13, L);
pub const BORDER_L: PropKey<f32> = PropKey::new(14, L);
pub const BORDER_T: PropKey<f32> = PropKey::new(15, L);
pub const BORDER_R: PropKey<f32> = PropKey::new(16, L);
pub const BORDER_B: PropKey<f32> = PropKey::new(17, L);

// ── Flex（20..39）──
pub const FLEX_DIRECTION: PropKey<u32> = PropKey::new(20, L);
pub const FLEX_WRAP: PropKey<u32> = PropKey::new(21, L);
pub const JUSTIFY_CONTENT: PropKey<u32> = PropKey::new(22, L);
pub const ALIGN_ITEMS: PropKey<u32> = PropKey::new(23, L);
pub const ALIGN_SELF: PropKey<u32> = PropKey::new(24, L);
pub const ALIGN_CONTENT: PropKey<u32> = PropKey::new(25, L);
pub const FLEX_GROW: PropKey<f32> = PropKey::new(26, L);
pub const FLEX_SHRINK: PropKey<f32> = PropKey::new(27, L);
pub const FLEX_BASIS: PropKey<f32> = PropKey::new(28, L);
/// 主轴 item 间距
pub const GAP: PropKey<f32> = PropKey::new(29, L);
/// 交叉轴行间距
pub const LINE_GAP: PropKey<f32> = PropKey::new(30, L);
pub const POSITION_TYPE: PropKey<u32> = PropKey::new(31, L);
pub const LEFT: PropKey<f32> = PropKey::new(32, L);
pub const TOP: PropKey<f32> = PropKey::new(33, L);
pub const RIGHT: PropKey<f32> = PropKey::new(34, L);
pub const BOTTOM: PropKey<f32> = PropKey::new(35, L);
pub const DISPLAY: PropKey<u32> = PropKey::new(36, L);

// ── 滚动（40..49）──
pub const OVERFLOW_SCROLL: PropKey<bool> = PropKey::new(40, L);
/// 显式内容宽（虚拟化列表：真实内容远大于实际子节点）
pub const CONTENT_WIDTH: PropKey<f32> = PropKey::new(41, L);
pub const CONTENT_HEIGHT: PropKey<f32> = PropKey::new(42, L);
pub const SCROLL_X: PropKey<f32> = PropKey::new(43, L);
pub const SCROLL_Y: PropKey<f32> = PropKey::new(44, L);

// ── 文本（50..69）──
pub const TEXT: PropKey<SharedString> = PropKey::new(50, L);
pub const FONT_SIZE: PropKey<f32> = PropKey::new(51, L.union(INH));
pub const FONT_FAMILY: PropKey<SharedString> = PropKey::new(52, L.union(INH));
pub const FONT_WEIGHT: PropKey<u32> = PropKey::new(53, L.union(INH));
pub const LINE_HEIGHT: PropKey<f32> = PropKey::new(54, L.union(INH));
pub const TEXT_ALIGN: PropKey<u32> = PropKey::new(55, L);
/// 是否按可用宽度换行；false 时始终单行
pub const TEXT_WRAP: PropKey<bool> = PropKey::new(56, L);
pub const ITALIC: PropKey<bool> = PropKey::new(57, L.union(INH));

// ── 外观（70..89）──
pub const BG: PropKey<Color> = PropKey::new(70, P);
pub const FG: PropKey<Color> = PropKey::new(71, P.union(INH));
pub const BORDER_COLOR: PropKey<Color> = PropKey::new(72, P);
pub const RADIUS: PropKey<f32> = PropKey::new(73, P);
pub const OPACITY: PropKey<f32> = PropKey::new(74, P);

// ── 状态（90..99）──
pub const VISIBLE: PropKey<bool> = PropKey::new(90, L);
pub const DISABLED: PropKey<bool> = PropKey::new(91, PropFlags::empty());

/// 当前已占用的槽位上界（不含）。新增属性取下一个号。
pub const SLOT_COUNT: u16 = 100;

// ── 枚举取值常量 ──

pub mod flex_direction {
    pub const ROW: u32 = 0;
    pub const ROW_REVERSE: u32 = 1;
    pub const COLUMN: u32 = 2;
    pub const COLUMN_REVERSE: u32 = 3;
}

pub mod flex_wrap {
    pub const NO_WRAP: u32 = 0;
    pub const WRAP: u32 = 1;
    pub const WRAP_REVERSE: u32 = 2;
}

pub mod align {
    pub const AUTO: u32 = 0;
    pub const START: u32 = 1;
    pub const CENTER: u32 = 2;
    pub const END: u32 = 3;
    pub const STRETCH: u32 = 4;
    pub const BASELINE: u32 = 5;
    pub const SPACE_BETWEEN: u32 = 6;
    pub const SPACE_AROUND: u32 = 7;
    pub const SPACE_EVENLY: u32 = 8;
}

pub mod position_type {
    pub const RELATIVE: u32 = 0;
    pub const ABSOLUTE: u32 = 1;
}

pub mod display {
    pub const FLEX: u32 = 0;
    pub const NONE: u32 = 1;
}

pub mod text_align {
    pub const START: u32 = 0;
    pub const CENTER: u32 = 1;
    pub const END: u32 = 2;
    pub const JUSTIFY: u32 = 3;
}
