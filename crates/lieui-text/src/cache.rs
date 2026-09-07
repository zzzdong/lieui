//! 测度缓存键与确定性哈希
//!
//! 布局 pass 会对同一文本节点测量多次（flex-basis / 交叉轴 / stretch），
//! 文本整形是纯 CPU 大头，必须按 (文本, 样式, 换行, 最大宽度) 缓存。

use std::hash::{BuildHasherDefault, Hash, Hasher};

/// FNV-1a：确定性哈希器。
///
/// 不使用 `RandomState`（SipHash）—— 它每进程随机种子，会让遍历顺序不可复现
/// （见设计 §6.3「确定性三规范」）。
#[derive(Default)]
pub struct FnvHasher(u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        if self.0 == 0 { FNV_OFFSET } else { self.0 }
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { FNV_OFFSET } else { self.0 };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        self.0 = h;
    }
}

pub type FnvBuildHasher = BuildHasherDefault<FnvHasher>;

/// `u32::MAX` 表示「无最大宽度约束」
pub const NO_MAX_WIDTH: u32 = u32::MAX;

/// 测度缓存键
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MeasureKey {
    pub text_hash: u64,
    pub style_hash: u64,
    pub wrap: bool,
    pub max_width_bits: u32,
}

impl MeasureKey {
    pub fn new(text_hash: u64, style_hash: u64, wrap: bool, max_width: Option<f32>) -> Self {
        let max_width_bits = match max_width {
            Some(w) if w.is_finite() => w.to_bits(),
            _ => NO_MAX_WIDTH,
        };
        Self {
            text_hash,
            style_hash,
            wrap,
            max_width_bits,
        }
    }
}

/// 缓存统计
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl CacheStats {
    pub fn total(&self) -> u64 {
        self.hits + self.misses
    }
    /// 命中率。无查询时返回 1.0（避免 0/0 影响断言）。
    pub fn hit_rate(&self) -> f32 {
        if self.total() == 0 {
            1.0
        } else {
            self.hits as f32 / self.total() as f32
        }
    }
}

/// 计算文本的确定性哈希
pub fn hash_str(s: &str) -> u64 {
    let mut h = FnvHasher::default();
    s.hash(&mut h);
    h.finish()
}

/// 增量混合器：把样式字段压成一个 u64
pub struct HashMix(u64);

impl HashMix {
    pub fn new() -> Self {
        Self(FNV_OFFSET)
    }
    pub fn mix_u64(&mut self, v: u64) {
        self.0 ^= v;
        self.0 = self.0.wrapping_mul(FNV_PRIME);
    }
    pub fn mix_f32(&mut self, v: f32) {
        self.mix_u64(v.to_bits() as u64);
    }
    pub fn mix_bool(&mut self, v: bool) {
        self.mix_u64(v as u64);
    }
    pub fn mix_str(&mut self, s: &str) {
        self.mix_u64(hash_str(s));
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for HashMix {
    fn default() -> Self {
        Self::new()
    }
}
