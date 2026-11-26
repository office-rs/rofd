//! The gpui host's signing line (spec 2026-11-23 §10, §11, §13): the
//! dialog state, the pre-save check, the signing itself, and the texts of
//! the status bar and the detail view. Everything here is plain Rust over
//! rofd-sign, constructed in tests without a Window; the rendering is in
//! `panel.rs`, the store's place on disk in `store_file.rs`.

use std::path::{Path, PathBuf};

use rofd_dom::Rect;
use rofd_io::Preserved;
use rofd_sign::{
    CertificateId, CertificateStore, PfxSigner, Placement, Provider, SealImage, SignError,
    SignOptions, SignRequest, SignatureOverall, SignatureReport, SignatureValue,
    SignatureValueStatus, VerifyOptions,
};

pub mod flow;
pub mod panel;
pub mod store_file;

/// The long side of a seal on the page, millimetres.
pub const SEAL_SIZE_MM: f64 = 40.0;

/// One of the seal pictures this app ships (`assets/seals`, embedded).
pub struct BuiltinSeal {
    pub name: &'static str,
    pub png: &'static [u8],
    pub width_px: u32,
    pub height_px: u32,
}

/// The built-in seal pictures: a round seal, a square seal and a bar seal
/// (the same three files the web host ships in `public/seals`).
pub const BUILTIN_SEALS: [BuiltinSeal; 3] = [
    BuiltinSeal {
        name: "圆章",
        png: include_bytes!("../../assets/seals/round.png"),
        width_px: 256,
        height_px: 256,
    },
    BuiltinSeal {
        name: "方章",
        png: include_bytes!("../../assets/seals/square.png"),
        width_px: 256,
        height_px: 256,
    },
    BuiltinSeal {
        name: "条章",
        png: include_bytes!("../../assets/seals/bar.png"),
        width_px: 384,
        height_px: 128,
    },
];

/// Which seal picture the dialog offers.
#[derive(Debug, Clone, PartialEq)]
pub enum SealChoice {
    /// None: an electronic signature.
    None,
    Builtin(usize),
    /// A picture the user picked, already read and - when it was not a PNG
    /// - converted, with its pixel size.
    File {
        name: String,
        png: Vec<u8>,
        width_px: u32,
        height_px: u32,
    },
}

impl SealChoice {
    /// The picture's pixels, and its name in the dialog.
    fn pixels(&self) -> Option<(&[u8], u32, u32)> {
        match self {
            SealChoice::None => None,
            SealChoice::Builtin(index) => BUILTIN_SEALS
                .get(*index)
                .map(|seal| (seal.png, seal.width_px, seal.height_px)),
            SealChoice::File {
                png,
                width_px,
                height_px,
                ..
            } => Some((png, *width_px, *height_px)),
        }
    }

    /// The picture rofd-sign is given, or `None` for an electronic signature.
    pub fn png(&self) -> Option<Vec<u8>> {
        self.pixels().map(|(png, _, _)| png.to_vec())
    }
}

/// Read a picture the user picked as a seal: any format the `image` crate
/// reads; what is not a PNG becomes one (what the kernel takes).
pub fn read_seal_image(path: &Path) -> Result<SealChoice, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "章图".to_owned());
    let unreadable = |reason: String| format!("章图无法读取（{name}）：{reason}");
    let bytes = std::fs::read(path).map_err(|e| unreadable(format!("读取失败：{e}")))?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| unreadable(format!("不是可识别的图片：{e}")))?;
    let rgba = decoded.to_rgba8();
    let (width_px, height_px) = rgba.dimensions();
    if width_px == 0 || height_px == 0 {
        return Err(unreadable("图片是空的".to_owned()));
    }
    let png = if image::guess_format(&bytes).is_ok_and(|f| f == image::ImageFormat::Png) {
        bytes
    } else {
        let mut out = std::io::Cursor::new(Vec::new());
        rgba.write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| unreadable(format!("无法转成 PNG：{e}")))?;
        out.into_inner()
    };
    Ok(SealChoice::File {
        name,
        png,
        width_px,
        height_px,
    })
}

