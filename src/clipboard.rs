//! 系统剪贴板访问封装
//!
//! 目前仅支持纯文本的复制/粘贴/剪切。剪贴板句柄以线程本地变量懒加载，
//! 避免在 widget 构建路径中携带额外状态。

use std::cell::RefCell;

thread_local! {
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

fn with_clipboard<R, F: FnOnce(&mut arboard::Clipboard) -> R>(f: F) -> Option<R> {
    CLIPBOARD.with(|cb| {
        let mut cb = cb.borrow_mut();
        if cb.is_none() {
            *cb = arboard::Clipboard::new().ok();
        }
        cb.as_mut().map(f)
    })
}

/// 将文本写入系统剪贴板。
pub fn copy_text(text: &str) {
    let _ = with_clipboard(|cb| cb.set_text(text.to_string()));
}

/// 从系统剪贴板读取文本。
pub fn paste_text() -> Option<String> {
    with_clipboard(|cb| cb.get_text().ok()).flatten()
}

/// 剪切：复制后删除原内容。对 Input 来说，删除逻辑由调用方自行处理。
pub fn cut_text(text: &str) {
    copy_text(text);
}
