//! PDFKit — PDF 页面管理小工具（lieui 演示版）
//!
//! 纯 UI 演示：用内存假数据模拟 PDF 页面管理。UI 结构与交互参照
//! `../pdfkit`（真实应用）移植：
//!   - 侧栏每页缩略图 + 选择 + 上移/下移重排
//!   - 未加载时居中提示
//!   - 图标按钮工具栏（分组 + 禁用态 + tooltip）
//!   - 完整状态栏
//!
//! 布局:
//!   ┌─ Header ──────────────────────────────────────┐
//!   │  PDFKit  |  sample.pdf  |           [Import]   │
//!   ├─ Sidebar ─┬─ Toolbar ─────────────────────────┤
//!   │  页面      │  [打开][保存][合并] | [旋转] | ...│
//!   │  ┌───┐    ├───────────────────────────────────┤
//!   │  │ 1 │▴▾ │        Page Preview                │
//!   │  │ 2 │▴▾ │        612 × 792                   │
//!   │  ...     │                                    │
//!   ├─ Status ─┴───────────────────────────────────┤
//!   │  页面数: 8  |  已选择: 1  |  Ready              │
//!   └───────────────────────────────────────────────┘

use lieui::geometry::Color;
use lieui::layout::types::FlexAlign;
use lieui::prelude::*;
use lieui::state::State;
use lieui::widget::Widget;

fn hex(c: u32) -> Color {
    Color::new(
        ((c >> 16) & 0xff) as u8,
        ((c >> 8) & 0xff) as u8,
        (c & 0xff) as u8,
    )
}

#[derive(Clone, Default)]
struct PageInfo {
    index: usize,
    label: String,
    dimensions: String,
    preview_text: String,
}

fn sample_pages() -> Vec<PageInfo> {
    (1..=8)
        .map(|i| PageInfo {
            index: i - 1,
            label: format!("Page {}", i),
            dimensions: "612 × 792 pt".to_string(),
            preview_text: format!("Text content extracted from page {}.", i),
        })
        .collect()
}

fn divider(border: Color) -> impl Widget {
    Container::new().height(1.0).background(border)
}

/// 垂直分隔线。
fn sep(border: Color) -> impl Widget {
    Container::new().width(1.0).height(20.0).background(border)
}

// ========== 应用 ==========

