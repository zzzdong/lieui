//! FlexNode — 核心 Flexbox 布局节点
//!
//! 参考 Taitank::TaitankNode / W3C CSS Flexbox Level 1

use crate::flex_line::FlexLine;
use crate::measure::LayoutTree;
use crate::style::FlexStyle;
use crate::types::*;

/// Flex 布局节点
#[derive(Debug, Clone)]
pub struct FlexNode {
    /// 关联的 ElementId（用于样式查询和结果写回）
    pub id: u64,
    pub style: FlexStyle,
    pub layout_result: LayoutResult,
    pub children: Vec<FlexNode>,
    pub is_frozen: bool,
    pub is_dirty: bool,
    pub has_new_layout: bool,
    pub in_initial_state: bool,

    /// 叶子节点的预测量尺寸（无约束时的 intrinsic size）。
    /// 布局时若 `LayoutTree::measure` 返回 None（非文本叶子），则回落到该值。
    pub intrinsic_size: Option<(f32, f32)>,
}

impl FlexNode {
    pub fn new(id: u64, style: FlexStyle) -> Self {
        Self {
            id,
            style,
            layout_result: LayoutResult::default(),
            children: Vec::new(),
            is_frozen: false,
            is_dirty: true,
            has_new_layout: false,
            in_initial_state: true,
            intrinsic_size: None,
        }
    }

    pub fn new_column(id: u64) -> Self {
        Self::new(id, FlexStyle::column())
    }
    pub fn new_row(id: u64) -> Self {
        Self::new(id, FlexStyle::row())
    }

    /// 创建叶子节点（测试用，id=0）
    pub fn new_leaf(width: f32, height: f32) -> Self {
        let mut s = FlexStyle::default();
        s.dim[Dimension::Width as usize] = width;
        s.dim[Dimension::Height as usize] = height;
        Self::new(0, s)
    }
    /// 创建带 id 的叶子节点
    pub fn new_leaf_with_id(id: u64, width: f32, height: f32) -> Self {
        let mut s = FlexStyle::default();
        s.dim[Dimension::Width as usize] = width;
        s.dim[Dimension::Height as usize] = height;
        Self::new(id, s)
    }

    pub fn add_child(&mut self, child: FlexNode) {
        self.children.push(child);
        self.is_dirty = true;
    }

    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    // ---- Dimension ----

    pub fn is_layout_dimension_defined(&self, axis: FlexDirection) -> bool {
        is_defined(self.layout_result.dim[K_AXIS_DIM[axis as usize] as usize])
    }

    pub fn set_layout_dimension(&mut self, axis: FlexDirection, value: f32) {
        self.layout_result.dim[K_AXIS_DIM[axis as usize] as usize] = value;
    }

    pub fn get_layout_dimension(&self, axis: FlexDirection) -> f32 {
        let v = self.layout_result.dim[K_AXIS_DIM[axis as usize] as usize];
        if is_defined(v) { v } else { VALUE_UNDEFINED }
    }

    // ---- Border/Padding ----

    pub fn get_start_border(&self, axis: FlexDirection) -> f32 {
        self.style.get_start_border(axis)
    }
    pub fn get_end_border(&self, axis: FlexDirection) -> f32 {
        self.style.get_end_border(axis)
    }
    pub fn get_start_padding_and_border(&self, axis: FlexDirection) -> f32 {
        self.style.get_start_padding(axis) + self.style.get_start_border(axis)
    }
    pub fn get_end_padding_and_border(&self, axis: FlexDirection) -> f32 {
        self.style.get_end_padding(axis) + self.style.get_end_border(axis)
    }
    pub fn get_padding_and_border(&self, axis: FlexDirection) -> f32 {
        self.get_start_padding_and_border(axis) + self.get_end_padding_and_border(axis)
    }

    // ---- Margin ----

    pub fn get_start_margin(&self, axis: FlexDirection) -> f32 {
        self.style.get_start_margin(axis)
    }
    pub fn get_end_margin(&self, axis: FlexDirection) -> f32 {
        self.style.get_end_margin(axis)
    }
    pub fn get_margin(&self, axis: FlexDirection) -> f32 {
        self.style.get_start_margin(axis) + self.style.get_end_margin(axis)
    }
    pub fn is_auto_start_margin(&self, axis: FlexDirection) -> bool {
        self.style.is_auto_start_margin(axis)
    }
    pub fn is_auto_end_margin(&self, axis: FlexDirection) -> bool {
        self.style.is_auto_end_margin(axis)
    }

