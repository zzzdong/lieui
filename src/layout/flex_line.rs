//! FlexLine — 弹性行管理
//!
//! 参考 Taitank::FlexLine
//! 用于将 flex items 按行分组，管理 flex-grow/flex-shrink 分配

use crate::layout::flex_node::FlexNode;
use crate::layout::types::*;

/// Flex 符号
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexSign {
    PositiveFlexibility,
    NegativeFlexibility,
}

/// 弹性行
#[derive(Debug)]
pub struct FlexLine {
    pub items: Vec<usize>,
    pub container_main_inner_size: f32,
    pub sum_hypothetical_main_size: f32,
    pub total_flex_grow: f32,
    pub total_flex_shrink: f32,
    pub total_weighted_flex_shrink: f32,
    pub line_cross_size: f32,
    pub initial_free_space: f32,
    pub remaining_free_space: f32,
    pub gap: f32,
}

impl Default for FlexLine {
    fn default() -> Self {
        Self::new()
    }
}

impl FlexLine {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            container_main_inner_size: 0.0,
            sum_hypothetical_main_size: 0.0,
            total_flex_grow: 0.0,
            total_flex_shrink: 0.0,
            total_weighted_flex_shrink: 0.0,
            line_cross_size: 0.0,
            initial_free_space: 0.0,
            remaining_free_space: 0.0,
            gap: 0.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn sign(&self) -> FlexSign {
        if self.sum_hypothetical_main_size < self.container_main_inner_size {
            FlexSign::PositiveFlexibility
        } else {
            FlexSign::NegativeFlexibility
        }
    }

    pub fn add_item(
        &mut self,
        idx: usize,
        hypothetical_main_axis_margin_boxsize: f32,
        flex_grow: f32,
        flex_shrink: f32,
        flex_base_size: f32,
    ) {
        self.sum_hypothetical_main_size += hypothetical_main_axis_margin_boxsize;
        self.total_flex_grow += flex_grow;
        self.total_flex_shrink += flex_shrink;
        self.total_weighted_flex_shrink += flex_shrink * flex_base_size;
        self.items.push(idx);
    }

    /// 冻结不可变 item
    pub fn freeze_inflexible_items(
        &mut self,
        main_axis: FlexDirection,
        children: &mut [FlexNode],
    ) -> Vec<usize> {
        let flex_sign = self.sign();
        self.remaining_free_space =
            self.container_main_inner_size - self.sum_hypothetical_main_size;

        let mut inflexible_items = Vec::new();

        for &idx in &self.items {
            let freeze = {
                let item = &children[idx];
                let flex_factor = if flex_sign == FlexSign::PositiveFlexibility {
                    item.style.flex_grow
                } else {
                    item.style.flex_shrink
                };
                flex_factor == 0.0
                    || (flex_sign == FlexSign::PositiveFlexibility
                        && item.layout_result.flex_base_size
                            > item.layout_result.hypothetical_main_axis_size)
                    || (flex_sign == FlexSign::NegativeFlexibility
                        && item.layout_result.flex_base_size
                            < item.layout_result.hypothetical_main_axis_size)
            };

            if freeze {
                let new_dim = children[idx].layout_result.hypothetical_main_axis_size;
                children[idx].layout_result.dim[K_AXIS_DIM[main_axis as usize] as usize] = new_dim;
                inflexible_items.push(idx);
            }
        }

        self.freeze_violations(&inflexible_items, main_axis, children);
        self.initial_free_space = self.remaining_free_space;
        inflexible_items
    }

    /// 冻结违规 item
    pub fn freeze_violations(
        &mut self,
        violations: &[usize],
        main_axis: FlexDirection,
        children: &mut [FlexNode],
    ) {
        for &idx in violations {
            if children[idx].is_frozen {
                continue;
            }

            let dim = K_AXIS_DIM[main_axis as usize] as usize;
            let layout_dim = children[idx].layout_result.dim[dim];
            self.remaining_free_space -=
                layout_dim - children[idx].layout_result.hypothetical_main_axis_size;

            self.total_flex_grow -= children[idx].style.flex_grow;
            self.total_flex_shrink -= children[idx].style.flex_shrink;
            self.total_weighted_flex_shrink -=
                children[idx].style.flex_shrink * children[idx].layout_result.flex_base_size;
            self.total_weighted_flex_shrink = self.total_weighted_flex_shrink.max(0.0);

            children[idx].is_frozen = true;
        }
    }

