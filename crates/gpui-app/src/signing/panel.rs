//! The signing panel, the signature detail panel and the save-confirm
//! panel of the gpui host (spec 2026-11-23 §13, M16, M17), rendered the way
//! the print panel is: a centered modal overlay whose field chips mutate
//! the shell's signing state through shell listeners; the kit buttons ride
//! the action bridge (their click contexts cannot reach the shell entity).
//!
//! The decisions live in `super` (validation, the pre-save check); this
//! module only renders them.

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, rgb, AnyElement, ClickEvent, Context, Entity, ImageSource, ObjectFit,
    RenderImage, Window,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input as TextInput, InputState};
use rofd_sign::{CertificateId, CertificateStore, SignatureOverall, SignatureReport};

use super::{
    integrity_text, seal_match_text, signature_value_text, validity_text, verdict_text,
    SaveConfirm, SealChoice, SignDialog, SigningState, BUILTIN_SEALS,
};
use crate::print_dialog::{plain_chip, toggle_chip};
use crate::sessions::{AppAction, AppActionBridge};
use crate::shell::AppShell;

/// The action a save-confirm button fires (built on each click: an
/// `AppAction` is not `Copy`).
type ConfirmAction = fn() -> AppAction;

/// Millimetres a stepper click moves a margin.
const MARGIN_STEP: f64 = 1.0;
/// The narrowest and the widest margin the stepper reaches, and where a
/// margin that is not a number starts over.
const MARGIN_RANGE: (f64, f64) = (1.0, 200.0);
const MARGIN_DEFAULT: f64 = 10.0;

/// Edit the open dialog; an edit clears a stale error.
fn edit_dialog(
    this: &mut AppShell,
    edit: impl FnOnce(&mut SignDialog),
    cx: &mut Context<AppShell>,
) {
    if let Some(dialog) = this.signing.dialog.as_mut() {
        edit(dialog);
        dialog.error = None;
    }
    cx.notify();
}

/// One margin step inside the range.
fn stepped(margin: f64, delta: f64) -> f64 {
    let current = if margin.is_finite() && margin >= MARGIN_RANGE.0 && margin <= MARGIN_RANGE.1 {
        margin
    } else {
        MARGIN_DEFAULT
    };
    (current + delta).clamp(MARGIN_RANGE.0, MARGIN_RANGE.1)
}

/// The certificate chip's text.
fn certificate_label(store: &CertificateStore, selected: Option<&CertificateId>) -> String {
    let list = store.list();
    if list.is_empty() {
        return "证书库为空，请先导入…".to_owned();
    }
    let id = selected.or_else(|| list.first().map(|entry| &entry.id));
    list.iter()
        .find(|entry| Some(&entry.id) == id)
        .map_or_else(|| "请选择证书".to_owned(), |entry| entry.label.clone())
}

/// The next certificate of the store (round the end).
fn next_certificate(this: &AppShell) -> Option<CertificateId> {
    let list = this.signing.store.list();
    let current = this
        .signing
        .dialog
        .as_ref()
        .and_then(|dialog| dialog.certificate.clone());
    list.iter()
        .enumerate()
        .find(|(_, entry)| Some(&entry.id) == current.as_ref())
        .and_then(|(index, _)| list.get(index + 1))
        .or_else(|| list.first())
        .map(|entry| entry.id.clone())
}

