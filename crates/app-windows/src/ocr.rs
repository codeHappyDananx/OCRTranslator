use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, CString},
    fs,
    io::{Cursor, Read, Write},
    os::raw::{c_char, c_void},
    path::{Path, PathBuf},
};
use windows::{
    core::HSTRING,
    Globalization::Language,
    Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat},
    Media::Ocr::OcrEngine,
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrLanguageInfo {
    pub tag: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrEngineStatus {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneOcrPackageInfo {
    pub name: String,
    pub size_bytes: Option<u64>,
}

pub fn detect_ocr_engines() -> Result<Vec<OcrEngineStatus>> {
    let windows_languages = available_windows_ocr_languages().unwrap_or_default();
    let snipping = find_snippingtool_oneocr();
    Ok(vec![
        OcrEngineStatus {
            id: "windows".to_string(),
            name: "Windows OCR".to_string(),
            available: !windows_languages.is_empty(),
            detail: if windows_languages.is_empty() {
                "未发现可用 Windows OCR 语言".to_string()
            } else {
                format!(
                    "{}",
                    windows_languages
                        .iter()
                        .map(|lang| format!("{} {}", lang.tag, lang.display_name))
                        .collect::<Vec<_>>()
                        .join(" / ")
                )
            },
        },
        OcrEngineStatus {
            id: "snippingtool".to_string(),
            name: "SnippingTool OneOCR".to_string(),
            available: snipping.is_some(),
            detail: snipping
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| {
                    "未找到 oneocr.dll / oneocr.onemodel / onnxruntime.dll".to_string()
                }),
        },
    ])
}

pub async fn install_snippingtool_oneocr_runtime() -> Result<PathBuf> {
    let target = oneocr_cache_dir()?;
    if check_oneocr_dir(&target) {
        return Ok(target);
    }

    fs::create_dir_all(&target)
        .with_context(|| format!("创建 OneOCR 缓存目录失败：{}", target.display()))?;
    let client = oneocr_download_client()?;
    let html = fetch_screensketch_package_list(&client).await?;
    let package = select_oneocr_screensketch_bundle(&html)
        .ok_or_else(|| anyhow!("没有找到 Microsoft.ScreenSketch msixbundle 下载项"))?;
    let bytes = client
        .get(&package.url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    extract_oneocr_from_bundle(bytes.as_ref(), &target)
        .with_context(|| format!("解包 OneOCR 失败：{}", package.name))?;
    if !check_oneocr_dir(&target) {
        bail!("解包完成但未找到 oneocr.dll / oneocr.onemodel / onnxruntime.dll");
    }
    Ok(target)
}

pub async fn preview_snippingtool_oneocr_package() -> Result<Option<OneOcrPackageInfo>> {
    if check_oneocr_dir(&oneocr_cache_dir()?) {
        return Ok(None);
    }
    let client = oneocr_download_client()?;
    let html = fetch_screensketch_package_list(&client).await?;
    Ok(
        select_oneocr_screensketch_bundle(&html).map(|package| OneOcrPackageInfo {
            name: package.name,
            size_bytes: package.size_bytes,
        }),
    )
}

#[derive(Debug, Clone)]
struct StorePackage {
    version: Vec<u32>,
    url: String,
    name: String,
    size_bytes: Option<u64>,
}

fn oneocr_download_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("OCR-Translator/0.1")
        .build()?)
}

async fn fetch_screensketch_package_list(client: &reqwest::Client) -> Result<String> {
    Ok(client
        .post("https://store.rg-adguard.net/api/GetFiles")
        .header("origin", "https://store.rg-adguard.net")
        .header("referer", "https://store.rg-adguard.net/")
        .form(&[
            ("type", "PackageFamilyName"),
            ("url", "Microsoft.ScreenSketch_8wekyb3d8bbwe"),
        ])
        .send()
        .await?
        .text()
        .await?)
}

