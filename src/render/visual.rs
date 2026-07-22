//! VisualElement — 纯数据渲染描述（基于 liecharts）
use std::sync::Arc;
use crate::geometry::Color;
use crate::text::TextLayout;
pub use kurbo::{Affine, BezPath, Point as KPoint, Rect as KRect, Vec2};

/// 2D 变换
#[derive(Debug, Clone, Copy, Default)]
pub struct Transform {
    pub translate: Vec2, pub rotate: f64, pub scale: Vec2,
}
impl Transform {
    pub fn new() -> Self { Self::default() }
    pub fn translate(x:f64,y:f64)->Self{Self{translate:Vec2::new(x,y),..Default::default()}}
    pub fn to_affine(&self)->Affine{
        Affine::translate(self.translate)*Affine::rotate(self.rotate)*Affine::scale_non_uniform(self.scale.x,self.scale.y)
    }
}

/// 填充+描边样式
#[derive(Debug, Clone, Default)]
pub struct FillStrokeStyle {
    pub fill: Option<Color>, pub stroke: Option<Stroke>,
}
impl FillStrokeStyle {
    pub fn new()->Self{Self::default()}
    pub fn with_fill(mut self,c:Color)->Self{self.fill=Some(c);self}
    pub fn with_stroke(mut self,c:Color,w:f64)->Self{self.stroke=Some(Stroke::new(c,w));self}
}

/// 描边
#[derive(Debug, Clone)]
pub struct Stroke { pub color: Color, pub width: f64 }
impl Stroke { pub fn new(c:Color,w:f64)->Self{Self{color:c,width:w}} }
impl Default for Stroke { fn default()->Self{Self{color:Color::BLACK,width:1.0}} }

/// 带 z_index 的视觉元素
#[derive(Debug, Clone)]
pub struct LayeredElement {
    pub element: VisualElement,
    pub z_index: i32,
    pub element_id: Option<u64>,
}
impl LayeredElement {
    pub fn new(e: VisualElement, z: i32)->Self{Self{element:e,z_index:z,element_id:None}}
    pub fn with_id(mut self,id:u64)->Self{self.element_id=Some(id);self}
    pub fn z_index(&self)->i32{self.z_index}
}

/// 视觉元素枚举（基于 liecharts，使用 kurbo 类型）
pub enum VisualElement {
    Rect { rect: KRect, style: FillStrokeStyle },
    RoundedRect { rect: KRect, radius: f64, style: FillStrokeStyle },
    Circle { center: KPoint, radius: f64, style: FillStrokeStyle },
    Line { start: KPoint, end: KPoint, style: Stroke },
    Path { path: BezPath, style: FillStrokeStyle },
    TextRun {
        text: String, position: KPoint, color: Color,
        font_size: f64, font_family: String, rotation: f64,
        max_width: Option<f64>, layout: Option<Box<TextLayout>>,
    },
    Image {
        bounds: KRect, data: Arc<Vec<u8>>, width: u32, height: u32, opacity: Option<f32>,
    },
    Group { children: Vec<LayeredElement>, transform: Option<Transform> },
}

impl std::fmt::Debug for VisualElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>)->std::fmt::Result{
        match self{
            Self::Rect{rect,style}=>f.debug_struct("Rect").field("rect",rect).field("style",style).finish(),
            Self::RoundedRect{rect,radius,..}=>f.debug_struct("RoundedRect").field("rect",rect).field("radius",radius).finish(),
            Self::TextRun{text,color,font_size,..}=>f.debug_struct("TextRun").field("text",text).field("color",color).field("font_size",font_size).finish(),
            Self::Group{children,transform}=>f.debug_struct("Group").field("children",&children.len()).field("transform",transform).finish(),
            _ => write!(f,"{:?}",std::mem::discriminant(self)),
        }
    }
}

impl Clone for VisualElement {
    fn clone(&self)->Self{
        match self{
            Self::Rect{rect,style}=>Self::Rect{rect:*rect,style:style.clone()},
            Self::RoundedRect{rect,radius,style,..}=>Self::RoundedRect{rect:*rect,radius:*radius,style:style.clone()},
            Self::Circle{center,radius,style}=>Self::Circle{center:*center,radius:*radius,style:style.clone()},
            Self::Line{start,end,style}=>Self::Line{start:*start,end:*end,style:style.clone()},
            Self::Path{path,style}=>Self::Path{path:path.clone(),style:style.clone()},
            Self::TextRun{text,position,color,font_size,font_family,rotation,max_width,layout}=>
                Self::TextRun{text:text.clone(),position:*position,color:*color,font_size:*font_size,
                    font_family:font_family.clone(),rotation:*rotation,max_width:*max_width,layout:layout.clone()},
            Self::Image{bounds,data,width,height,opacity}=>
                Self::Image{bounds:*bounds,data:data.clone(),width:*width,height:*height,opacity:*opacity},
            Self::Group{children,transform}=>Self::Group{children:children.clone(),transform:*transform},
        }
    }
}