    /// 解析弹性长度 (W3C §9.7)
    pub fn resolve_flexible_lengths(
        &mut self,
        main_axis: FlexDirection,
        children: &mut [FlexNode],
    ) -> bool {
        let flex_sign = self.sign();
        let sum_flex_factors = if flex_sign == FlexSign::PositiveFlexibility {
            self.total_flex_grow
        } else {
            self.total_flex_shrink
        };

        let mut remaining_free_space = self.remaining_free_space;
        if sum_flex_factors > 0.0 && sum_flex_factors < 1.0 {
            let value = self.initial_free_space * sum_flex_factors;
            if value < remaining_free_space {
                remaining_free_space = value;
            }
        }

        let mut used_free_space = 0.0f32;
        let mut total_violation = 0.0f32;
        let mut min_violations = Vec::new();
        let mut max_violations = Vec::new();

        for &idx in &self.items {
            if children[idx].is_frozen {
                continue;
            }

            let extra_space = {
                let item = &children[idx];
                if remaining_free_space > 0.0
                    && self.total_flex_grow > 0.0
                    && flex_sign == FlexSign::PositiveFlexibility
                {
                    remaining_free_space * item.style.flex_grow / self.total_flex_grow
                } else if remaining_free_space < 0.0
                    && self.total_weighted_flex_shrink > 0.0
                    && flex_sign == FlexSign::NegativeFlexibility
                {
                    remaining_free_space
                        * item.style.flex_shrink
                        * item.layout_result.flex_base_size
                        / self.total_weighted_flex_shrink
                } else {
                    0.0
                }
            };

            let violation = if extra_space.is_finite() {
                let item_main_size =
                    children[idx].layout_result.hypothetical_main_axis_size + extra_space;
                let adjust = children[idx].bound_axis(main_axis, item_main_size);
                children[idx].layout_result.dim[K_AXIS_DIM[main_axis as usize] as usize] = adjust;
                used_free_space += adjust - children[idx].layout_result.hypothetical_main_axis_size;
                adjust - item_main_size
            } else {
                0.0
            };

            if violation > 0.0 {
                min_violations.push(idx);
            } else if violation < 0.0 {
                max_violations.push(idx);
            }
            total_violation += violation;
        }

        if total_violation != 0.0 {
            let to_freeze = if total_violation < 0.0 {
                max_violations
            } else {
                min_violations
            };
            self.freeze_violations(&to_freeze, main_axis, children);
        } else {
            self.remaining_free_space -= used_free_space;
        }

        total_violation == 0.0
    }

    /// 行内对齐
    pub fn align_items(
        &mut self,
        main_axis: FlexDirection,
        children: &mut [FlexNode],
        justify_content: FlexAlign,
        parent_padding_border_start: f32,
        parent_layout_dim: f32,
    ) {
        self.remaining_free_space = self.container_main_inner_size;
        let mut auto_margin_count = 0;

        for &idx in &self.items {
            let dim = {
                let item = &children[idx];
                let di = K_AXIS_DIM[main_axis as usize] as usize;
                if is_defined(item.layout_result.dim[di]) {
                    item.layout_result.dim[di]
                } else {
                    0.0
                }
            };
            self.remaining_free_space -= dim + children[idx].get_margin(main_axis);
            if children[idx].is_auto_start_margin(main_axis) {
                auto_margin_count += 1;
            }
            if children[idx].is_auto_end_margin(main_axis) {
                auto_margin_count += 1;
            }
        }

        let auto_margin = if self.remaining_free_space > 0.0 && auto_margin_count > 0 {
            let m = self.remaining_free_space / auto_margin_count as f32;
            self.remaining_free_space = 0.0;
            m
        } else {
            0.0
        };

        for &idx in &self.items {
            let item = &mut children[idx];
            if item.is_auto_start_margin(main_axis) {
                item.set_layout_start_margin(main_axis, auto_margin);
            } else {
                item.set_layout_start_margin(main_axis, item.get_start_margin(main_axis));
            }
            if item.is_auto_end_margin(main_axis) {
                item.set_layout_end_margin(main_axis, auto_margin);
            } else {
                item.set_layout_end_margin(main_axis, item.get_end_margin(main_axis));
            }
        }

        let mut offset = parent_padding_border_start;
        let space = match justify_content {
            FlexAlign::Start => 0.0,
            FlexAlign::Center => {
                offset += self.remaining_free_space / 2.0;
                0.0
            }
            FlexAlign::End => {
                offset += self.remaining_free_space;
                0.0
            }
            FlexAlign::SpaceBetween if self.items.len() > 1 => {
                self.remaining_free_space / (self.items.len() - 1) as f32
            }
            FlexAlign::SpaceAround => {
                let s = self.remaining_free_space / self.items.len() as f32;
                offset += s / 2.0;
                s
            }
            FlexAlign::SpaceEvenly => {
                let s = self.remaining_free_space / (self.items.len() + 1) as f32;
                offset += s;
                s
            }
            _ => 0.0,
        };

        let dim_key = K_AXIS_DIM[main_axis as usize] as usize;
        for &idx in &self.items {
            offset += children[idx].get_layout_start_margin(main_axis);
            children[idx].set_layout_start_position(main_axis, offset);

            let child_dim = if is_defined(children[idx].layout_result.dim[dim_key]) {
                children[idx].layout_result.dim[dim_key]
            } else {
                0.0
            };

            let container_dim = if is_defined(parent_layout_dim) {
                parent_layout_dim
            } else {
                0.0
            };
            children[idx].set_layout_end_position(main_axis, container_dim - child_dim - offset);
            offset += child_dim + children[idx].get_layout_end_margin(main_axis) + space + self.gap;
        }
    }
}