    pub fn set_layout_start_margin(&mut self, axis: FlexDirection, value: f32) {
        self.layout_result.margin[K_AXIS_START[axis as usize] as usize] = value;
    }
    pub fn set_layout_end_margin(&mut self, axis: FlexDirection, value: f32) {
        self.layout_result.margin[K_AXIS_END[axis as usize] as usize] = value;
    }
    pub fn get_layout_start_margin(&self, axis: FlexDirection) -> f32 {
        let v = self.layout_result.margin[K_AXIS_START[axis as usize] as usize];
        if is_defined(v) { v } else { 0.0 }
    }
    pub fn get_layout_end_margin(&self, axis: FlexDirection) -> f32 {
        let v = self.layout_result.margin[K_AXIS_END[axis as usize] as usize];
        if is_defined(v) { v } else { 0.0 }
    }

    // ---- Position ----

    pub fn set_layout_start_position(&mut self, axis: FlexDirection, value: f32) {
        self.layout_result.position[K_AXIS_START[axis as usize] as usize] = value;
    }
    pub fn set_layout_end_position(&mut self, axis: FlexDirection, value: f32) {
        self.layout_result.position[K_AXIS_END[axis as usize] as usize] = value;
    }

    // ---- Axis resolution ----

    pub fn resolve_main_axis(&self) -> FlexDirection {
        let ma = self.style.flex_direction;
        if self.layout_result.direction == Direction::Rtl {
            match ma {
                FlexDirection::Row => return FlexDirection::RowReverse,
                FlexDirection::RowReverse => return FlexDirection::Row,
                _ => {}
            }
        }
        ma
    }

    pub fn resolve_cross_axis(&self) -> FlexDirection {
        let ma = self.style.flex_direction;

        if is_row_direction(ma) {
            if self.style.flex_wrap == FlexWrap::WrapReverse {
                FlexDirection::ColumnReverse
            } else {
                FlexDirection::Column
            }
        } else {
            let c = if self.style.flex_wrap == FlexWrap::WrapReverse {
                FlexDirection::RowReverse
            } else {
                FlexDirection::Row
            };
            if self.layout_result.direction == Direction::Rtl {
                match c {
                    FlexDirection::Row => return FlexDirection::RowReverse,
                    FlexDirection::RowReverse => return FlexDirection::Row,
                    _ => {}
                }
            }
            c
        }
    }

    fn resolve_direction(&self, pd: Direction) -> Direction {
        match self.style.direction {
            Direction::Inherit => match pd {
                Direction::Inherit => Direction::Ltr,
                d => d,
            },
            d => d,
        }
    }

    fn resolve_style_values(&mut self) {
        let ma = self.resolve_main_axis();
        let ca = self.resolve_cross_axis();
        self.set_layout_start_margin(ma, self.get_start_margin(ma));
        self.set_layout_end_margin(ma, self.get_end_margin(ma));
        self.set_layout_start_margin(ca, self.get_start_margin(ca));
        self.set_layout_end_margin(ca, self.get_end_margin(ca));
        let lp = &mut self.layout_result;
        lp.padding[K_AXIS_START[ma as usize] as usize] = self.style.get_start_padding(ma);
        lp.padding[K_AXIS_END[ma as usize] as usize] = self.style.get_end_padding(ma);
        lp.padding[K_AXIS_START[ca as usize] as usize] = self.style.get_start_padding(ca);
        lp.padding[K_AXIS_END[ca as usize] as usize] = self.style.get_end_padding(ca);
        lp.border[K_AXIS_START[ma as usize] as usize] = self.style.get_start_border(ma);
        lp.border[K_AXIS_END[ma as usize] as usize] = self.style.get_end_border(ma);
        lp.border[K_AXIS_START[ca as usize] as usize] = self.style.get_start_border(ca);
        lp.border[K_AXIS_END[ca as usize] as usize] = self.style.get_end_border(ca);
    }

    pub fn bound_axis(&self, axis: FlexDirection, value: f32) -> f32 {
        let d = K_AXIS_DIM[axis as usize] as usize;
        let min = self.style.min_dim[d];
        let max = self.style.max_dim[d];
        let mut v = value;
        if is_defined(max) && max >= 0.0 && v > max {
            v = max;
        }
        if is_defined(min) && min >= 0.0 && v < min {
            v = min;
        }
        v
    }

    fn get_node_align(&self, idx: usize) -> FlexAlign {
        let a = self.children[idx].style.align_self;
        if a == FlexAlign::Auto {
            self.style.align_items
        } else {
            a
        }
    }

    // ============ Public API ============

