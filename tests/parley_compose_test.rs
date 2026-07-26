use lieui::text::{align_to_utf8_boundary, editor_cursor_geometry};
use lieui::view::paint::TextStyle;
use lieui::widget::input::InputState;

fn fresh_editor() -> (InputState, TextStyle) {
    let style = TextStyle::default();
    let state = InputState::new(&style);
    (state, style)
}

#[test]
fn compose_empty_then_nonempty_then_commit() {
    let (mut state, style) = fresh_editor();
    lieui::text::with_text_contexts(|fc, lc| {
        let mut driver = state.editor.driver(fc, lc);
        // parley 的 set_compose 不允许空字符串，空预编辑应调用 clear_compose。
        driver.clear_compose();
        driver.set_compose("ni", Some((2, 2)));
        driver.set_compose("你", Some((3, 3)));
        driver.clear_compose();
        driver.insert_or_replace_selection("你");
    });
    let _ = editor_cursor_geometry(&mut state.editor, &style, Some(200.0), 1.0);
}

#[test]
fn compose_with_utf8_boundary_correction() {
    let (mut state, style) = fresh_editor();
    lieui::text::with_text_contexts(|fc, lc| {
        let text = "你好";
        // 故意传入不合法的字节偏移，应被修正。
        let start = align_to_utf8_boundary(text, 1);
        let end = align_to_utf8_boundary(text, 4);
        state
            .editor
            .driver(fc, lc)
            .set_compose(text, Some((start, end)));
    });
    let _ = editor_cursor_geometry(&mut state.editor, &style, Some(200.0), 1.0);
}