fn select_oneocr_screensketch_bundle(html: &str) -> Option<StorePackage> {
    let row_re = Regex::new(r#"(?s)<tr.*?</tr>"#).ok()?;
    let link_re = Regex::new(
        r#"<a href="([^"]+)".*?>(Microsoft\.ScreenSketch_([0-9.]+)_neutral_~_8wekyb3d8bbwe\.msixbundle)</a>"#,
    )
    .ok()?;
    let mut packages = Vec::new();
    for row in row_re.find_iter(html) {
        let row = row.as_str();
        let Some(cap) = link_re.captures(row) else {
            continue;
        };
        let version = cap[3]
            .split('.')
            .filter_map(|part| part.parse::<u32>().ok())
            .collect::<Vec<_>>();
        if version.is_empty() {
            continue;
        }
        packages.push(StorePackage {
            version,
            url: html_unescape(&cap[1]),
            name: cap[2].to_string(),
            size_bytes: parse_package_size(row),
        });
    }
    let mut oneocr_candidates = packages
        .iter()
        .filter(|package| package.version.as_slice() >= [2022, 2511].as_slice())
        .cloned()
        .collect::<Vec<_>>();
    oneocr_candidates.sort_by(|a, b| {
        a.size_bytes
            .unwrap_or(u64::MAX)
            .cmp(&b.size_bytes.unwrap_or(u64::MAX))
            .then_with(|| b.version.cmp(&a.version))
    });
    oneocr_candidates.into_iter().next().or_else(|| {
        packages.sort_by(|a, b| a.version.cmp(&b.version));
        packages.pop()
    })
}

fn parse_package_size(row: &str) -> Option<u64> {
    let text = Regex::new(r#"<.*?>"#)
        .ok()?
        .replace_all(row, " ")
        .to_string();
    let re = Regex::new(r#"([0-9]+(?:\.[0-9]+)?)\s*(KB|MB|GB)"#).ok()?;
    let cap = re.captures(&text)?;
    let value = cap[1].parse::<f64>().ok()?;
    let multiplier = match &cap[2] {
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

fn extract_oneocr_from_bundle(bundle_bytes: &[u8], target: &Path) -> Result<()> {
    let cursor = Cursor::new(bundle_bytes);
    let mut bundle = zip::ZipArchive::new(cursor).context("读取 msixbundle 失败")?;
    let mut x64_msix = Vec::new();
    for i in 0..bundle.len() {
        let mut file = bundle.by_index(i)?;
        let name = file.name().to_string();
        if name.starts_with("SnippingTool") && name.ends_with("_x64.msix") {
            file.read_to_end(&mut x64_msix)?;
            break;
        }
    }
    if x64_msix.is_empty() {
        bail!("msixbundle 中没有找到 SnippingTool x64 msix");
    }

    let cursor = Cursor::new(x64_msix);
    let mut msix = zip::ZipArchive::new(cursor).context("读取 SnippingTool msix 失败")?;
    for i in 0..msix.len() {
        let mut file = msix.by_index(i)?;
        let name = file.name().replace('\\', "/");
        if !name.starts_with("SnippingTool/") || name.ends_with('/') {
            continue;
        }
        let relative = name.trim_start_matches("SnippingTool/");
        if relative.is_empty() {
            continue;
        }
        let out_path = target.join(relative);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut out)?;
        out.flush()?;
    }
    Ok(())
}

fn oneocr_cache_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("无法定位 LOCALAPPDATA"))?;
    Ok(base.join("OCR-Translator").join("SnippingTool"))
}

fn check_oneocr_dir(dir: &Path) -> bool {
    dir.join("oneocr.dll").is_file()
        && dir.join("oneocr.onemodel").is_file()
        && dir.join("onnxruntime.dll").is_file()
}

fn find_snippingtool_oneocr() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("OCR_TRANSLATOR_ONEOCR_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("SnippingTool"));
            candidates.push(dir.join("resources").join("SnippingTool"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local.clone())
                .join("OCR-Translator")
                .join("SnippingTool"),
        );
        candidates.push(
            PathBuf::from(local)
                .join("DN-OCR-Translator")
                .join("SnippingTool"),
        );
    }
    candidates.push(PathBuf::from("cache").join("SnippingTool"));
    candidates.push(PathBuf::from(
        "F:\\AI\\LunaTranslator\\src\\cache\\SnippingTool",
    ));
    if let Ok(entries) = std::fs::read_dir("C:\\Program Files\\WindowsApps") {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("Microsoft.ScreenSketch_") && name.contains("_x64__") {
                candidates.push(path.join("SnippingTool"));
            }
        }
    }
    candidates.into_iter().find(|dir| check_oneocr_dir(dir))
}

fn html_unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

pub fn available_windows_ocr_languages() -> Result<Vec<OcrLanguageInfo>> {
    let languages =
        OcrEngine::AvailableRecognizerLanguages().context("读取 Windows OCR 可用语言失败")?;
    let mut out = Vec::new();
    for i in 0..languages.Size()? {
        let lang = languages.GetAt(i)?;
        out.push(OcrLanguageInfo {
            tag: lang.LanguageTag()?.to_string(),
            display_name: lang.DisplayName()?.to_string(),
        });
    }
    Ok(out)
}