    /// 执行布局。`host` 提供叶子节点的文本测量能力（按可用宽度重新测量以支持换行）。
    pub fn layout<T: LayoutTree>(
        &mut self,
        host: &mut T,
        parent_width: f32,
        parent_height: f32,
        parent_direction: Direction,
    ) {
        let ma = self.style.flex_direction;
        if is_undefined(self.style.flex_basis)
            && is_defined(self.style.dim[K_AXIS_DIM[ma as usize] as usize])
        {
            self.style.flex_basis = self.style.dim[K_AXIS_DIM[ma as usize] as usize];
        }

        let mut swr = false;
        if is_undefined(self.style.dim[0]) && is_defined(parent_width) {
            let w = parent_width - self.get_margin(FlexDirection::Row);
            self.style.dim[0] = if w > 0.0 { w } else { 0.0 };
            swr = true;
        }
        let mut shr = false;
        if is_undefined(self.style.dim[1]) && is_defined(parent_height) {
            let h = parent_height - self.get_margin(FlexDirection::Column);
            self.style.dim[1] = if h > 0.0 { h } else { 0.0 };
            shr = true;
        }

        self.layout_impl(
            host,
            parent_width,
            parent_height,
            parent_direction,
            LayoutAction::Layout,
        );

        if swr {
            self.style.dim[0] = VALUE_UNDEFINED;
        }
        if shr {
            self.style.dim[1] = VALUE_UNDEFINED;
        }

        let ma = self.resolve_main_axis();
        let ca = self.resolve_cross_axis();
        self.set_layout_start_position(ma, self.get_start_margin(ma));
        self.set_layout_end_position(ma, self.get_end_margin(ma));
        self.set_layout_start_position(ca, self.get_start_margin(ca));
        self.set_layout_end_position(ca, self.get_end_margin(ca));
    }

    // ============ LayoutImpl ============

    fn layout_impl<T: LayoutTree>(
        &mut self,
        host: &mut T,
        parent_width: f32,
        parent_height: f32,
        parent_direction: Direction,
        layout_action: LayoutAction,
    ) {
        let dir = self.resolve_direction(parent_direction);
        if self.layout_result.direction != dir {
            self.layout_result.direction = dir;
            self.resolve_style_values();
        }
        let ma = self.style.flex_direction;
        let perform_layout = layout_action == LayoutAction::Layout;

        // ★ 记录原始父约束，供重排边界子树在下一帧复现完全相同的输入
        self.layout_result.avail = [parent_width, parent_height];

        let pw = if is_defined(parent_width) {
            (parent_width - self.get_margin(FlexDirection::Row)).max(0.0)
        } else {
            parent_width
        };
        let ph = if is_defined(parent_height) {
            (parent_height - self.get_margin(FlexDirection::Column)).max(0.0)
        } else {
            parent_height
        };

        let nw = if is_defined(self.style.dim[0]) {
            self.bound_axis(FlexDirection::Row, self.style.dim[0])
        } else {
            VALUE_UNDEFINED
        };
        let nh = if is_defined(self.style.dim[1]) {
            self.bound_axis(FlexDirection::Column, self.style.dim[1])
        } else {
            VALUE_UNDEFINED
        };

        if layout_action == LayoutAction::MeasureWidth && is_defined(nw) {
            self.layout_result.dim[0] = nw;
            return;
        }
        if layout_action == LayoutAction::MeasureHeight && is_defined(nh) {
            self.layout_result.dim[1] = nh;
            return;
        }

        // 9.2 available space
        let mut aw = VALUE_UNDEFINED;
        if is_defined(nw) {
            aw = nw - self.get_padding_and_border(FlexDirection::Row);
        } else if is_defined(pw) {
            aw = pw - self.get_padding_and_border(FlexDirection::Row);
        }

        let mut ah = VALUE_UNDEFINED;
        if is_defined(nh) {
            ah = nh - self.get_padding_and_border(FlexDirection::Column);
        } else if is_defined(ph) {
            ah = ph - self.get_padding_and_border(FlexDirection::Column);
        }

        // max_dim clamp
        if is_defined(self.style.max_dim[0]) {
            if float_is_equal(self.style.max_dim[0], self.style.min_dim[0]) {
                self.style.dim[0] = self.style.min_dim[0];
            }
            let mdw = self.style.max_dim[0] - self.get_padding_and_border(FlexDirection::Row);
            if mdw >= 0.0 && mdw < nan_as_inf(aw) {
                aw = mdw;
            }
        }
        if is_defined(self.style.max_dim[1]) {
            if float_is_equal(self.style.max_dim[1], self.style.min_dim[1]) {
                self.style.dim[1] = self.style.min_dim[1];
            }
            let mdh = self.style.max_dim[1] - self.get_padding_and_border(FlexDirection::Column);
            if mdh >= 0.0 && mdh < nan_as_inf(ah) {
                ah = mdh;
            }
        }

        aw = if aw < 0.0 { 0.0 } else { aw };
        ah = if ah < 0.0 { 0.0 } else { ah };

        let wm = if is_defined(nw) {
            MeasureMode::Exactly
        } else if is_defined(aw) {
            MeasureMode::AtMost
        } else {
            MeasureMode::Undefined
        };
        let hm = if is_defined(nh) {
            MeasureMode::Exactly
        } else if is_defined(ah) {
            MeasureMode::AtMost
        } else {
            MeasureMode::Undefined
        };

        if perform_layout {
            self.layout_result.had_overflow = false;
        }

        // leaf
        if self.children.is_empty() {
            self.layout_single_node(host, aw, ah, wm, hm);
            return;
        }

        let asz = TaitankSize {
            width: aw,
            height: ah,
        };

        // Step 3
        self.calculate_items_flex_basis(host, asz);

        // Step 5
        let mut fl = self.collect_flex_lines(asz);

        // Step 4: container main size
        let max_sum = fl
            .iter()
            .map(|l| l.sum_hypothetical_main_size)
            .fold(0.0f32, f32::max);
        let cims = if is_defined(self.style.dim[K_AXIS_DIM[ma as usize] as usize]) {
            self.style.dim[K_AXIS_DIM[ma as usize] as usize] - self.get_padding_and_border(ma)
        } else {
            max_sum
        };
        self.layout_result.dim[K_AXIS_DIM[ma as usize] as usize] =
            self.bound_axis(ma, cims + self.get_padding_and_border(ma));

        if (layout_action == LayoutAction::MeasureWidth && is_row_direction(ma))
            || (layout_action == LayoutAction::MeasureHeight && is_column_direction(ma))
        {
            return;
        }

        // Step 6
        self.determine_items_main_axis_size(&mut fl, layout_action);

        // Step 7-11
        let slcs = self.determine_cross_axis_size(host, &mut fl, asz, layout_action);

        if !perform_layout {
            let ca = self.resolve_cross_axis();
            let cds = if is_defined(self.style.dim[K_AXIS_DIM[ca as usize] as usize]) {
                self.style.dim[K_AXIS_DIM[ca as usize] as usize]
            } else {
                slcs + self.get_padding_and_border(ca)
            };
            self.layout_result.dim[K_AXIS_DIM[ca as usize] as usize] = self.bound_axis(ca, cds);
            return;
        }

        // Step 12
        self.main_axis_alignment(&mut fl);
        // Step 13-16
        self.cross_axis_alignment(&mut fl);
        // Absolute
        self.layout_fixed_items(host);
    }