/// The centered signing panel overlay (rendered while `signing.dialog` is
/// `Some`).
pub fn sign_panel_overlay(
    signing: &mut SigningState,
    password_input: &Entity<InputState>,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let (error, certificate_text, seal, auto_place, right_mm, bottom_mm, last_page_only) = {
        let dialog = signing
            .dialog
            .as_mut()
            .expect("the panel is rendered for an open dialog");
        (
            dialog.error.clone(),
            certificate_label(&signing.store, dialog.certificate.as_ref()),
            dialog.seal.clone(),
            dialog.auto_place,
            dialog.right_mm,
            dialog.bottom_mm,
            dialog.last_page_only,
        )
    };
    let importing = signing
        .importing
        .as_ref()
        .map(|import| import.file_name.clone());

    // -- 证书: cycle through the store, or import a file --
    let certificate_chip = plain_chip(
        "sg-certificate",
        certificate_text,
        |this, _e, _w, cx| {
            let next = next_certificate(this);
            edit_dialog(this, |dialog| dialog.certificate = next, cx);
        },
        cx,
    )
    .into_any_element();
    let import_chip = plain_chip(
        "sg-import",
        "导入证书…".to_string(),
        |this, _e, _w, cx| {
            // A second click withdraws the import.
            if this.signing.importing.take().is_some() {
                cx.notify();
            } else {
                AppActionBridge::push(AppAction::ImportCertificate, cx);
            }
        },
        cx,
    )
    .into_any_element();
    // The file was picked: its password goes into the field below.
    let import_row = importing.map(|file_name| {
        field_row(
            "",
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .text_size(px(13.))
                .text_color(rgb(0x555555))
                .child(format!("导入 {file_name}：口令填在下面，然后"))
                .child(plain_chip(
                    "sg-import-confirm",
                    "导入".to_string(),
                    |_this, _e, _w, cx| {
                        AppActionBridge::push(AppAction::ImportCertificateConfirm, cx);
                    },
                    cx,
                )),
        )
    });

    // -- 口令 --
    // Located by the tests, in debug builds only.
    let password_control = div()
        .debug_selector(|| "sg-password".to_owned())
        .flex()
        .flex_row()
        .items_center()
        .min_w(px(220.))
        .child(
            TextInput::new(password_input)
                .appearance(true)
                .mask_toggle(),
        );

    // -- 章图: none, the built-ins, the user's own picture --
    let mut seal_chips: Vec<AnyElement> = vec![choice_chip(
        "sg-seal-none",
        "不使用章图",
        seal == SealChoice::None,
        |this, _e, _w, cx| edit_dialog(this, |dialog| dialog.seal = SealChoice::None, cx),
        cx,
    )];
    for (index, builtin) in BUILTIN_SEALS.iter().enumerate() {
        seal_chips.push(choice_chip(
            match index {
                0 => "sg-seal-0",
                1 => "sg-seal-1",
                _ => "sg-seal-2",
            },
            builtin.name,
            seal == SealChoice::Builtin(index),
            move |this, _e, _w, cx| {
                edit_dialog(this, |dialog| dialog.seal = SealChoice::Builtin(index), cx)
            },
            cx,
        ));
    }
    let own_picture = match &seal {
        SealChoice::File { name, .. } => Some(name.clone()),
        _ => None,
    };
    seal_chips.push(choice_chip(
        "sg-seal-file",
        "选择图片…",
        own_picture.is_some(),
        |_this, _e, _w, cx| AppActionBridge::push(AppAction::PickSealImage, cx),
        cx,
    ));

    // -- 自动盖章 and its position --
    let auto_chip = toggle_chip(
        "sg-auto",
        "按位置直接盖章，不用点击落位",
        auto_place,
        |this, _e, _w, cx| edit_dialog(this, |dialog| dialog.auto_place = !dialog.auto_place, cx),
        cx,
    );
    let position_rows: Vec<AnyElement> = if auto_place {
        vec![
            field_row(
                "距右边",
                stepper(
                    "sg-right",
                    right_mm,
                    |dialog, value| dialog.right_mm = value,
                    cx,
                ),
            ),
            field_row(
                "距底部",
                stepper(
                    "sg-bottom",
                    bottom_mm,
                    |dialog, value| dialog.bottom_mm = value,
                    cx,
                ),
            ),
            field_row(
                "范围",
                toggle_chip(
                    "sg-last-page",
                    "只在最后一页",
                    last_page_only,
                    |this, _e, _w, cx| {
                        edit_dialog(
                            this,
                            |dialog| dialog.last_page_only = !dialog.last_page_only,
                            cx,
                        )
                    },
                    cx,
                ),
            ),
        ]
    } else {
        Vec::new()
    };

    // -- The buttons --
    let cancel_button = Button::new("sg-cancel").label("取消").compact().on_click(
        move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(AppAction::SignCancel, cx);
        },
    );
    let confirm_button = Button::new("sg-confirm")
        .label("确定")
        .compact()
        .primary()
        .on_click(move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(AppAction::SignConfirm, cx);
        });

    div()
        .id("sign-backdrop")
        .absolute()
        .inset_0()
        // Modal: nothing behind this layer gets the mouse, or the editor
        // under the panel takes the focus back on every press.
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.35))
        // Modal: the backdrop takes the click; the panel has a cancel.
        .on_click(|_e: &ClickEvent, _w, _cx| {})
        .child(
            div()
                .id("sign-panel")
                .flex()
                .flex_col()
                .gap_2()
                .min_w(px(480.))
                .max_w(px(600.))
                .px_4()
                .py_4()
                .bg(rgb(0xFFFFFF))
                .rounded(px(8.))
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("签章"),
                )
                .child(field_row(
                    "证书",
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(certificate_chip)
                        .child(import_chip),
                ))
                .children(import_row)
                .child(field_row("口令", password_control))
                .child(field_row(
                    "章图",
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .children(seal_chips),
                ))
                .child(field_row("自动盖章", auto_chip))
                .children(position_rows)
                .when_some(own_picture, |this, name| {
                    this.child(
                        div()
                            .pl(px(92.))
                            .text_size(px(12.))
                            .text_color(rgb(0x999999))
                            .child(format!("已选择图片：{name}")),
                    )
                })
                .when_some(error, |this, error| {
                    this.child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(0xCF1322))
                            .child(error),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(cancel_button)
                        .child(confirm_button),
                ),
        )
        .into_any_element()
}