pub async fn recognize_png_windows_ocr(png: &[u8], lang: &str) -> Result<String> {
    let variants = windows_ocr_image_variants(png).unwrap_or_else(|_| vec![png.to_vec()]);
    let mut last_error = None;
    for variant in variants {
        match recognize_png_windows_ocr_variant(&variant, lang).await {
            Ok(text) if !text.trim().is_empty() => return Ok(text),
            Ok(_) => last_error = Some(anyhow!("未识别到文本")),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("未识别到文本")))
}

async fn recognize_png_windows_ocr_variant(png: &[u8], lang: &str) -> Result<String> {
    let stream = InMemoryRandomAccessStream::new().context("创建 OCR 图像流失败")?;
    let writer = DataWriter::CreateDataWriter(&stream).context("创建 OCR 数据写入器失败")?;
    writer.WriteBytes(png).context("写入 OCR PNG 失败")?;
    writer.StoreAsync()?.get().context("提交 OCR PNG 失败")?;
    stream.Seek(0).context("重置 OCR 图像流失败")?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?
        .get()
        .context("解码 OCR PNG 失败")?;
    let bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied)?
        .get()
        .context("转换 OCR 位图失败")?;
    let lang = normalize_ocr_lang(lang);
    let language = Language::CreateLanguage(&HSTRING::from(lang))
        .with_context(|| format!("创建 OCR 语言失败：{lang}"))?;
    if !OcrEngine::IsLanguageSupported(&language)
        .with_context(|| format!("检查 OCR 语言失败：{lang}"))?
    {
        bail!("系统未安装 {lang} 的 Windows OCR 语言包");
    }
    let engine = OcrEngine::TryCreateFromLanguage(&language)
        .with_context(|| format!("创建 Windows OCR 引擎失败：{lang}"))?;
    let result = engine
        .RecognizeAsync(&bitmap)?
        .get()
        .context("Windows OCR 识别失败")?;
    let lines = result.Lines().context("读取 OCR 行失败")?;
    let mut out = Vec::new();
    for i in 0..lines.Size()? {
        if let Ok(line) = lines.GetAt(i) {
            if let Ok(text) = line.Text() {
                let text = text.to_string();
                if !text.trim().is_empty() {
                    out.push(text);
                }
            }
        }
    }
    let text = out.join("\n").trim().to_string();
    Ok(text)
}

fn windows_ocr_image_variants(png: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut variants = vec![png.to_vec()];
    let image = image::load_from_memory(png)
        .context("解码 OCR 预处理图片失败")?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let min_height_scale = if height < 90 {
        ((90 + height - 1) / height).max(2)
    } else {
        1
    };
    let min_width_scale = if width < 360 {
        ((360 + width - 1) / width).max(2)
    } else {
        1
    };
    let scale = min_height_scale.max(min_width_scale).clamp(2, 5);
    variants.push(preprocess_windows_ocr_image(&image, scale, false)?);
    variants.push(preprocess_windows_ocr_image(&image, scale, true)?);
    Ok(variants)
}

pub fn preprocess_png_for_windows_ocr(png: &[u8]) -> Result<Vec<u8>> {
    let image = image::load_from_memory(png)
        .context("解码 OCR 预处理图片失败")?
        .to_rgba8();
    let (_, height) = image.dimensions();
    let scale = if height < 90 {
        ((90 + height - 1) / height).max(2)
    } else {
        2
    }
    .clamp(2, 5);
    preprocess_windows_ocr_image(&image, scale, true)
}

fn preprocess_windows_ocr_image(
    image: &image::RgbaImage,
    scale: u32,
    high_contrast: bool,
) -> Result<Vec<u8>> {
    let (width, height) = image.dimensions();
    let average_luma = if high_contrast {
        let total = image.pixels().fold(0u64, |acc, pixel| {
            acc + (0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2])) as u64
        });
        total / u64::from(width.max(1) * height.max(1))
    } else {
        255
    };
    let scaled = image::imageops::resize(
        image,
        width * scale,
        height * scale,
        image::imageops::FilterType::Lanczos3,
    );
    let padding = 32;
    let mut canvas = image::RgbaImage::from_pixel(
        scaled.width() + padding * 2,
        scaled.height() + padding * 2,
        image::Rgba([255, 255, 255, 255]),
    );
    for (x, y, pixel) in scaled.enumerate_pixels() {
        let mut out = *pixel;
        if high_contrast {
            let lum = (0.299 * f32::from(pixel[0])
                + 0.587 * f32::from(pixel[1])
                + 0.114 * f32::from(pixel[2])) as u8;
            let value = if average_luma < 128 {
                if lum > 150 {
                    0
                } else {
                    255
                }
            } else if lum < 185 {
                0
            } else {
                255
            };
            out = image::Rgba([value, value, value, 255]);
        } else {
            out[3] = 255;
        }
        canvas.put_pixel(x + padding, y + padding, out);
    }
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut out, image::ImageFormat::Png)
        .context("编码 OCR 预处理图片失败")?;
    Ok(out.into_inner())
}