    // ============ Step 3 ============

    fn calculate_items_flex_basis<T: LayoutTree>(&mut self, host: &mut T, asz: TaitankSize) {
        let ma = self.style.flex_direction;
        let parent_dir = self.layout_result.direction;
        for i in 0..self.children.len() {
            if self.children[i].style.display_type == DisplayType::None {
                continue;
            }
            if self.children[i].style.position_type == PositionType::Absolute {
                continue;
            }

            let item = &mut self.children[i];
            if is_defined(item.style.get_flex_basis())
                && is_defined(self.style.dim[K_AXIS_DIM[ma as usize] as usize])
            {
                item.layout_result.flex_base_size = item.style.get_flex_basis();
            } else if is_defined(item.style.dim[K_AXIS_DIM[ma as usize] as usize]) {
                item.layout_result.flex_base_size =
                    item.style.dim[K_AXIS_DIM[ma as usize] as usize];
            } else {
                let old = item.style.get_dimension_axis(ma);
                item.style.set_dimension_axis(ma, item.style.flex_basis);
                item.layout_impl(
                    host,
                    asz.width,
                    asz.height,
                    parent_dir,
                    if is_row_direction(ma) {
                        LayoutAction::MeasureWidth
                    } else {
                        LayoutAction::MeasureHeight
                    },
                );
                item.style.set_dimension_axis(ma, old);
                item.layout_result.flex_base_size =
                    if is_defined(item.layout_result.dim[K_AXIS_DIM[ma as usize] as usize]) {
                        item.layout_result.dim[K_AXIS_DIM[ma as usize] as usize]
                    } else {
                        0.0
                    };
            }
            item.layout_result.hypothetical_main_axis_size =
                item.bound_axis(ma, item.layout_result.flex_base_size);
            item.layout_result.hypothetical_main_axis_margin_boxsize =
                item.layout_result.hypothetical_main_axis_size + item.get_margin(ma);
        }
    }

    // ============ Step 5 ============