/// `label` above a control (the panel is wide enough for long chips).
fn field_row(label: &str, control: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .when(!label.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0x8C8C8C))
                    .child(label.to_string()),
            )
        })
        .child(control)
        .into_any_element()
}

/// One choice of a set: highlighted while it is the current one.
fn choice_chip(
    id: &'static str,
    label: &str,
    on: bool,
    on_click: impl Fn(&mut AppShell, &ClickEvent, &mut Window, &mut Context<AppShell>) + 'static,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .px_2()
        .h(px(26.))
        .rounded(px(4.))
        .text_size(px(13.))
        .cursor_pointer()
        .hover(|d| d.bg(rgb(0xE2E8F0)))
        .when(on, |d| d.bg(rgb(0xD9E7F9)).text_color(rgb(0x1E6CFF)))
        .when(!on, |d| d.bg(rgb(0xF0F2F5)).text_color(rgb(0x555555)))
        .on_click(cx.listener(on_click))
        .child(label.to_string())
        .into_any_element()
}

/// `−  value mm  +` for a margin.
fn stepper(
    id_prefix: &'static str,
    value: f64,
    set: fn(&mut SignDialog, f64),
    cx: &mut Context<AppShell>,
) -> AnyElement {
    let step = |delta: f64| {
        move |this: &mut AppShell, _e: &ClickEvent, _w: &mut Window, cx: &mut Context<AppShell>| {
            edit_dialog(
                this,
                |dialog| {
                    let margin = if id_prefix == "sg-right" {
                        dialog.right_mm
                    } else {
                        dialog.bottom_mm
                    };
                    set(dialog, stepped(margin, delta));
                },
                cx,
            );
        }
    };
    let button = |suffix: &str, glyph: &'static str, delta: f64| {
        div()
            .id(format!("{id_prefix}-{suffix}"))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(26.))
            .rounded(px(4.))
            .text_size(px(13.))
            .bg(rgb(0xF0F2F5))
            .cursor_pointer()
            .hover(|d| d.bg(rgb(0xE2E8F0)))
            .on_click(cx.listener(step(delta)))
            .child(glyph)
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(button("dec", "−", -MARGIN_STEP))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .min_w(px(52.))
                .h(px(26.))
                .text_size(px(13.))
                .child(format!("{} mm", millimetres(value))),
        )
        .child(button("inc", "+", MARGIN_STEP))
        .into_any_element()
}