pub async fn recognize_png_snippingtool_oneocr(png: &[u8]) -> Result<String> {
    let variants = oneocr_image_variants(png).unwrap_or_else(|_| vec![png.to_vec()]);
    tokio::task::spawn_blocking(move || {
        let mut last_error = None;
        for variant in variants {
            match recognize_png_snippingtool_oneocr_blocking(&variant) {
                Ok(text) if !text.trim().is_empty() => return Ok(text),
                Ok(_) => last_error = Some(anyhow!("未识别到文本")),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("未识别到文本")))
    })
    .await
    .context("OneOCR 工作线程失败")?
}

fn oneocr_image_variants(png: &[u8]) -> Result<Vec<Vec<u8>>> {
    let image = image::load_from_memory(png)
        .context("解码 OneOCR 预处理图片失败")?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if width >= 50 && height >= 50 {
        return Ok(vec![png.to_vec()]);
    }
    let scale_w = if width < 50 {
        (50 + width - 1) / width
    } else {
        1
    };
    let scale_h = if height < 50 {
        (50 + height - 1) / height
    } else {
        1
    };
    let scale = scale_w.max(scale_h).clamp(2, 6);
    Ok(vec![preprocess_oneocr_image(&image, scale)?])
}

fn preprocess_oneocr_image(image: &image::RgbaImage, scale: u32) -> Result<Vec<u8>> {
    let (width, height) = image.dimensions();
    let scaled = image::imageops::resize(
        image,
        width * scale,
        height * scale,
        image::imageops::FilterType::Lanczos3,
    );
    let padding = 8;
    let mut canvas = image::RgbaImage::from_pixel(
        scaled.width() + padding * 2,
        scaled.height() + padding * 2,
        image::Rgba([255, 255, 255, 255]),
    );
    for (x, y, pixel) in scaled.enumerate_pixels() {
        let mut out = *pixel;
        out[3] = 255;
        canvas.put_pixel(x + padding, y + padding, out);
    }
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut out, image::ImageFormat::Png)
        .context("编码 OneOCR 预处理图片失败")?;
    Ok(out.into_inner())
}

#[repr(C)]
struct OneOcrImage {
    t: i32,
    col: i32,
    row: i32,
    _unk: i32,
    step: i64,
    data_ptr: i64,
}

type CreateOcrInitOptions = unsafe extern "C" fn(*mut i64) -> i64;
type CreateOcrProcessOptions = unsafe extern "C" fn(*mut i64) -> i64;
type CreateOcrPipeline = unsafe extern "C" fn(*const c_char, *const c_char, i64, *mut i64) -> i64;
type OcrInitOptionsSetUseModelDelayLoad = unsafe extern "C" fn(i64, i8) -> i64;
type OcrProcessOptionsSetMaxRecognitionLineCount = unsafe extern "C" fn(i64, i64) -> i64;
type RunOcrPipeline = unsafe extern "C" fn(i64, *mut OneOcrImage, i64, *mut i64) -> i64;
type GetOcrLineCount = unsafe extern "C" fn(i64, *mut i64) -> i64;
type GetOcrLine = unsafe extern "C" fn(i64, i64, *mut i64) -> i64;
type GetOcrLineContent = unsafe extern "C" fn(i64, *mut i64) -> i64;
type ReleaseOcrResult = unsafe extern "C" fn(i64);