    fn collect_flex_lines(&mut self, asz: TaitankSize) -> Vec<FlexLine> {
        let mut lines: Vec<FlexLine> = Vec::new();
        let n = self.children.len();
        let aw = if K_AXIS_DIM[self.style.flex_direction as usize] == Dimension::Width {
            asz.width
        } else {
            asz.height
        };
        let aw = if is_undefined(aw) { f32::INFINITY } else { aw };
        let mut line: Option<FlexLine> = None;
        let gap = self.style.item_space;
        let mut i = 0;
        while i < n {
            if self.children[i].style.position_type == PositionType::Absolute
                || self.children[i].style.display_type == DisplayType::None
            {
                if i == n - 1 {
                    if let Some(l) = line.take() {
                        lines.push(l);
                    }
                    break;
                }
                i += 1;
                continue;
            }
            if line.is_none() {
                line = Some(FlexLine::new());
            }
            let lr = line.as_mut().unwrap();

            // 对第 2+ 个 item 添加 gap
            if !lr.is_empty() && gap > 0.0 {
                lr.sum_hypothetical_main_size += gap;
            }

            let hms = self.children[i]
                .layout_result
                .hypothetical_main_axis_margin_boxsize;
            let ls = aw - (lr.sum_hypothetical_main_size + hms);

            if self.style.flex_wrap == FlexWrap::NoWrap {
                lr.add_item(
                    i,
                    hms,
                    self.children[i].style.flex_grow,
                    self.children[i].style.flex_shrink,
                    self.children[i].layout_result.flex_base_size,
                );
                if i == n - 1 {
                    lines.push(line.take().unwrap());
                    break;
                }
                i += 1;
            } else {
                if ls >= 0.0 || lr.is_empty() {
                    lr.add_item(
                        i,
                        hms,
                        self.children[i].style.flex_grow,
                        self.children[i].style.flex_shrink,
                        self.children[i].layout_result.flex_base_size,
                    );
                    if i == n - 1 {
                        lines.push(line.take().unwrap());
                    }
                    i += 1;
                } else {
                    lines.push(line.take().unwrap());
                }
            }
        }
        lines
    }

    // ============ Step 6 ============

    fn determine_items_main_axis_size(&mut self, fl: &mut [FlexLine], la: LayoutAction) {
        let ma = self.style.flex_direction;
        let mc = self.layout_result.dim[K_AXIS_DIM[ma as usize] as usize]
            - self.get_padding_and_border(ma);
        if la == LayoutAction::Layout {
            for c in &mut self.children {
                c.is_frozen = false;
            }
        }
        for line in fl.iter_mut() {
            line.container_main_inner_size = mc;
            let _ = line.freeze_inflexible_items(ma, &mut self.children);
            while !line.resolve_flexible_lengths(ma, &mut self.children) {}
            if la == LayoutAction::Layout && line.remaining_free_space < 0.0 {
                self.layout_result.had_overflow = true;
            }
        }
    }

    // ============ Step 7-11 ============

    fn determine_cross_axis_size<T: LayoutTree>(
        &mut self,
        host: &mut T,
        fl: &mut [FlexLine],
        asz: TaitankSize,
        la: LayoutAction,
    ) -> f32 {
        let ma = self.style.flex_direction;
        let ca = self.resolve_cross_axis();
        let parent_dir = self.layout_result.direction;
        let fl_len = fl.len();
        let parent_dim_ca = self.style.dim[K_AXIS_DIM[ca as usize] as usize];
        let pb_ca = self.get_padding_and_border(ca);
        let align_content = self.style.align_content;
        let mut slcs = 0.0f32;

        for line in fl.iter_mut() {
            let mut max_cs = 0.0f32;

            for &idx in &line.items {
                let mut act = la;
                let stretch_mode = self.get_node_align(idx) == FlexAlign::Stretch
                    && self.children[idx].style.is_dimension_auto(ca)
                    && !self.children[idx].style.is_auto_margin(ca)
                    && la == LayoutAction::Layout;

                if stretch_mode {
                    act = if K_AXIS_DIM[ca as usize] == Dimension::Width {
                        LayoutAction::MeasureWidth
                    } else {
                        LayoutAction::MeasureHeight
                    };
                }

                let old_m = self.children[idx].style.get_dimension_axis(ma);
                let cur_layout_dim = self.children[idx].get_layout_dimension(ma);
                self.children[idx]
                    .style
                    .set_dimension_axis(ma, cur_layout_dim);
                self.children[idx].layout_impl(host, asz.width, asz.height, parent_dir, act);
                self.children[idx].style.set_dimension_axis(ma, old_m);
                let child_had_overflow = self.children[idx].layout_result.had_overflow;
                self.layout_result.had_overflow |= child_had_overflow;

                let cross_dim = self.children[idx].get_layout_dimension(ca);
                let cross_margin = self.children[idx].get_margin(ca);
                let ocs = cross_dim + cross_margin;
                if ocs > max_cs {
                    max_cs = ocs;
                }
            }

            max_cs = self.bound_axis(ca, max_cs);
            line.line_cross_size = max_cs;
            slcs += max_cs;
            if fl_len == 1 && is_defined(parent_dim_ca) {
                let ic = self.bound_axis(ca, parent_dim_ca) - pb_ca;
                line.line_cross_size = ic;
                slcs = ic;
            }
        }

        // align-content: stretch
        if is_defined(parent_dim_ca) && align_content == FlexAlign::Stretch {
            let ic = self.bound_axis(ca, parent_dim_ca) - pb_ca;
            if slcs < ic {
                let ex = (ic - slcs) / fl.len() as f32;
                for line in fl.iter_mut() {
                    line.line_cross_size += ex;
                }
            }
        }

        // align-self: stretch (Step 11)
        for line in fl.iter_mut() {
            let items: Vec<usize> = line.items.clone();
            for idx in items {
                let is_stretch = self.get_node_align(idx) == FlexAlign::Stretch
                    && self.children[idx].style.is_dimension_auto(ca)
                    && !self.children[idx].style.is_auto_margin(ca);

                if is_stretch {
                    let margin_ca = self.children[idx].get_margin(ca);
                    let nc = self.children[idx].bound_axis(ca, line.line_cross_size - margin_ca);
                    self.children[idx].layout_result.dim[K_AXIS_DIM[ca as usize] as usize] = nc;
                    let old_m = self.children[idx].style.get_dimension_axis(ma);
                    let old_c = self.children[idx].style.get_dimension_axis(ca);
                    let cur_ma = self.children[idx].get_layout_dimension(ma);
                    let cur_ca = self.children[idx].get_layout_dimension(ca);
                    self.children[idx].style.set_dimension_axis(ma, cur_ma);
                    self.children[idx].style.set_dimension_axis(ca, cur_ca);
                    self.children[idx].layout_impl(host, asz.width, asz.height, parent_dir, la);
                    self.children[idx].style.set_dimension_axis(ma, old_m);
                    self.children[idx].style.set_dimension_axis(ca, old_c);
                }
            }
        }
        slcs
    }

