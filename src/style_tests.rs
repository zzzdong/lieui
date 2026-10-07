use super::*;

#[test]
fn paint_default_is_opaque_and_unstyled() {
    let p = PaintStyle::default();
    assert_eq!(p.opacity, 1.0);
    assert!(p.background_color.is_none());
    assert!(!p.clip_content);
    assert!(!p.is_interactive());
}

#[test]
fn paint_builder_chains_and_clamps() {
    let p = PaintStyle::new()
        .background(Color::WHITE)
        .hover_background(Color::RED)
        .radius(6.0)
        .border(1.0, Color::BLACK)
        .opacity(3.0)
        .clip(true);
    assert_eq!(p.background_color, Some(Color::WHITE));
    assert_eq!(p.border_radius, 6.0);
    assert_eq!(p.opacity, 1.0); // clamp
    assert!(p.clip_content);
    assert!(p.is_interactive()); // hover_background 存在
}

#[test]
fn text_style_forwards_to_spec() {
    let t = TextStyle::new()
        .font_size(48.0)
        .font_family("serif")
        .text_align(TextAlign::Center)
        .wrap(false)
        .color(Color::RED);
    assert_eq!(t.spec.font_size, 48.0);
    assert_eq!(t.spec.font_family, "serif");
    assert_eq!(t.spec.text_align, TextAlign::Center);
    assert!(!t.spec.wrap);
    assert_eq!(t.color, Color::RED);
    // 排版字段变化必须体现在 spec 上（布局引擎读的就是它）
    assert_eq!(
        t.spec,
        TextSpec::new(48.0)
            .font_family("serif")
            .text_align(TextAlign::Center)
            .wrap(false)
    );
}

#[test]
fn text_style_default_matches_spec_default() {
    let t = TextStyle::default();
    assert_eq!(t.spec, TextSpec::default());
    assert_eq!(t.color, Color::BLACK);
    assert!(!t.is_interactive());
}

#[test]
fn image_style_defaults() {
    let i = ImageStyle::default();
    assert_eq!(i.opacity, 1.0);
    assert_eq!(i.fit, ImageFit::Fill);
    assert_eq!(ImageStyle::new().size(10, 20).fit(ImageFit::Cover).width, 10);
}