/// A margin for display: no trailing zeros, one decimal at most.
fn millimetres(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.1}")
    }
}

/// A seal picture decoded for the detail panel.
pub fn thumbnail(png: &[u8]) -> Option<Arc<RenderImage>> {
    let decoded = image::load_from_memory(png).ok()?.to_rgba8();
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(decoded)])))
}

/// The signature detail panel: one section per signature - the seal
/// picture (`pictures` in the reports' order; without one, "电子签名"),
/// the signer, the time, the provider, the three layers, the certificate.
pub fn signature_detail_overlay(
    reports: &[SignatureReport],
    pictures: &[Option<Arc<RenderImage>>],
) -> AnyElement {
    let close_button = Button::new("sgd-close").label("关闭").compact().on_click(
        move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(AppAction::ToggleSignatureDetail, cx);
        },
    );
    let sections: Vec<AnyElement> = reports
        .iter()
        .enumerate()
        .map(|(index, report)| detail_section(report, pictures.get(index).and_then(Option::as_ref)))
        .collect();
    div()
        .id("detail-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.35))
        .on_click(|_e: &ClickEvent, _w, _cx| {})
        .child(
            div()
                .id("sign-detail")
                .flex()
                .flex_col()
                .gap_2()
                .min_w(px(480.))
                .max_w(px(620.))
                .max_h(px(580.))
                .overflow_y_scroll()
                .px_4()
                .py_4()
                .bg(rgb(0xFFFFFF))
                .rounded(px(8.))
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(15.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("签章详情"),
                        )
                        .child(close_button),
                )
                .when(reports.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(0x999999))
                            .child("该文档没有签章。"),
                    )
                })
                .children(sections)
                .when(!reports.is_empty(), |this| {
                    this.child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(0x999999))
                            .child("未验证证书链与吊销状态"),
                    )
                }),
        )
        .into_any_element()
}

fn detail_section(report: &SignatureReport, picture: Option<&Arc<RenderImage>>) -> AnyElement {
    let verdict_color = match report.overall {
        SignatureOverall::Valid => rgb(0x389E0D),
        SignatureOverall::Invalid => rgb(0xCF1322),
        _ => rgb(0x8C8C8C),
    };
    let head: AnyElement = match picture {
        Some(render) => div()
            .flex_shrink_0()
            .child(
                gpui::img(ImageSource::Render(Arc::clone(render)))
                    .size(px(36.))
                    .object_fit(ObjectFit::Contain),
            )
            .into_any_element(),
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(56.))
            .h(px(36.))
            .flex_shrink_0()
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(0xD9D9D9))
            .text_size(px(12.))
            .text_color(rgb(0x999999))
            .child(if report.kind == rofd_dom::SignatureKind::Sign {
                "电子签名"
            } else {
                "无章图"
            })
            .into_any_element(),
    };
    let integrity = integrity_text(&report.integrity);
    let mut facts: Vec<(&str, String)> = vec![
        (
            "时间",
            report.date_time.clone().unwrap_or_else(|| "—".to_owned()),
        ),
        (
            "提供者",
            report.provider.clone().unwrap_or_else(|| "—".to_owned()),
        ),
        ("文件完整性", integrity),
        ("印章一致性", seal_match_text(report.seal_match).to_owned()),
        ("签名值", signature_value_text(&report.signature_value)),
    ];
    if let Some(certificate) = &report.certificate {
        facts.push(("颁发者", certificate.issuer.clone()));
        facts.push(("序列号", certificate.serial.clone()));
        facts.push((
            "有效期",
            validity_text(
                certificate.not_before.as_deref(),
                certificate.not_after.as_deref(),
            ),
        ));
    }
    let signer = report.certificate.as_ref().map_or_else(
        || "未知签章人".to_owned(),
        |certificate| certificate.subject.clone(),
    );
    div()
        .flex()
        .flex_col()
        .gap_1()
        .py_2()
        .border_b_1()
        .border_color(rgb(0xF0F0F0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(head)
                .child(div().flex_1().text_size(px(13.)).child(signer))
                .child(
                    div()
                        .px_2()
                        .rounded(px(10.))
                        .text_size(px(12.))
                        .bg(rgb(0xF5F5F5))
                        .text_color(verdict_color)
                        .child(verdict_text(report.overall)),
                ),
        )
        .children(facts.into_iter().map(|(label, value)| {
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .pl(px(72.))
                .text_size(px(13.))
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(0x8C8C8C))
                        .child(label.to_string()),
                )
                .child(div().flex_1().text_color(rgb(0x262626)).child(value))
        }))
        .into_any_element()
}