    // ============ Step 12: Main-Axis Alignment ============

    fn main_axis_alignment(&mut self, fl: &mut [FlexLine]) {
        let ma = self.resolve_main_axis();
        let mc = self.get_layout_dimension(ma) - self.get_padding_and_border(ma);
        let justify_content = self.style.justify_content;
        let pbs = self.get_start_padding_and_border(ma);
        let ld = self.get_layout_dimension(ma);
        let gap = self.style.item_space;

        for line in fl.iter_mut() {
            line.container_main_inner_size = mc;
            line.gap = gap;
            line.align_items(ma, &mut self.children, justify_content, pbs, ld);
        }
    }

    // ============ Step 13-16: Cross-Axis Alignment ============

    fn cross_axis_alignment(&mut self, fl: &mut [FlexLine]) {
        let ca = self.resolve_cross_axis();
        let lc = fl.len();
        let mut slcs = 0.0f32;
        let parent_layout_dim_ca = self.get_layout_dimension(ca);

        // Step 13-14
        for line in fl.iter_mut() {
            slcs += line.line_cross_size;
            for &idx in &line.items {
                let cd = if is_defined(
                    self.children[idx].layout_result.dim[K_AXIS_DIM[ca as usize] as usize],
                ) {
                    self.children[idx].layout_result.dim[K_AXIS_DIM[ca as usize] as usize]
                } else {
                    0.0
                };
                let margin_ca = self.children[idx].get_margin(ca);
                let is_auto_start = self.children[idx].is_auto_start_margin(ca);
                let is_auto_end = self.children[idx].is_auto_end_margin(ca);
                let rem = line.line_cross_size - cd - margin_ca;

                if rem > 0.0 {
                    if is_auto_start && is_auto_end {
                        self.children[idx].set_layout_start_margin(ca, rem / 2.0);
                        self.children[idx].set_layout_end_margin(ca, rem / 2.0);
                    } else if is_auto_start {
                        self.children[idx].set_layout_start_margin(ca, rem);
                    } else if is_auto_end {
                        self.children[idx].set_layout_end_margin(ca, rem);
                    } else {
                        let sm = self.children[idx].get_start_margin(ca);
                        let em = self.children[idx].get_end_margin(ca);
                        self.children[idx].set_layout_start_margin(ca, sm);
                        self.children[idx].set_layout_end_margin(ca, em);
                    }
                } else {
                    let sm = self.children[idx].get_start_margin(ca);
                    let em = self.children[idx].get_end_margin(ca);
                    self.children[idx].set_layout_start_margin(ca, sm);
                    self.children[idx].set_layout_end_margin(ca, em);
                }

                let r2 = line.line_cross_size
                    - cd
                    - self.children[idx].get_layout_start_margin(ca)
                    - self.children[idx].get_layout_end_margin(ca);
                let mut off = self.children[idx].get_layout_start_margin(ca);
                match self.get_node_align(idx) {
                    FlexAlign::Center => off += r2 / 2.0,
                    FlexAlign::End => off += r2,
                    _ => {}
                }
                self.children[idx].set_layout_start_position(ca, off);
            }
        }

        // Step 15: container cross size
        let cds = if is_defined(self.style.dim[K_AXIS_DIM[ca as usize] as usize]) {
            self.style.dim[K_AXIS_DIM[ca as usize] as usize]
        } else {
            slcs + self.get_padding_and_border(ca)
        };
        self.layout_result.dim[K_AXIS_DIM[ca as usize] as usize] = self.bound_axis(ca, cds);

        // Step 16: align flex lines
        let ic = self.layout_result.dim[K_AXIS_DIM[ca as usize] as usize]
            - self.get_padding_and_border(ca);
        let rem = ic - slcs;
        let mut off = self.get_start_padding_and_border(ca);
        let space = match self.style.align_content {
            FlexAlign::Center => {
                off += rem / 2.0;
                0.0
            }
            FlexAlign::End => {
                off += rem;
                0.0
            }
            FlexAlign::SpaceBetween if lc > 1 => rem / (lc - 1) as f32,
            FlexAlign::SpaceAround => {
                let s = rem / lc as f32;
                off += s / 2.0;
                s
            }
            _ => 0.0,
        };

        let mut cpos = off;
        for line in fl.iter_mut() {
            for &idx in &line.items {
                let st = cpos
                    + self.children[idx].layout_result.position[K_AXIS_START[ca as usize] as usize];
                self.children[idx].set_layout_start_position(ca, st);

                // compute end position
                let ld = parent_layout_dim_ca;
                let sp =
                    self.children[idx].layout_result.position[K_AXIS_START[ca as usize] as usize];
                let cd = if is_defined(
                    self.children[idx].layout_result.dim[K_AXIS_DIM[ca as usize] as usize],
                ) {
                    self.children[idx].layout_result.dim[K_AXIS_DIM[ca as usize] as usize]
                } else {
                    0.0
                };
                self.children[idx].set_layout_end_position(ca, ld - sp - cd);
            }
            cpos += line.line_cross_size + space;
        }
    }

