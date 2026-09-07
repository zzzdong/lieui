//! 代际索引 Arena —— 所有 ID 的底层存储
//!
//! ★ 关键：`remove` 时 `generation += 1`。这保证任何持有旧 ID 的代码在 `get`
//! 时静默得到 `None`，而不是读到被复用的新值。

pub struct GenerationalArena<T> {
    slots: Vec<Slot<T>>,
    /// 空闲 index 栈（LIFO 复用，提升缓存局部性）
    free: Vec<u32>,
    len: usize,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

impl<T> Default for GenerationalArena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GenerationalArena<T> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free: Vec::new(),
            len: 0,
        }
    }

    /// 插入一个值，返回 `(index, generation)`
    pub fn insert(&mut self, value: T) -> (u32, u32) {
        self.len += 1;
        match self.free.pop() {
            Some(idx) => {
                let slot = &mut self.slots[idx as usize];
                slot.value = Some(value);
                (idx, slot.generation)
            }
            None => {
                let idx = self.slots.len() as u32;
                self.slots.push(Slot {
                    generation: 0,
                    value: Some(value),
                });
                (idx, 0)
            }
        }
    }

    #[inline]
    pub fn get(&self, index: u32, generation: u32) -> Option<&T> {
        match self.slots.get(index as usize) {
            Some(slot) if slot.generation == generation => slot.value.as_ref(),
            _ => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self, index: u32, generation: u32) -> Option<&mut T> {
        match self.slots.get_mut(index as usize) {
            Some(slot) if slot.generation == generation => slot.value.as_mut(),
            _ => None,
        }
    }

    #[inline]
    pub fn is_alive(&self, index: u32, generation: u32) -> bool {
        matches!(self.slots.get(index as usize), Some(s) if s.generation == generation && s.value.is_some())
    }

    /// 移除。`generation` 不匹配返回 `None`；成功则 generation 自增使旧句柄失效。
    pub fn remove(&mut self, index: u32, generation: u32) -> Option<T> {
        let slot = self.slots.get_mut(index as usize)?;
        if slot.generation != generation {
            return None;
        }
        let v = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(index);
        self.len -= 1;
        Some(v)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// 已分配的槽位数（含空闲）
    #[inline]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// 空闲槽位数
    #[inline]
    pub fn free_len(&self) -> usize {
        self.free.len()
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut a: GenerationalArena<String> = GenerationalArena::new();
        let (i, g) = a.insert("a".into());
        assert_eq!(a.get(i, g).unwrap(), "a");
    }

    #[test]
    fn stale_handle_returns_none_after_remove() {
        let mut a: GenerationalArena<u32> = GenerationalArena::new();
        let (i, g) = a.insert(1);
        assert_eq!(a.remove(i, g), Some(1));
        // 旧句柄失效
        assert!(a.get(i, g).is_none());
        // 槽位被复用，但 generation 已自增 → 旧句柄仍失败
        let (i2, g2) = a.insert(2);
        assert_eq!(i2, i);
        assert_ne!(g2, g);
        assert!(a.get(i, g).is_none());
        assert_eq!(*a.get(i2, g2).unwrap(), 2);
    }

    #[test]
    fn remove_twice_is_none() {
        let mut a: GenerationalArena<u32> = GenerationalArena::new();
        let (i, g) = a.insert(9);
        assert_eq!(a.remove(i, g), Some(9));
        assert_eq!(a.remove(i, g), None);
        assert_eq!(a.len(), 0);
        assert_eq!(a.free_len(), 1);
    }
}
