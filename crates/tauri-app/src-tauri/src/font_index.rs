//! 本机字体索引（draft 2026-10-18 §3.1）：启动时扫描系统字体目录一次，
//! 用 mmap + ttf-parser 提取每个 face 的 family 名（name ID 1 与 16，**所有
//! 语言记录**——"SimSun" 与 "宋体" 是两条独立条目，这正是免 SDK 侧别名处理
//! 的根基）。纯逻辑模块，可单测（`scan_dirs` 接受任意目录序列）。
//!
//! 优先级语义：目录按升序优先级传入（系统目录 → 每用户目录），**后扫胜出**
//! （HashMap 直接覆盖），对齐 Windows 每用户字体覆盖系统字体的行为。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 面向前端的清单条目（serde rename 对齐 JS 侧 `SystemFontEntry`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SystemFontEntry {
    pub family: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
}

/// 扫描产物：族名 → 文件名（小写）；文件名（小写）→ 绝对路径。
#[derive(Debug, Default, Clone)]
pub struct FontIndex {
    families: HashMap<String, String>,
    files: HashMap<String, PathBuf>,
}

impl FontIndex {
    /// 排序后的 `{family, fileName}[]`（同族多名各自成条——与切片别名并列
    /// 行为一致，picker/补载遍按名匹配）。
    pub fn entries(&self) -> Vec<SystemFontEntry> {
        let mut out: Vec<SystemFontEntry> = self
            .families
            .iter()
            .map(|(family, file_name)| SystemFontEntry {
                family: family.clone(),
                file_name: file_name.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.family.cmp(&b.family));
        out
    }

    /// 读命令的校验面：`name` 必须是扫描已见过的文件名（小写比较），且不含
    /// 任何路径成分。返回绝对路径。攻击面 = "扫描已经见过的文件"——白名单
    /// 退役后安全模型不升（读一个 Windows 自己已加载的字体文件）。
    pub fn resolve_font_file(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains(':')
            || name.contains("..")
        {
            return None;
        }
        self.files.get(&name.to_lowercase()).cloned()
    }
}

/// 扫描系统字体目录（app setup 一次，webview 就绪前）：
/// `%WINDIR%\Fonts` + `%LOCALAPPDATA%\Microsoft\Windows\Fonts`，升序优先级。
pub fn scan_font_index() -> FontIndex {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        dirs.push(Path::new(&windir).join("Fonts"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(
            Path::new(&local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    scan_dirs(&dirs)
}

/// 纯函数扫描入口（测试用）：逐目录、逐文件；读不了/解不了的文件跳过
/// （扫描永不失败）。同族后写胜出。
pub fn scan_dirs(dirs: &[PathBuf]) -> FontIndex {
    let mut index = FontIndex::default();
    for dir in dirs {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            continue; // 目录不存在（如无每用户字体目录）→ 跳过
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            index_file(&mut index, &path);
        }
    }
    index
}

/// 索引单个文件：扩展名过滤（.ttf/.ttc/.otf/.otc 大小写不敏感）→ mmap →
/// 每 face 提取 family 名。任何失败静默跳过（best-effort）。
fn index_file(index: &mut FontIndex, path: &Path) {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return;
    };
    let ext = ext.to_lowercase();
    if !matches!(ext.as_str(), "ttf" | "ttc" | "otf" | "otc") {
        return;
    }
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let file_name = file_name.to_lowercase();

    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    // mmap 而非 fs::read：name 表只有几 KB，只换入 ttf-parser 触到的页；
    // 整读会把几百 MB 字体目录全 slurp。字体目录文件由系统管理，映射稳定。
    let Ok(mmap) = (unsafe { memmap2::Mmap::map(&file) }) else {
        return;
    };
    let data: &[u8] = &mmap;

    let n_faces = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    for i in 0..n_faces {
        let Ok(face) = ttf_parser::Face::parse(data, i) else {
            continue;
        };
        let names = face.names();
        for name in names {
            // name ID 1（family）+ 16（preferred family），所有语言记录。
            // ttf-parser 0.25：`name_id` 是裸字段而非方法。
            let id = name.name_id;
            if id != 1 && id != 16 {
                continue;
            }
            if let Some(family) = name.to_string() {
                if !family.is_empty() {
                    index.families.insert(family, file_name.clone());
                }
            }
        }
    }
    // 文件本身进 files 映射（即使上面 face 解析全失败也无害：resolve 得到
    // 路径、读命令读到字节后内核注册失败 → 该族 settle，不重试）。
    index.files.insert(file_name, path.to_path_buf());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OFL 夹具（出处见 testdata/OFL.txt）：rofd render 测试同源拷贝。
    const TESTFONT: &[u8] = include_bytes!("../testdata/TestFont.ttf");
    const TESTFONT_CJK: &[u8] = include_bytes!("../testdata/TestFontCjk.otf");

    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rofd-font-index-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extracts_family_names_including_both_faces() {
        let dir = fixture_dir("extract");
        std::fs::write(dir.join("TestFont.ttf"), TESTFONT).unwrap();
        std::fs::write(dir.join("TestFontCjk.otf"), TESTFONT_CJK).unwrap();
        let index = scan_dirs(std::slice::from_ref(&dir));
        let families: Vec<String> = index.entries().into_iter().map(|e| e.family).collect();
        // TestFontCjk 的 name 表族名是 "Noto Sans CJK SC"（Task 4 实证）。
        assert!(
            families.iter().any(|f| f == "Noto Sans CJK SC"),
            "CJK 夹具族名应被提取：{families:?}"
        );
        assert!(
            families.len() >= 2,
            "两个夹具至少各产一个族名：{families:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn later_directory_wins_on_same_file_name() {
        // Windows 每用户覆盖语义：用户目录后扫，同族/同名后写胜出。
        let low = fixture_dir("prio-low");
        let high = fixture_dir("prio-high");
        std::fs::write(low.join("dup.ttf"), TESTFONT).unwrap();
        std::fs::write(high.join("dup.ttf"), TESTFONT_CJK).unwrap();
        let index = scan_dirs(&[low.clone(), high.clone()]);
        let resolved = index.resolve_font_file("dup.ttf").unwrap();
        assert!(
            resolved.starts_with(&high),
            "后扫目录的文件应胜出：{resolved:?}"
        );
        let _ = std::fs::remove_dir_all(&low);
        let _ = std::fs::remove_dir_all(&high);
    }

    #[test]
    fn resolve_rejects_paths_and_unknown_names() {
        let dir = fixture_dir("resolve");
        std::fs::write(dir.join("TestFont.ttf"), TESTFONT).unwrap();
        let index = scan_dirs(std::slice::from_ref(&dir));
        assert!(index.resolve_font_file("TestFont.ttf").is_some());
        assert!(
            index.resolve_font_file("TESTFONT.TTF").is_some(),
            "大小写不敏感"
        );
        assert!(
            index.resolve_font_file("../TestFont.ttf").is_none(),
            "相对路径拒绝"
        );
        assert!(
            index
                .resolve_font_file("C:/Windows/Fonts/simsun.ttc")
                .is_none(),
            "绝对路径拒绝"
        );
        assert!(
            index.resolve_font_file("nope.ttf").is_none(),
            "未扫描名拒绝"
        );
        assert!(index.resolve_font_file("").is_none(), "空名拒绝");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_never_fails_on_garbage_and_missing_dirs() {
        let dir = fixture_dir("garbage");
        std::fs::write(dir.join("broken.ttf"), b"not a font at all").unwrap();
        std::fs::write(dir.join("ignored.txt"), b"skip me").unwrap();
        let index = scan_dirs(&[dir.clone(), PathBuf::from("Z:/definitely/absent")]);
        assert!(
            index.entries().is_empty(),
            "垃圾字节与非字体扩展名都跳过：{:?}",
            index.entries()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