    // ============ Absolute positioning ============

    fn layout_fixed_items<T: LayoutTree>(&mut self, host: &mut T) {
        let ma = self.resolve_main_axis();
        let ca = self.resolve_cross_axis();
        let pw = self.get_layout_dimension(FlexDirection::Row)
            - self.get_padding_and_border(FlexDirection::Row);
        let ph = self.get_layout_dimension(FlexDirection::Column)
            - self.get_padding_and_border(FlexDirection::Column);
        let parent_dir = self.layout_result.direction;

        for i in 0..self.children.len() {
            if self.children[i].style.display_type == DisplayType::None {
                continue;
            }
            if self.children[i].style.position_type != PositionType::Absolute {
                continue;
            }

            let om = self.children[i].style.get_dimension_axis(ma);
            let oc = self.children[i].style.get_dimension_axis(ca);

            if is_undefined(om)
                && is_defined(self.children[i].style.get_start_position(ma))
                && is_defined(self.children[i].style.get_end_position(ma))
            {
                let sz = self.get_layout_dimension(ma)
                    - self.style.get_start_border(ma)
                    - self.style.get_end_border(ma)
                    - self.children[i].style.get_start_position(ma)
                    - self.children[i].style.get_end_position(ma)
                    - self.children[i].get_margin(ma);
                self.children[i].style.set_dimension_axis(ma, sz);
            }
            if is_undefined(oc)
                && is_defined(self.children[i].style.get_start_position(ca))
                && is_defined(self.children[i].style.get_end_position(ca))
            {
                let sz = self.get_layout_dimension(ca)
                    - self.style.get_start_border(ca)
                    - self.style.get_end_border(ca)
                    - self.children[i].style.get_start_position(ca)
                    - self.children[i].style.get_end_position(ca)
                    - self.children[i].get_margin(ca);
                self.children[i].style.set_dimension_axis(ca, sz);
            }

            self.children[i].layout_impl(host, pw, ph, parent_dir, LayoutAction::Layout);
            self.children[i].style.set_dimension_axis(ma, om);
            self.children[i].style.set_dimension_axis(ca, oc);
        }

        // Separate pass to set positions (avoids double borrow)
        for i in 0..self.children.len() {
            if self.children[i].style.position_type != PositionType::Absolute {
                continue;
            }
            if self.children[i].style.display_type == DisplayType::None {
                continue;
            }
            self.calc_fixed_pos(i, ma);
            self.calc_fixed_pos(i, ca);
        }
    }