/// The signing dialog: the same fields as the web host's dialog state
/// (`crates/web-app/src/sign/signDialogState.ts`), plus the password and
/// the error line.
#[derive(Debug, Clone, PartialEq)]
pub struct SignDialog {
    pub certificate: Option<CertificateId>,
    /// The certificate's password; checked before anything is signed.
    pub password: String,
    pub seal: SealChoice,
    /// 自动盖章: stamp at the given position instead of waiting for a click.
    pub auto_place: bool,
    pub right_mm: f64,
    pub bottom_mm: f64,
    /// 最后一页: the last page only; otherwise every page.
    pub last_page_only: bool,
    /// Why the last confirm was refused; the dialog stays open.
    pub error: Option<String>,
}

impl SignDialog {
    /// The defaults: the first built-in seal, placed by hand.
    pub fn new() -> Self {
        Self {
            certificate: None,
            password: String::new(),
            seal: SealChoice::Builtin(0),
            auto_place: false,
            right_mm: 10.0,
            bottom_mm: 10.0,
            last_page_only: true,
            error: None,
        }
    }

    /// What rofd-sign is asked for, before the position of a seal placed
    /// by hand is known.
    pub fn request(&self) -> SignRequest {
        let mut request = SignRequest::new();
        request.no_picture = self.seal == SealChoice::None;
        request.auto_place = self.auto_place;
        request.right_mm = self.right_mm;
        request.bottom_mm = self.bottom_mm;
        request.last_page_only = self.last_page_only;
        request
    }
}

impl Default for SignDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// A save waiting for the user's word (spec M16, M17): the package has no
/// annotation entry and declares signatures, or the save would invalidate
/// signatures. Bound to the session by its editor (indices move) and to
/// the target path (the session's own, or the one a Save-As prompt gave).
pub enum SaveConfirm {
    /// The preserving save could not write the annotations; a yes writes
    /// the whole package.
    Blocked {
        editor: gpui::Entity<rofd_gpui_view::OfdGpuiView>,
        path: Option<PathBuf>,
        close_after: bool,
    },
    /// A yes writes `bytes` (the preserving save's own output).
    Invalidated {
        editor: gpui::Entity<rofd_gpui_view::OfdGpuiView>,
        path: Option<PathBuf>,
        close_after: bool,
        bytes: Vec<u8>,
        /// The signatures that are valid now and would not be.
        ids: Vec<String>,
    },
}

/// A certificate file the user picked for import, waiting for its
/// password in the dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportingCertificate {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

/// Everything a seal placement in progress will sign with.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSign {
    /// The certificate file, as the store keeps it.
    pub pfx: Vec<u8>,
    pub password: String,
    pub request: SignRequest,
    /// The seal picture; `None` for an electronic signature.
    pub seal_png: Option<Vec<u8>>,
}

/// The shell's signing state.
#[derive(Debug, Default)]
pub struct SigningState {
    pub dialog: Option<SignDialog>,
    pub detail_open: bool,
    pub store: CertificateStore,
    pub pending: Option<PendingSign>,
    /// The session a pending seal placement belongs to.
    pub pending_session: usize,
    /// The certificate file picked for import, if any.
    pub importing: Option<ImportingCertificate>,
}

fn is_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Why the dialog cannot be confirmed yet.
pub fn validate_dialog(dialog: &SignDialog, store: &CertificateStore) -> Result<(), String> {
    let Some(certificate) = dialog.certificate.as_ref() else {
        return Err("请选择证书".to_owned());
    };
    if !store.list().iter().any(|entry| &entry.id == certificate) {
        return Err("证书已不存在，请重新导入".to_owned());
    }
    if dialog.password.is_empty() {
        return Err("请输入证书口令".to_owned());
    }
    if dialog.auto_place && !(is_positive(dialog.right_mm) && is_positive(dialog.bottom_mm)) {
        return Err("距右边和距底部必须是大于 0 的数".to_owned());
    }
    Ok(())
}

