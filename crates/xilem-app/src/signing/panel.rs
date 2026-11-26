//! The signing panel, the signature detail panel, the save-confirm panel
//! and the status bar's entry of the xilem host (spec 2026-11-23 §13,
//! M16, M17): xilem views over `AppState`'s signing state, laid out as
//! overlays in the window's zstack the way the print Backstage is. The
//! decisions live in `super` and `flow`; this module only renders them.

use std::sync::Arc;

use rofd_sign::{CertificateId, CertificateStore, SignatureReport};
use xilem::masonry::imaging::kurbo::Affine;
use xilem::masonry::imaging::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use xilem::masonry::imaging::{record::Scene, Painter};
use xilem::masonry::layout::Length;
use xilem::masonry::peniko::Color;
use xilem::masonry::properties::Padding;
use xilem::style::Style as _;
use xilem::view::{
    canvas, checkbox, flex_col, flex_row, label, sized_box, text_button, text_input, FlexExt,
};
use xilem::WidgetView;

use super::{
    flow, integrity_text, merge_masked, seal_match_text, signature_value_text, summary_text,
    validity_text, verdict_text, SaveConfirm, SealChoice, SignDialog, BUILTIN_SEALS,
    PASSWORD_BULLET,
};
use crate::AppState;

const GAP: Length = Length::const_px(6.0);
const CHIP_PAD: Padding = Padding::from_vh(Length::const_px(4.0), Length::const_px(10.0));
const CHIP: Color = Color::from_rgb8(0xF0, 0xF2, 0xF5);
const CHIP_ON: Color = Color::from_rgb8(0xD9, 0xE7, 0xF9);
const BORDER: Color = Color::from_rgb8(0xD9, 0xD9, 0xD9);

/// What a chip does when clicked. Boxed, so that every chip - whatever
/// its callback - is one view type and chips can share a `Vec` or a
/// tuple.
type Click = Arc<dyn Fn(&mut AppState) + Send + Sync + 'static>;

/// A borderless button.
fn button(text: String, on_click: Click) -> impl WidgetView<AppState> + use<> {
    text_button(text, move |app: &mut AppState| on_click(app))
        .padding(CHIP_PAD)
        .border_width(Length::ZERO)
        .corner_radius(Length::const_px(4.0))
}

/// One choice of a set: highlighted while it is the current one.
fn choice(text: String, on: bool, on_click: Click) -> impl WidgetView<AppState> + use<> {
    sized_box(button(text, on_click)).background_color(if on { CHIP_ON } else { CHIP })
}

/// Edit the open dialog; an edit clears a stale error.
fn edit(app: &mut AppState, edit: impl FnOnce(&mut SignDialog)) {
    if let Some(dialog) = app.signing.dialog.as_mut() {
        edit(dialog);
        dialog.error = None;
    }
}

/// The certificate chip's text.
pub(crate) fn certificate_label(
    store: &CertificateStore,
    selected: Option<&CertificateId>,
) -> String {
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
pub(crate) fn next_certificate(
    store: &CertificateStore,
    selected: Option<&CertificateId>,
) -> Option<CertificateId> {
    let list = store.list();
    list.iter()
        .enumerate()
        .find(|(_, entry)| Some(&entry.id) == selected)
        .and_then(|(index, _)| list.get(index + 1))
        .or_else(|| list.first())
        .map(|entry| entry.id.clone())
}

/// One margin step inside the range; a margin that is not a number, or
/// left the range, starts over from the default.
pub(crate) fn stepped(margin: f64, delta: f64) -> f64 {
    let current = if margin.is_finite() && (1.0..=200.0).contains(&margin) {
        margin
    } else {
        10.0
    };
    (current + delta).clamp(1.0, 200.0)
}

/// A margin for display: no trailing zeros, one decimal at most.
pub(crate) fn millimetres(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.1}")
    }
}

