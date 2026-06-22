//! Widget ID - 使用 slotmap 的 generational key
//!
//! slotmap 提供的 key 包含 (generation, index)，天然防止悬垂指针问题

use slotmap::new_key_type;

// 定义 WidgetId 作为 slotmap 的 key type
// 这会自动生成一个包含 generation 和 index 的结构体
// 并实现 Clone, Copy, Debug, PartialEq, Eq, Hash 等必要 trait
new_key_type! {
    pub struct WidgetId;
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn test_widget_id_unique() {
        let mut map: SlotMap<WidgetId, i32> = SlotMap::with_key();
        let id1 = map.insert(1);
        let id2 = map.insert(2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_widget_id_generational() {
        let mut map: SlotMap<WidgetId, i32> = SlotMap::with_key();
        let id1 = map.insert(1);

        // 删除后，相同 index 的 key 会因为 generation 不同而无法访问旧数据
        map.remove(id1);

        // id1 现在无效
        assert!(map.get(id1).is_none());

        // 新插入可能使用相同 index，但 generation 不同
        let id2 = map.insert(2);
        assert_ne!(id1, id2); // 即使 index 相同，generation 不同
    }
}