/// Check the dialog and the certificate's password (M7: the file is opened
/// once with it), and prepare what signing needs. An error keeps the
/// dialog open.
pub fn prepare_sign(
    dialog: &SignDialog,
    store: &CertificateStore,
    _now: &str,
) -> Result<PendingSign, String> {
    validate_dialog(dialog, store)?;
    let certificate = dialog.certificate.clone().expect("checked above");
    let pfx = store
        .certificate_file(&certificate)
        .ok_or_else(|| "证书已不存在，请重新导入".to_owned())?
        .to_vec();
    // The password, here and nowhere else: a wrong one is refused now.
    open_certificate(&pfx, &dialog.password)?;
    Ok(PendingSign {
        pfx,
        password: dialog.password.clone(),
        request: dialog.request(),
        seal_png: dialog.seal.png(),
    })
}

/// The seal preview's size on the page: the long side is
/// [`SEAL_SIZE_MM`], the other side follows the pixel ratio (the rule
/// rofd-sign applies when it signs). `None` without a picture.
pub fn seal_preview_size(choice: &SealChoice) -> Option<(f64, f64)> {
    let (_, width_px, height_px) = choice.pixels()?;
    let (width_px, height_px) = (f64::from(width_px), f64::from(height_px));
    let scale = SEAL_SIZE_MM / width_px.max(height_px);
    Some((width_px * scale, height_px * scale))
}

/// Everything a signature on the component needs (spec 2026-11-23 §17.11):
/// the signer, opened from the certificate file with its password, and the
/// options. `placed` is where the user put the seal (page index from 0, page
/// millimetres); without it the request's own position applies, which a seal
/// placed by hand does not have.
pub fn signing_material(
    pending: &PendingSign,
    now: &str,
    placed: Option<(u32, Rect)>,
) -> Result<(PfxSigner, SignOptions), String> {
    let placement = match placed {
        // The SDK counts pages from 1.
        Some((page, boundary)) => Some(Placement::At {
            page: page + 1,
            boundary,
        }),
        None if pending.request.signs_directly() => pending.request.placement(),
        None => return Err("签章还没有落位".to_owned()),
    };
    let options = SignOptions {
        seal: pending.seal_png.clone().map(|png| SealImage {
            png,
            size_mm: SEAL_SIZE_MM,
        }),
        placement,
        now: now.to_owned(),
        provider: Provider::default(),
    };
    let signer = open_certificate(&pending.pfx, &pending.password)?;
    Ok((signer, options))
}

/// Why the component refused to sign, in the words the dialog shows. The
/// library has one refusal of its own - the file cannot take the document's
/// annotations - and it is said here as the save says it.
pub fn sign_refusal_text(error: &SignError) -> String {
    match error {
        SignError::AnnotationsUnwritable => {
            "该文件没有批注入口，编辑中的批注无法签进文件：请先处理保存".to_owned()
        }
        other => other.to_string(),
    }
}

/// Open the certificate file for signing, with the message the dialog
/// shows. Only a wrong password is called one: a file that cannot be read,
/// or whose key is not an SM2 key, gets its own message (spec section 16).
pub(crate) fn open_certificate(pfx: &[u8], password: &str) -> Result<PfxSigner, String> {
    PfxSigner::open(pfx, password).map_err(|error| match error {
        SignError::Password { reason } => format!("证书口令错误：{reason}"),
        other => format!("证书文件无法使用：{other}"),
    })
}

/// What else a successful save has to tell the user (spec M14, M20):
/// nothing in the normal case. An exhausted ID reserve is written all the
/// same, and so is a fallback to the full write; neither may pass silently.
pub fn save_notes(preserved: &Preserved) -> Vec<String> {
    let mut notes = Vec::new();
    if preserved.reserve_exhausted {
        notes.push(
            "签章时预留的批注编号已用完：新批注的编号超出了文件记录的最大编号，其他软件之后新建的对象可能与之重复"
                .to_owned(),
        );
    }
    if preserved.full_write_fallback {
        notes.push("这份文件无法只改写批注，已整份重新保存（文件没有签章，不影响验签）".to_owned());
    }
    notes
}

/// What a save would do to the document's signatures.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveCheck {
    /// The package declares signatures and has no annotation entry: the
    /// preserving save could not write the annotations (M16).
    Blocked,
    /// Nothing is invalidated; the reports of the bytes to be written.
    Clear(Vec<SignatureReport>),
    /// These signatures are valid now and would not be after the save
    /// (M17).
    Invalidated(Vec<String>),
}