/// The seal's pixels for the detail panel.
pub(crate) fn thumbnail(png: &[u8]) -> Option<Arc<ImageData>> {
    let rgba = image::load_from_memory(png).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    (width > 0 && height > 0).then(|| {
        Arc::new(ImageData {
            data: Blob::new(Arc::new(rgba.into_raw())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        })
    })
}

/// The thumbnail, fit into a 36 px square (nothing is drawn without a
/// picture).
fn thumbnail_view(picture: Option<Arc<ImageData>>) -> impl WidgetView<AppState> + use<> {
    canvas(move |_app: &mut AppState, _ctx, scene: &mut Scene, size| {
        let Some(image) = picture.as_ref() else {
            return;
        };
        let scale =
            (size.width / f64::from(image.width)).min(size.height / f64::from(image.height));
        Painter::new(scene).draw_image(image.as_ref(), Affine::scale(scale));
    })
    .width(Length::const_px(36.0))
    .height(Length::const_px(36.0))
}

/// A centered white panel on a dimmed backdrop that fills the window.
fn overlay(content: impl WidgetView<AppState>) -> impl WidgetView<AppState> {
    let panel = sized_box(content)
        .background_color(Color::WHITE)
        .border_color(BORDER)
        .border_width(Length::const_px(1.0))
        .corner_radius(Length::const_px(8.0))
        .padding(Padding::from_vh(
            Length::const_px(16.0),
            Length::const_px(16.0),
        ));
    let filler = || sized_box(label("")).flex(1.0);
    let centered = flex_col((filler(), flex_row((filler(), panel, filler())), filler()));
    sized_box(centered).background_color(Color::from_rgba8(0, 0, 0, 0x59))
}

/// A `−` / `+` pair for a margin.
fn stepper(
    read: fn(&SignDialog) -> f64,
    set: fn(&mut SignDialog, f64),
) -> (
    impl WidgetView<AppState> + use<>,
    impl WidgetView<AppState> + use<>,
) {
    let dec: Click = Arc::new(move |app: &mut AppState| {
        edit(app, |dialog| set(dialog, stepped(read(dialog), -1.0)));
    });
    let inc: Click = Arc::new(move |app: &mut AppState| {
        edit(app, |dialog| set(dialog, stepped(read(dialog), 1.0)));
    });
    (
        choice("−".to_owned(), false, dec),
        choice("+".to_owned(), false, inc),
    )
}

/// The signing panel overlay (rendered while `app.signing.dialog` is
/// `Some`).
pub(crate) fn sign_panel(app: &mut AppState) -> impl WidgetView<AppState> + use<> {
    let dialog = app
        .signing
        .dialog
        .clone()
        .expect("the panel is rendered for an open dialog");
    let certificate_text = certificate_label(&app.signing.store, dialog.certificate.as_ref());
    let importing = app
        .signing
        .importing
        .as_ref()
        .map(|import| import.file_name.clone());
    // One bullet per character of the password.
    let bullets = PASSWORD_BULLET
        .to_string()
        .repeat(dialog.password.chars().count());
    let own_picture_name = match &dialog.seal {
        SealChoice::File { name, .. } => Some(name.clone()),
        _ => None,
    };
    let own_picture = own_picture_name.is_some();

    // -- 证书: cycle through the store, or import a file --
    let cycle: Click = Arc::new(|app: &mut AppState| {
        let selected = app
            .signing
            .dialog
            .as_ref()
            .and_then(|dialog| dialog.certificate.clone());
        let next = next_certificate(&app.signing.store, selected.as_ref());
        edit(app, |dialog| dialog.certificate = next);
    });
    let import: Click = Arc::new(|app: &mut AppState| {
        // A second click withdraws the import.
        if app.signing.importing.take().is_some() {
            return;
        }
        flow::import_certificate(app);
    });
    let certificate_row = flex_row((
        label("证书"),
        choice(certificate_text, true, cycle),
        choice("导入证书…".to_owned(), importing.is_some(), import),
    ))
    .gap(GAP);
    // The file was picked: its password goes into the field below.
    let import_row = importing.map(|file_name| {
        flex_row((
            label(""),
            label(format!("导入 {file_name}：口令填在下面，然后")),
            choice(
                "导入".to_owned(),
                true,
                Arc::new(flow::confirm_import_certificate),
            ),
        ))
        .gap(GAP)
    });

    // -- 口令: the field shows bullets, the password is rebuilt from them --
    let clear: Click = Arc::new(|app: &mut AppState| edit(app, |dialog| dialog.password.clear()));
    let password_row = flex_row((
        label("口令"),
        text_input(bullets, |app: &mut AppState, shown: String| {
            edit(app, |dialog| {
                dialog.password = merge_masked(&dialog.password, &shown)
            });
        })
        .placeholder("签名证书的口令"),
        choice("清空".to_owned(), false, clear),
    ))
    .gap(GAP);

    // -- 章图: none, the built-ins, the user's own picture --
    let none: Click =
        Arc::new(|app: &mut AppState| edit(app, |dialog| dialog.seal = SealChoice::None));
    let mut seals = vec![choice(
        "不使用章图".to_owned(),
        dialog.seal == SealChoice::None,
        none,
    )];
    for (index, builtin) in BUILTIN_SEALS.iter().enumerate() {
        let pick: Click = Arc::new(move |app: &mut AppState| {
            edit(app, |dialog| dialog.seal = SealChoice::Builtin(index));
        });
        seals.push(choice(
            builtin.name.to_owned(),
            dialog.seal == SealChoice::Builtin(index),
            pick,
        ));
    }
    seals.push(choice(
        own_picture_name.map_or_else(|| "选择图片…".to_owned(), |name| format!("图片：{name}")),
        own_picture,
        Arc::new(flow::pick_seal_image),
    ));
    let seal_row = flex_row((label("章图"), flex_row(seals).gap(GAP))).gap(GAP);

    // -- 自动盖章 and its position --
    let auto_place = dialog.auto_place;
    let auto_row = checkbox(
        "自动盖章：按位置直接盖章，不用点击落位",
        auto_place,
        |app: &mut AppState, on| edit(app, |dialog| dialog.auto_place = on),
    );
    let (right_dec, right_inc) = stepper(
        |dialog| dialog.right_mm,
        |dialog, value| dialog.right_mm = value,
    );
    let (bottom_dec, bottom_inc) = stepper(
        |dialog| dialog.bottom_mm,
        |dialog, value| dialog.bottom_mm = value,
    );
    let position_row = auto_place.then(|| {
        flex_row((
            label("距右边"),
            right_dec,
            label(format!("{} mm", millimetres(dialog.right_mm))),
            right_inc,
            label("距底部"),
            bottom_dec,
            label(format!("{} mm", millimetres(dialog.bottom_mm))),
            bottom_inc,
            checkbox(
                "只在最后一页",
                dialog.last_page_only,
                |app: &mut AppState, on| edit(app, |dialog| dialog.last_page_only = on),
            ),
        ))
        .gap(GAP)
    });

    let error_row = dialog.error.map(|error| label(format!("错误：{error}")));
    let cancel: Click = Arc::new(flow::cancel_sign_dialog);
    let confirm: Click = Arc::new(flow::confirm_sign_dialog);

    overlay(
        flex_col((
            label("签章"),
            certificate_row,
            import_row,
            password_row,
            seal_row,
            auto_row,
            position_row,
            error_row,
            flex_row((
                choice("取消".to_owned(), false, cancel),
                choice("确定".to_owned(), true, confirm),
            ))
            .gap(GAP),
        ))
        .gap(GAP),
    )
}

/// The signature detail panel (rendered while `app.signing.detail_open`):
/// one section per signature. `pictures` are the document's seal pictures
/// in the reports' order.
pub(crate) fn signature_detail(
    app: &mut AppState,
    pictures: &[Option<Arc<Vec<u8>>>],
) -> impl WidgetView<AppState> + use<> {
    let reports = app.signatures.clone();
    let sections: Vec<_> = reports
        .iter()
        .enumerate()
        .map(|(index, report)| {
            let picture = pictures
                .get(index)
                .and_then(|picture| picture.as_deref())
                .and_then(|bytes| thumbnail(bytes));
            section(report, picture)
        })
        .collect();
    let empty = reports.is_empty().then(|| label("该文档没有签章。"));
    let foot = (!reports.is_empty()).then(|| label("未验证证书链与吊销状态"));
    let close: Click = Arc::new(|app: &mut AppState| app.signing.detail_open = false);
    let title = flex_row((
        label("签章详情").flex(1.0),
        choice("关闭".to_owned(), false, close),
    ))
    .gap(GAP);
    overlay(flex_col((title, flex_col(sections).gap(GAP), empty, foot)).gap(GAP))
}

/// One signature: the seal picture ("电子签名" or "无章图" without one),
/// the signer, the verdict, the three layers, the certificate.
fn section(
    report: &SignatureReport,
    picture: Option<Arc<ImageData>>,
) -> impl WidgetView<AppState> + use<> {
    let signer = match (&report.certificate, picture.is_some()) {
        (Some(certificate), true) => certificate.subject.clone(),
        (Some(certificate), false) if report.kind == rofd_dom::SignatureKind::Sign => {
            format!("电子签名：{}", certificate.subject)
        }
        (Some(certificate), false) => format!("无章图：{}", certificate.subject),
        (None, _) => "未知签章人".to_owned(),
    };
    let verdict = verdict_text(report.overall).to_owned();
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
    let fact_rows: Vec<_> = facts
        .into_iter()
        .map(|(name, value)| flex_row((label(name), label(value))).gap(GAP))
        .collect();
    flex_col((
        flex_row((
            sized_box(thumbnail_view(picture)).border_color(BORDER),
            label(signer).flex(1.0),
            label(verdict),
        ))
        .gap(GAP),
        flex_col(fact_rows).gap(Length::const_px(2.0)),
    ))
    .gap(GAP)
}

/// A save that would invalidate signatures, or that cannot write the
/// annotations of a package without an annotation entry: the user decides
/// (M16, M17). Never decided on the user's behalf.
pub(crate) fn save_confirm_panel(confirm: &SaveConfirm) -> impl WidgetView<AppState> + use<> {
    let content = match confirm {
        SaveConfirm::Blocked { .. } => "该文件没有批注入口，保存会使签章失效。".to_owned(),
        SaveConfirm::Invalidated { ids, .. } => {
            format!("保存后 {} 个签章将失效。", ids.len())
        }
    };
    let no: Click = Arc::new(|app: &mut AppState| flow::save_confirm_answer(app, false));
    let yes: Click = Arc::new(|app: &mut AppState| flow::save_confirm_answer(app, true));
    overlay(
        flex_col((
            label("保存会使签章失效"),
            label(content),
            flex_row((
                choice("取消".to_owned(), false, no),
                choice("仍然保存".to_owned(), true, yes),
            ))
            .gap(GAP),
        ))
        .gap(GAP),
    )
}

/// The status bar: the signatures of the file (the entry opens the detail
/// panel) and what the signing line last did. `None` when there is
/// nothing to say - a document without signatures starts as before.
pub(crate) fn status_bar(app: &mut AppState) -> Option<impl WidgetView<AppState> + use<>> {
    let summary = summary_text(&app.signatures);
    let status = app.status.clone();
    if summary.is_none() && status.is_empty() {
        return None;
    }
    let entry = summary.map(|text| {
        let toggle: Click = Arc::new(|app: &mut AppState| {
            app.signing.detail_open = !app.signing.detail_open;
        });
        choice(text, app.signing.detail_open, toggle)
    });
    Some(
        sized_box(flex_row((entry, label(status).flex(1.0))).gap(GAP))
            .background_color(Color::from_rgb8(0xFA, 0xFA, 0xFA))
            .border_color(BORDER)
            .border_width(Length::const_px(1.0))
            .padding(Padding::from_vh(
                Length::const_px(2.0),
                Length::const_px(6.0),
            )),
    )
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
        // The only certificate is its own successor.
        assert_eq!(next_certificate(&store, Some(&entry.id)), Some(entry.id));
    }

    #[test]
    fn a_seal_picture_decodes_for_the_detail_panel() {
        assert!(thumbnail(BUILTIN_SEALS[0].png).is_some());
        assert!(thumbnail(b"not a picture").is_none());
    }
}