fn recognize_png_snippingtool_oneocr_blocking(png: &[u8]) -> Result<String> {
    let runtime_dir = find_snippingtool_oneocr().ok_or_else(|| {
        anyhow!("未安装 SnippingTool OneOCR 运行库，请先点击“安装 OneOCR 运行库”")
    })?;
    let image = image::load_from_memory(png)
        .context("OneOCR 解码截图失败")?
        .to_rgba8();
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width < 8 || height < 8 {
        bail!("选区太小，OneOCR 无法识别");
    }
    let mut pixels = image.into_raw();
    let step = (width as i64) * 4;

    unsafe {
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let new_path = format!("{};{}", runtime_dir.display(), old_path.to_string_lossy());
        std::env::set_var("PATH", new_path);

        let _onnx = libloading::Library::new(runtime_dir.join("onnxruntime.dll"))
            .with_context(|| format!("加载 onnxruntime.dll 失败：{}", runtime_dir.display()))?;
        let oneocr = libloading::Library::new(runtime_dir.join("oneocr.dll"))
            .with_context(|| format!("加载 oneocr.dll 失败：{}", runtime_dir.display()))?;

        let create_init: libloading::Symbol<CreateOcrInitOptions> =
            oneocr.get(b"CreateOcrInitOptions")?;
        let set_delay: libloading::Symbol<OcrInitOptionsSetUseModelDelayLoad> =
            oneocr.get(b"OcrInitOptionsSetUseModelDelayLoad")?;
        let create_pipeline: libloading::Symbol<CreateOcrPipeline> =
            oneocr.get(b"CreateOcrPipeline")?;
        let create_process_options: libloading::Symbol<CreateOcrProcessOptions> =
            oneocr.get(b"CreateOcrProcessOptions")?;
        let set_line_count: libloading::Symbol<OcrProcessOptionsSetMaxRecognitionLineCount> =
            oneocr.get(b"OcrProcessOptionsSetMaxRecognitionLineCount")?;
        let run_pipeline: libloading::Symbol<RunOcrPipeline> = oneocr.get(b"RunOcrPipeline")?;
        let get_line_count: libloading::Symbol<GetOcrLineCount> = oneocr.get(b"GetOcrLineCount")?;
        let get_line: libloading::Symbol<GetOcrLine> = oneocr.get(b"GetOcrLine")?;
        let get_line_content: libloading::Symbol<GetOcrLineContent> =
            oneocr.get(b"GetOcrLineContent")?;
        let release_result: libloading::Symbol<ReleaseOcrResult> =
            oneocr.get(b"ReleaseOcrResult")?;

        let mut ctx = 0i64;
        ensure_oneocr_ok(create_init(&mut ctx), "CreateOcrInitOptions")?;
        ensure_oneocr_ok(set_delay(ctx, 0), "OcrInitOptionsSetUseModelDelayLoad")?;

        let model = CString::new(
            runtime_dir
                .join("oneocr.onemodel")
                .to_string_lossy()
                .as_bytes(),
        )
        .context("OneOCR 模型路径包含非法字符")?;
        let key = CString::new("kj)TGtrK>f]b[Piow.gU+nC@s\"\"\"\"\"\"4")?;
        let mut pipeline = 0i64;
        ensure_oneocr_ok(
            create_pipeline(model.as_ptr(), key.as_ptr(), ctx, &mut pipeline),
            "CreateOcrPipeline",
        )?;

        let mut options = 0i64;
        ensure_oneocr_ok(
            create_process_options(&mut options),
            "CreateOcrProcessOptions",
        )?;
        ensure_oneocr_ok(
            set_line_count(options, 1000),
            "OcrProcessOptionsSetMaxRecognitionLineCount",
        )?;

        let mut img = OneOcrImage {
            t: 3,
            col: width,
            row: height,
            _unk: 0,
            step,
            data_ptr: pixels.as_mut_ptr() as *mut c_void as i64,
        };
        let mut result = 0i64;
        ensure_oneocr_ok(
            run_pipeline(pipeline, &mut img, options, &mut result),
            "RunOcrPipeline",
        )?;
        if result == 0 {
            bail!("OneOCR 返回空结果");
        }

        let mut line_count = 0i64;
        ensure_oneocr_ok(get_line_count(result, &mut line_count), "GetOcrLineCount")?;
        let mut lines = Vec::new();
        for index in 0..line_count {
            let mut line = 0i64;
            ensure_oneocr_ok(get_line(result, index, &mut line), "GetOcrLine")?;
            if line == 0 {
                continue;
            }
            let mut content_ptr = 0i64;
            ensure_oneocr_ok(
                get_line_content(line, &mut content_ptr),
                "GetOcrLineContent",
            )?;
            if content_ptr == 0 {
                continue;
            }
            let text = CStr::from_ptr(content_ptr as *const c_char)
                .to_string_lossy()
                .trim()
                .to_string();
            if !text.is_empty() {
                lines.push(text);
            }
        }
        release_result(result);

        let text = lines.join("\n").trim().to_string();
        if text.is_empty() {
            bail!("未识别到文本");
        }
        Ok(text)
    }
}

fn ensure_oneocr_ok(code: i64, action: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        bail!("{action} 失败，OneOCR 返回码 {code}");
    }
}

fn normalize_ocr_lang(lang: &str) -> &'static str {
    match lang {
        "zh-CN" | "zh" | "zh-Hans" => "zh-Hans",
        "zh-TW" | "zh-Hant" | "cht" => "zh-Hant",
        "ja" | "jp" => "ja",
        "ko" => "ko",
        "en" | "auto" | "" => "en-US",
        _ => "en-US",
    }
}
