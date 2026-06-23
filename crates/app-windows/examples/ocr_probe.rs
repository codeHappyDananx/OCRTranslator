use anyhow::{bail, Context, Result};
use app_windows::{
    install_snippingtool_oneocr_runtime, recognize_png_snippingtool_oneocr,
    recognize_png_windows_ocr,
};
use std::{env, fs};

#[tokio::main]
async fn main() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(|s| s.as_str()) == Some("install-oneocr") {
        let path = install_snippingtool_oneocr_runtime().await?;
        println!("OneOCR runtime: {}", path.display());
        return Ok(());
    }
    if args.len() < 2 {
        bail!(
            "Usage: cargo run -p app-windows --example ocr_probe -- <windows|oneocr> <image.png>"
        );
    }
    let engine = &args[0];
    let image_path = &args[1];
    let png = fs::read(image_path).with_context(|| format!("读取图片失败：{image_path}"))?;
    let text = match engine.as_str() {
        "windows" => recognize_png_windows_ocr(&png, "en").await?,
        "oneocr" | "snippingtool" => recognize_png_snippingtool_oneocr(&png).await?,
        other => bail!("Unknown OCR engine: {other}"),
    };
    println!("{text}");
    Ok(())
}