/// The pre-save check (spec M16, M17) on the bytes a save would write,
/// against the reports of the file as it is.
pub fn pre_save_check(
    before: &[SignatureReport],
    preserved: &Preserved,
) -> Result<SaveCheck, String> {
    if preserved.blocked_by_signature {
        return Ok(SaveCheck::Blocked);
    }
    let after = rofd_sign::verify(&preserved.bytes, &VerifyOptions::default());
    let invalidated = rofd_sign::newly_invalidated(before, &after);
    if invalidated.is_empty() {
        Ok(SaveCheck::Clear(after))
    } else {
        Ok(SaveCheck::Invalidated(invalidated))
    }
}

/// The signing time as rofd-sign takes it (`YYYY-MM-DD HH:mm:ss`, local
/// time). The HOST reads the clock; the library never does.
pub fn host_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---- the texts of the status bar and the detail view ----

/// The status bar's one line about the signatures; `None` (the entry is
/// not shown) when the document has none. A failure outranks everything
/// else; "仅未验证" counts the signatures that could not be confirmed.
pub fn summary_text(reports: &[SignatureReport]) -> Option<String> {
    if reports.is_empty() {
        return None;
    }
    let count = |overall: SignatureOverall| {
        reports
            .iter()
            .filter(|report| report.overall == overall)
            .count()
    };
    let invalid = count(SignatureOverall::Invalid);
    if invalid > 0 {
        return Some(format!("签章：有失效（{invalid}/{}）", reports.len()));
    }
    let valid = count(SignatureOverall::Valid);
    if valid == reports.len() {
        return Some(format!("签章：全部有效（{valid}）"));
    }
    Some(format!("签章：仅未验证（{}）", reports.len() - valid))
}

pub fn verdict_text(overall: SignatureOverall) -> &'static str {
    match overall {
        SignatureOverall::Valid => "有效",
        SignatureOverall::Invalid => "失效",
        SignatureOverall::Unverified => "未验证",
        SignatureOverall::Unparseable => "无法解析",
        SignatureOverall::Unsupported => "不支持",
    }
}

/// The integrity line of the detail panel: what changed, or that the
/// signature protects no file at all.
pub fn integrity_text(integrity: &rofd_sign::IntegrityResult) -> String {
    if integrity.ok {
        return "有效".to_owned();
    }
    if integrity.protected_files == 0 {
        return "失效（该签名未保护任何文件）".to_owned();
    }
    let files: Vec<&str> = integrity
        .failures
        .iter()
        .map(|failure| failure.file_ref.as_str())
        .collect();
    format!("失效（{}）", files.join("、"))
}

pub fn seal_match_text(matches: Option<bool>) -> &'static str {
    match matches {
        None => "—",
        Some(true) => "一致",
        Some(false) => "不一致",
    }
}

pub fn signature_value_text(value: &SignatureValue) -> String {
    let text = match value.status {
        SignatureValueStatus::Ok => "有效",
        SignatureValueStatus::Fail => "失败",
        SignatureValueStatus::Unverifiable => "无法验证",
    };
    match value.reason.as_deref() {
        None => text.to_owned(),
        Some("") => text.to_owned(),
        Some(reason) => format!("{text}（{reason}）"),
    }
}

/// A certificate's time as a day (`YYYY-MM-DD`). Only a time that opens with
/// eight digits is formatted: a report carries whatever the certificate says
/// there, and anything else is shown as it is.
fn formatted_day(time: &str) -> String {
    let digits: String = time.chars().take(8).collect();
    if digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..8])
    } else {
        time.to_owned()
    }
}

/// The days a certificate is good for.
pub fn validity_text(not_before: Option<&str>, not_after: Option<&str>) -> String {
    match (not_before, not_after) {
        (None, None) => "—".to_owned(),
        (Some(from), None) => format!("{} 起", formatted_day(from)),
        (None, Some(until)) => format!("至 {}", formatted_day(until)),
        (Some(from), Some(until)) => {
            format!("{} 至 {}", formatted_day(from), formatted_day(until))
        }
    }
}

#[cfg(test)]
mod tests;