fn main() {
    let pages = State::new(Vec::<PageInfo>::new());
    let selected = State::new(Vec::<bool>::new());
    let current = State::new(None::<usize>);
    // 每页旋转角度（0/90/180/270）。
    let rotations = State::new(Vec::<i32>::new());
    let status = State::new("Ready — click Import PDF... to begin".to_string());

    // ── 加载示例文档 ──
    let load = {
        let p = pages.clone();
        let s = selected.clone();
        let c = current.clone();
        let r = rotations.clone();
        let st = status.clone();
        move || {
            let sm = sample_pages();
            let n = sm.len();
            p.set(sm);
            s.set(vec![false; n]);
            r.set(vec![0; n]);
            c.set(Some(0));
            st.set(format!("已加载 {} 页", n));
        }
    };

    // ── 切换选择 ──
    let tog = {
        let s = selected.clone();
        move |idx: usize| {
            s.update(|v| {
                if let Some(b) = v.get_mut(idx) {
                    *b = !*b;
                }
            })
        }
    };

    // ── 设置当前页 ──
    let set_cur = {
        let c = current.clone();
        move |idx: usize| c.set(Some(idx))
    };

    // ── 删除选中页 ──
    let del = {
        let p = pages.clone();
        let s = selected.clone();
        let c = current.clone();
        let r = rotations.clone();
        let st = status.clone();
        move || {
            let cnt = {
                let sel = s.get();
                sel.iter().filter(|&&b| b).count()
            };
            if cnt == 0 {
                st.set("未选择页面".to_string());
                return;
            }
            let list = p.get();
            let rot = r.get();
            let kept: Vec<_> = list
                .iter()
                .enumerate()
                .filter(|(i, _)| !s.get().get(*i).copied().unwrap_or(false))
                .map(|(i, info)| (info.clone(), rot.get(i).copied().unwrap_or(0)))
                .enumerate()
                .map(|(ni, (mut inf, _rot))| {
                    inf.index = ni;
                    inf.label = format!("Page {}", ni + 1);
                    inf
                })
                .collect();
            let n = kept.len();
            p.set(kept);
            s.set(vec![false; n]);
            r.set(vec![0; n]);
            c.set(if n > 0 { Some(0) } else { None });
            st.set(format!("已删除 {cnt} 页，剩余 {n} 页"));
        }
    };

    // ── 上移/下移页面 ──
    let move_page = {
        let p = pages.clone();
        let s = selected.clone();
        let r = rotations.clone();
        let c = current.clone();
        let st = status.clone();
        move |idx: usize, dir: i32| {
            let n = p.get().len();
            let to = idx as i64 + dir as i64;
            if idx >= n || to < 0 || to >= n as i64 {
                st.set(
                    if dir < 0 {
                        "已是第一页"
                    } else {
                        "已是最后一页"
                    }
                    .to_string(),
                );
                return;
            }
            let mut list = (*p.get()).clone();
            let mut rot = (*r.get()).clone();
            list.swap(idx, to as usize);
            rot.swap(idx, to as usize);
            p.set(list);
            r.set(rot);
            s.set(vec![false; n]);
            c.set(Some(to as usize));
            st.set(format!(
                "第 {} 页{}",
                idx + 1,
                if dir < 0 { "上移" } else { "下移" }
            ));
        }
    };

    // ── 旋转当前页 ──
    let rotate_current = {
        let r = rotations.clone();
        let c = current.clone();
        let st = status.clone();
        move |delta: i32| {
            let Some(idx) = *c.get() else {
                st.set("未加载文档".to_string());
                return;
            };
            r.update(|v| {
                if let Some(deg) = v.get_mut(idx) {
                    *deg = (*deg + delta).rem_euclid(360);
                }
            });
            st.set(format!("已旋转第 {} 页 {}°", idx + 1, delta));
        }
    };

    // ── 旋转选中页 ──
    let rotate_selected = {
        let r = rotations.clone();
        let s = selected.clone();
        let st = status.clone();
        move |delta: i32| {
            let mut n = 0;
            r.update(|v| {
                for (i, deg) in v.iter_mut().enumerate() {
                    if s.get().get(i).copied().unwrap_or(false) {
                        *deg = (*deg + delta).rem_euclid(360);
                        n += 1;
                    }
                }
            });
            if n == 0 {
                st.set("未选择页面".to_string());
            } else {
                st.set(format!("已旋转 {n} 页 {}°", delta));
            }
        }
    };

    // ── 全选 / 取消全选 ──
    let select_all = {
        let s = selected.clone();
        let p = pages.clone();
        move || s.set(vec![true; p.get().len()])
    };
    let deselect_all = {
        let s = selected.clone();
        let p = pages.clone();
        move || s.set(vec![false; p.get().len()])
    };

    // ── 提取选中页 ──
    let ext = {
        let s = selected.clone();
        let st = status.clone();
        move || {
            let c = s.get().iter().filter(|&&b| b).count();
            st.set(if c == 0 {
                "未选择页面".to_string()
            } else {
                format!("已提取 {c} 页 → extracted.pdf")
            });
        }
    };

    // ── Builder ──
    let app = Application::new(WindowConfig::new().size(960.0, 640.0), move |_ctx| {
        let c = |h| hex(h);
        let hbg = c(0x1e293b);
        let sbb = c(0xf1f5f9);
        let acc = c(0x3b82f6);
        let acl = c(0xdbeafe);
        let tp = c(0x0f172a);
        let ts = c(0x64748b);
        let tm = c(0x94a3b8);
        let bd = c(0xe2e8f0);
        let sbg = c(0xf8fafc);
        let cbgb = c(0xf8fafc);
        let tb = c(0xdbeafe);

        let pl = pages.get();
        let sl = selected.get();
        let rl = rotations.get();
        let cu = *current.get();
        let sc = sl.iter().filter(|&&b| b).count();
        let stxt: String = status.get().clone();
        let total = pl.len();

        // ── 左侧：页面列表 ──
        let side = if total == 0 {
            // 未加载：居中提示。
            Column::new().expand(true).child(
                Column::new()
                    .expand(true)
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Center)
                    .child(Text::new("未加载页面").font_size(11.0).color(tm)),
            )
        } else {
            // 标题仅标识区块；页数交给状态栏。
            let mut side = Column::new()
                .spacing(4.0)
                .child(Text::new("页面").font_size(13.0).color(ts));
            for (idx, info) in pl.iter().enumerate() {
                let chk = *sl.get(idx).unwrap_or(&false);
                let curp = cu == Some(idx);
                let rot = *rl.get(idx).unwrap_or(&0);
                let dim = if rot != 0 {
                    format!("{} 旋转{}°", info.dimensions, rot)
                } else {
                    info.dimensions.clone()
                };

                let t = tog.clone();
                let c = set_cur.clone();
                let up = move_page.clone();
                let down = move_page.clone();

                side = side.child(
                    Container::new()
                        .background(if curp { acl } else { Color::WHITE })
                        .child(
                            Row::new()
                                .spacing(10.0)
                                .align_items(FlexAlign::Center)
                                .child(
                                    Container::new()
                                        .expand(true)
                                        .on_click(move || c(idx))
                                        .child(
                                            Row::new()
                                                .spacing(10.0)
                                                .align_items(FlexAlign::Center)
                                                .child(
                                                    Container::new()
                                                        .width(48.0)
                                                        .height(64.0)
                                                        .background(tb)
                                                        .child(
                                                            Column::new()
                                                                .expand(true)
                                                                .justify_content(FlexAlign::Center)
                                                                .align_items(FlexAlign::Center)
                                                                .child(
                                                                    Text::new(format!(
                                                                        "{}",
                                                                        idx + 1
                                                                    ))
                                                                    .font_size(28.0)
                                                                    .color(acc),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    Column::new()
                                                        .spacing(2.0)
                                                        .child(
                                                            Text::new(&info.label)
                                                                .font_size(13.0)
                                                                .color(tp),
                                                        )
                                                        .child(
                                                            Text::new(&dim)
                                                                .font_size(10.0)
                                                                .color(tm),
                                                        ),
                                                ),
                                        ),
                                )
                                .child(Checkbox::new(chk).label("选择").on_click(move || t(idx)))
                                .child(
                                    Column::new()
                                        .spacing(2.0)
                                        .child(
                                            IconButton::new(IconName::KeyboardArrowUp)
                                                .size(18.0)
                                                .icon_size(12.0)
                                                .on_click(move || up(idx, -1)),
                                        )
                                        .child(
                                            IconButton::new(IconName::KeyboardArrowDown)
                                                .size(18.0)
                                                .icon_size(12.0)
                                                .on_click(move || down(idx, 1)),
                                        ),
                                ),
                        ),
                );
            }
            side
        };

        let sidebar = Container::new()
            .width(220.0)
            .background(sbb)
            .child(ScrollView::expand().child(side));

        // ── 右侧 ──
        let preview: Container = if let Some(idx) = cu {
            if let Some(info) = pl.get(idx) {
                let rot = *rl.get(idx).unwrap_or(&0);
                let card = Container::new().expand(true).padding(24.0).child(
                    Column::new()
                        .spacing(12.0)
                        .child(
                            Column::new()
                                .spacing(4.0)
                                .child(Text::new(&info.label).font_size(24.0).color(tp))
                                .child(
                                    Text::new(format!(
                                        "{}{}",
                                        info.dimensions,
                                        if rot != 0 {
                                            format!("  (旋转{}°)", rot)
                                        } else {
                                            String::new()
                                        }
                                    ))
                                    .font_size(13.0)
                                    .color(ts),
                                ),
                        )
                        .child(divider(bd))
                        .child(
                            Column::new()
                                .spacing(6.0)
                                .child(Text::new("Text preview:").font_size(11.0).color(tm))
                                .child(Text::new(&info.preview_text).font_size(12.0).color(tp)),
                        ),
                );
                Container::new().expand(true).background(cbgb).child(
                    Column::new()
                        .expand(true)
                        .justify_content(FlexAlign::Center)
                        .align_items(FlexAlign::Stretch)
                        .child(card),
                )
            } else {
                Container::new().expand(true).background(cbgb)
            }
        } else {
            Container::new().expand(true).background(cbgb).child(
                Column::new()
                    .expand(true)
                    .justify_content(FlexAlign::Center)
                    .align_items(FlexAlign::Center)
                    .child(Text::new("打开 PDF 以预览").font_size(14.0).color(tm)),
            )
        };

        // ── 工具栏（图标按钮分组 + 禁用态）──
        let has_sel = sc > 0;
        let load_btn = IconButton::new(IconName::FolderOpen)
            .tooltip("导入 PDF 文件")
            .on_click(load.clone());
        let save_btn = IconButton::new(IconName::Save)
            .tooltip("保存当前文档")
            .disabled(total == 0)
            .on_click({
                let st = status.clone();
                move || {
                    st.set(
                        if total == 0 {
                            "未加载文档"
                        } else {
                            "已保存 → sample.pdf"
                        }
                        .to_string(),
                    )
                }
            });
        let merge_btn = IconButton::new(IconName::CreateNewFolder)
            .tooltip("合并多个 PDF 文件（独立模式）")
            .on_click({
                let st = status.clone();
                move || st.set("合并模式：选择文件后合并输出".to_string())
            });
        let rotate_cw = IconButton::new(IconName::Rotate90DegreesCw)
            .tooltip("当前页顺时针旋转 90°")
            .disabled(total == 0)
            .on_click({
                let f = rotate_current.clone();
                move || f(90)
            });
        let rotate_ccw = IconButton::new(IconName::Rotate90DegreesCcw)
            .tooltip("当前页逆时针旋转 90°")
            .disabled(total == 0)
            .on_click({
                let f = rotate_current.clone();
                move || f(-90)
            });
        let rotate_sel_cw = IconButton::new(IconName::RotateRight)
            .tooltip("选中页顺时针旋转 90°")
            .disabled(!has_sel)
            .on_click({
                let f = rotate_selected.clone();
                move || f(90)
            });
        let rotate_sel_ccw = IconButton::new(IconName::RotateLeft)
            .tooltip("选中页逆时针旋转 90°")
            .disabled(!has_sel)
            .on_click({
                let f = rotate_selected.clone();
                move || f(-90)
            });
        let delete_btn = IconButton::new(IconName::Delete)
            .tooltip("删除选中页面")
            .disabled(!has_sel)
            .on_click(del.clone());
        let extract_btn = IconButton::new(IconName::OpenInNew)
            .tooltip("提取选中页面为新 PDF")
            .disabled(!has_sel)
            .on_click(ext.clone());
        let select_all_btn = IconButton::new(IconName::Done)
            .tooltip("全选所有页面")
            .on_click(select_all.clone());
        let deselect_btn = IconButton::new(IconName::Close)
            .tooltip("取消全选")
            .on_click(deselect_all.clone());

        // flex_shrink(0)：toolbar 高度固定，不受下方内容高度影响。
        let toolbar = Column::new()
            .spacing(0.0)
            .flex_shrink(0.0)
            .child(
                Container::new().padding(8.0).child(
                    Row::new()
                        .expand(true)
                        .justify_content(FlexAlign::SpaceBetween)
                        .align_items(FlexAlign::Center)
                        .child(
                            Row::new()
                                .spacing(4.0)
                                .align_items(FlexAlign::Center)
                                .child(load_btn)
                                .child(save_btn)
                                .child(sep(bd))
                                .child(merge_btn)
                                .child(sep(bd))
                                .child(rotate_cw)
                                .child(rotate_ccw)
                                .child(rotate_sel_cw)
                                .child(rotate_sel_ccw)
                                .child(sep(bd))
                                .child(delete_btn)
                                .child(extract_btn)
                                .child(sep(bd))
                                .child(select_all_btn)
                                .child(deselect_btn),
                        )
                        .child(
                            Text::new(format!("已选择 {sc}"))
                                .font_size(12.0)
                                .color(if sc > 0 { acc } else { tm }),
                        ),
                ),
            )
            .child(divider(bd));

        let right = Column::new()
            .spacing(0.0)
            .expand(true)
            .child(toolbar)
            .child(preview);

        // ── 底部状态栏 ──
        let status_bar = Container::new().background(sbg).child(
            Row::new()
                .spacing(24.0)
                .align_items(FlexAlign::Center)
                .child(
                    Text::new(format!("页面数: {total}"))
                        .font_size(11.0)
                        .color(ts),
                )
                .child(Text::new(format!("已选择: {sc}")).font_size(11.0).color(ts))
                .child(Text::new(&stxt).font_size(11.0).color(tm)),
        );

        // ── 页面结构 ──
        Box::new(
            Container::new().expand(true).child(
                Column::new()
                    .spacing(0.0)
                    .expand(true)
                    .child(
                        Container::new().background(hbg).child(
                            Row::new()
                                .justify_content(FlexAlign::SpaceBetween)
                                .align_items(FlexAlign::Center)
                                .child(
                                    Row::new()
                                        .spacing(16.0)
                                        .align_items(FlexAlign::Center)
                                        .child(
                                            Text::new("PDFKit").font_size(20.0).color(Color::WHITE),
                                        )
                                        .child(Text::new("|").font_size(16.0).color(c(0x475569)))
                                        .child(
                                            Text::new(if total > 0 { "sample.pdf" } else { "-" })
                                                .font_size(13.0)
                                                .color(tm),
                                        ),
                                )
                                .child(Button::new("Import PDF...").on_click(load.clone())),
                        ),
                    )
                    .child(
                        Row::new()
                            .spacing(0.0)
                            .expand(true)
                            .align_items(FlexAlign::Stretch)
                            .child(sidebar)
                            .child(right),
                    )
                    .child(divider(bd))
                    .child(status_bar),
            ),
        )
    });

    #[cfg(feature = "inspector")]
    let app = app.inspector(true);

    app.run();
}