    fn calc_fixed_pos(&mut self, idx: usize, axis: FlexDirection) {
        let start_pos = self.children[idx].style.get_start_position(axis);
        let end_pos = self.children[idx].style.get_end_position(axis);
        let layout_dim = self.get_layout_dimension(axis);
        let pbs = self.get_start_padding_and_border(axis);
        let child_dim = self.children[idx].get_layout_dimension(axis);
        let margin_start = self.children[idx].get_layout_start_margin(axis);

        if is_defined(start_pos) {
            let sp = self.get_start_border(axis)
                + self.children[idx].get_layout_start_margin(axis)
                + start_pos;
            self.children[idx].set_layout_start_position(axis, sp);
            self.children[idx].set_layout_end_position(axis, layout_dim - sp - child_dim);
        } else if is_defined(end_pos) {
            let ep = self.get_end_border(axis)
                + self.children[idx].get_layout_end_margin(axis)
                + end_pos;
            self.children[idx].set_layout_end_position(axis, ep);
            self.children[idx].set_layout_start_position(axis, layout_dim - ep - child_dim);
        } else {
            let rem = layout_dim - self.get_padding_and_border(axis) - child_dim;
            let mut off = pbs;
            let al = if axis == self.resolve_main_axis() {
                self.style.justify_content
            } else {
                self.get_node_align(idx)
            };
            match al {
                FlexAlign::Center => off += rem / 2.0,
                FlexAlign::End => off += rem,
                _ => {}
            }
            let sp = self.get_start_padding_and_border(axis) + margin_start + off;
            self.children[idx].set_layout_start_position(axis, sp);
            self.children[idx].set_layout_end_position(axis, layout_dim - sp - child_dim);
        }
    }

    // ============ Leaf layout ============

    fn layout_single_node<T: LayoutTree>(
        &mut self,
        host: &mut T,
        aw: f32,
        ah: f32,
        width_measure_mode: MeasureMode,
        height_measure_mode: MeasureMode,
    ) {
        let intrinsic_w = self.intrinsic_size.map_or(0.0, |(w, _)| w);
        let intrinsic_h = self.intrinsic_size.map_or(0.0, |(_, h)| h);

        // 文本叶子：按可用宽度重新测量以支持换行（而非使用单行 intrinsic size）。
        // 可用宽度只在 Exactly / AtMost 模式下有效；Undefined 表示父级不约束宽度。
        // 是否换行由 host 决定（见 `LayoutTree::measure`），引擎不感知文本样式。
        let (content_w, content_h) = {
            let avail_w = match width_measure_mode {
                MeasureMode::Exactly | MeasureMode::AtMost if is_defined(aw) => Some(aw),
                _ => None,
            };
            host.measure(self.id, avail_w)
                .unwrap_or((intrinsic_w, intrinsic_h))
        };

        match width_measure_mode {
            MeasureMode::Exactly => {
                self.layout_result.dim[0] = aw + self.get_padding_and_border(FlexDirection::Row);
            }
            MeasureMode::AtMost => {
                let dw = if is_defined(self.style.dim[0]) {
                    self.style.dim[0]
                } else {
                    content_w
                };
                let pb = self.get_padding_and_border(FlexDirection::Row);
                self.layout_result.dim[0] = self.bound_axis(
                    FlexDirection::Row,
                    if is_defined(aw) && dw + pb > aw + pb {
                        aw + pb
                    } else {
                        dw + pb
                    },
                );
            }
            MeasureMode::Undefined => {
                let dw = if is_defined(self.style.dim[0]) {
                    self.style.dim[0]
                } else {
                    content_w
                };
                self.layout_result.dim[0] = dw + self.get_padding_and_border(FlexDirection::Row);
            }
        }

        match height_measure_mode {
            MeasureMode::Exactly => {
                self.layout_result.dim[1] = ah + self.get_padding_and_border(FlexDirection::Column);
            }
            MeasureMode::AtMost => {
                let dh = if is_defined(self.style.dim[1]) {
                    self.style.dim[1]
                } else {
                    content_h
                };
                let pb = self.get_padding_and_border(FlexDirection::Column);
                self.layout_result.dim[1] = self.bound_axis(
                    FlexDirection::Column,
                    if is_defined(ah) && dh + pb > ah + pb {
                        ah + pb
                    } else {
                        dh + pb
                    },
                );
            }
            MeasureMode::Undefined => {
                let dh = if is_defined(self.style.dim[1]) {
                    self.style.dim[1]
                } else {
                    content_h
                };
                self.layout_result.dim[1] = dh + self.get_padding_and_border(FlexDirection::Column);
            }
        }

        self.is_dirty = false;
        self.has_new_layout = true;
        self.in_initial_state = false;
    }

    // ============ Convenience getters ============

    pub fn get_left(&self) -> f32 {
        self.layout_result.position[CSSDirection::Left as usize]
    }
    pub fn get_top(&self) -> f32 {
        self.layout_result.position[CSSDirection::Top as usize]
    }
    pub fn get_width(&self) -> f32 {
        self.layout_result.dim[Dimension::Width as usize]
    }
    pub fn get_height(&self) -> f32 {
        self.layout_result.dim[Dimension::Height as usize]
    }
}