/// A save that would invalidate signatures, or that cannot write the
/// annotations of a package without an annotation entry: the user decides
/// (M16, M17). Never decided on the user's behalf.
pub fn save_confirm_overlay(confirm: &SaveConfirm) -> AnyElement {
    // The actions are not `Copy` and a button fires more than once: the
    // closures rebuild them.
    let (content, confirm_action, cancel_action): (String, ConfirmAction, ConfirmAction) =
        match confirm {
            SaveConfirm::Blocked { .. } => (
                "该文件没有批注入口，保存会使签章失效。".to_owned(),
                || AppAction::SaveBlockedConfirm,
                || AppAction::SaveBlockedCancel,
            ),
            SaveConfirm::Invalidated { ids, .. } => (
                format!("保存后 {} 个签章将失效。", ids.len()),
                || AppAction::SaveInvalidatedConfirm,
                || AppAction::SaveInvalidatedCancel,
            ),
        };
    let cancel_button = Button::new("save-confirm-cancel")
        .label("取消")
        .compact()
        .on_click(move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(cancel_action(), cx);
        });
    let confirm_button = Button::new("save-confirm-ok")
        .label("仍然保存")
        .compact()
        .primary()
        .on_click(move |_e: &ClickEvent, _w, cx| {
            AppActionBridge::push(confirm_action(), cx);
        });
    div()
        .id("save-confirm-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.35))
        .on_click(|_e: &ClickEvent, _w, _cx| {})
        .child(
            div()
                .id("save-confirm")
                .flex()
                .flex_col()
                .gap_3()
                .min_w(px(360.))
                .max_w(px(460.))
                .px_4()
                .py_4()
                .bg(rgb(0xFFFFFF))
                .rounded(px(8.))
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("保存会使签章失效"),
                )
                .child(div().text_size(px(13.)).child(content))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .gap_2()
                        .child(cancel_button)
                        .child(confirm_button),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stepper_moves_by_whole_millimetres_inside_the_range() {
        assert_eq!(stepped(10.0, 1.0), 11.0);
        assert_eq!(stepped(10.0, -1.0), 9.0);
        assert_eq!(stepped(15.5, 1.0), 16.5);
        // It stops at both ends.
        assert_eq!(stepped(1.0, -1.0), 1.0);
        assert_eq!(stepped(200.0, 1.0), 200.0);
        // A margin outside the range starts over from the default.
        assert_eq!(stepped(f64::NAN, 1.0), 11.0);
        assert_eq!(stepped(0.0, -1.0), 9.0);
    }

    #[test]
    fn a_margin_shows_without_a_trailing_zero() {
        assert_eq!(millimetres(10.0), "10");
        assert_eq!(millimetres(15.5), "15.5");
        assert_eq!(millimetres(40.0 / 3.0), "13.3");
    }

    #[test]
    fn the_certificate_chip_names_the_selected_entry_or_the_first_one() {
        let pfx = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sign/tests/fixtures/USER.p12"
        ))
        .expect("fixture");
        let mut store = CertificateStore::default();
        assert_eq!(certificate_label(&store, None), "证书库为空，请先导入…");
        let entry = store.import(pfx, "777777").expect("import");
        assert_eq!(certificate_label(&store, None), "Test Certificate");
        assert_eq!(
            certificate_label(&store, Some(&entry.id)),
            "Test Certificate"
        );
        assert_eq!(
            certificate_label(&store, Some(&CertificateId("gone".to_owned()))),
            "请选择证书"
        );
    }
}
