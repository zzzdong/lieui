use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::attributes::AttrError;
use vello_cpu::peniko::Color;

use crate::element::style::{Style, StyleError};

type Result<T> = std::result::Result<T, StyleError>;

#[derive(Debug, Clone)]
pub struct View {
    pub children: Vec<Element>,
}

#[derive(Debug, Clone)]
pub enum Element {
    Div(Div),
    Text(Text),
    Image(Image),
}

#[derive(Debug, Clone)]
pub struct Div {
    pub style: Style,
    pub children: Vec<Element>,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub style: Style,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub style: Style,
    pub src: String,
}

// 在 read_view 返回前，跑一次继承
fn inherit_styles(root: &mut Element) {
    fn walk(parent_style: &Style, node: &mut Element) {
        let child_raw = match node {
            Element::Div(d) => &mut d.style,
            Element::Text(t) => &mut t.style,
            Element::Image(i) => &mut i.style,
        };
        // 仅继承“可继承”字段
        if child_raw.color == Color::TRANSPARENT {
            child_raw.color = parent_style.color;
        }
        if child_raw.font_size == 0.0 {
            child_raw.font_size = parent_style.font_size;
        }
        if child_raw.font_family == "sans-serif" {
            child_raw.font_family = parent_style.font_family.clone();
        }
        // background/border/margin 不继承，保持本地或默认值

        // 继续向下
        let next_parent = child_raw.clone();
        match node {
            Element::Div(d) => d.children.iter_mut().for_each(|c| walk(&next_parent, c)),
            Element::Text(_) => {}
            Element::Image(_) => {}
        }
    }
    // 虚拟根样式：黑字 14 px sans-serif
    let root_style = Style {
        color: Color::from_rgb8(0, 0, 0),
        font_size: 14.0,
        font_family: "sans-serif".into(),
        ..Default::default()
    };
    match root {
        Element::Div(d) => d.children.iter_mut().for_each(|c| walk(&root_style, c)),
        Element::Text(_) => {}
        Element::Image(_) => {}
    }
}

pub fn read_view(xml: &str) -> Result<View> {
    let mut view = parse_view_xml(xml)?;
    for elem in view.children.iter_mut() {
        inherit_styles(elem);
    }
    Ok(view)
}

pub fn parse_view_xml(xml: &str) -> Result<View> {
    let mut reader = Reader::from_str(xml);

    reader.config_mut().trim_text(true);

    enum StackElem {
        View(View),
        Div(Div),
        Text(Text),
        Image(Image),
    }
    let mut stack: Vec<StackElem> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let tag = std::str::from_utf8(&e.name().as_ref())?.to_lowercase();

                let style = Style::from_attrs(
                    e.attributes()
                        .collect::<std::result::Result<Vec<_>, AttrError>>()?,
                )?;

                match tag.as_str() {
                    "view" => {
                        stack.push(StackElem::View(View {
                            children: Vec::new(),
                        }));
                    }
                    "div" => {
                        stack.push(StackElem::Div(Div {
                            style,
                            children: Vec::new(),
                        }));
                    }
                    "text" => {
                        stack.push(StackElem::Text(Text {
                            style,
                            text: String::new(),
                        }));
                    }
                    "image" => {
                        let mut src = String::new();
                        for attr in e.attributes() {
                            let attr = attr?;
                            if std::str::from_utf8(&attr.key.as_ref())?.eq_ignore_ascii_case("src")
                            {
                                src = std::str::from_utf8(&attr.value)?.to_string();
                            }
                        }
                        stack.push(StackElem::Image(Image { style, src }));
                    }
                    _ => {}
                }
            }
            Event::Text(e) => {
                let text = e.decode()?;
                if let Some(StackElem::Text(t)) = stack.last_mut() {
                    t.text.push_str(&text);
                }
            }
            Event::End(e) => {
                // let tag = std::str::from_utf8(&e.name().as_ref())?.to_lowercase();
                if let Some(elem) = stack.pop() {
                    match elem {
                        StackElem::View(v) => return Ok(v),
                        StackElem::Div(d) => {
                            let div = Element::Div(d);
                            if let Some(parent) = stack.last_mut() {
                                match parent {
                                    StackElem::View(pv) => pv.children.push(div),
                                    StackElem::Div(pd) => pd.children.push(div),
                                    _ => {}
                                }
                            }
                        }
                        StackElem::Text(t) => {
                            let text = Element::Text(t);
                            if let Some(parent) = stack.last_mut() {
                                match parent {
                                    StackElem::View(pv) => pv.children.push(text),
                                    StackElem::Div(pd) => pd.children.push(text),
                                    _ => {}
                                }
                            }
                        }
                        StackElem::Image(i) => {
                            let img = Element::Image(i);
                            if let Some(parent) = stack.last_mut() {
                                match parent {
                                    StackElem::View(pv) => pv.children.push(img),
                                    StackElem::Div(pd) => pd.children.push(img),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Err(StyleError::Attr("Unexpected EOF".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        let xml = r#"
            <view>
                <div width="100" height="100" background-color="red" border-width="1" border-color="black" border-radius="2">
                    <text font-size="12" color="white">
                        Hello, world!
                    </text>
                </div>
            </view>
        "#;

        let view = read_view(xml).unwrap();
        println!("{:#?}", view);
    }
}
