use lieui::text::{create_plain_editor, editor_cursor_geometry, editor_layout_size};
use lieui::view::paint::TextStyle;

#[test]
fn plain_editor_empty_layout_does_not_panic() {
    let style = TextStyle::default();
    let mut editor = create_plain_editor(&style);
    let _ = editor_layout_size(&mut editor, &style, Some(200.0));
}

#[test]
fn plain_editor_empty_caret_does_not_panic() {
    let style = TextStyle::default();
    let mut editor = create_plain_editor(&style);
    let _ = editor_cursor_geometry(&mut editor, &style, Some(200.0), 1.0);
}
