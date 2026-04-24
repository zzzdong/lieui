use crate::geometry::Size;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConstraint {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl LayoutConstraint {
    pub const UNBOUNDED: Self = Self {
        min_width: 0.0,
        max_width: f32::INFINITY,
        min_height: 0.0,
        max_height: f32::INFINITY,
    };

    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    pub fn clamp_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    pub fn clamp_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// 获取最大尺寸
    pub fn max_size(&self) -> Size {
        Size::new(self.max_width, self.max_height)
    }
}

impl Default for LayoutConstraint {
    fn default() -> Self {
        Self::UNBOUNDED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraint_clamp() {
        let constraint = LayoutConstraint {
            min_width: 10.0,
            max_width: 100.0,
            min_height: 20.0,
            max_height: 200.0,
        };

        assert_eq!(constraint.clamp_width(5.0), 10.0);
        assert_eq!(constraint.clamp_width(50.0), 50.0);
        assert_eq!(constraint.clamp_width(150.0), 100.0);
    }
}
