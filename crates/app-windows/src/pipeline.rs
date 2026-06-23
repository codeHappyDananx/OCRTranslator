use crate::{
    preprocess_png_for_windows_ocr, recognize_png_snippingtool_oneocr, recognize_png_windows_ocr,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrPipelineRequest {
    pub engine: String,
    pub source_lang: String,
    pub save_preprocessed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrPipelineResult {
    pub engine: String,
    pub text: String,
    pub preprocessed_png: Option<Vec<u8>>,
}

pub async fn recognize_png_pipeline(
    png: &[u8],
    request: OcrPipelineRequest,
) -> Result<OcrPipelineResult> {
    let preprocessed_png = if request.save_preprocessed {
        preprocess_png_for_windows_ocr(png).ok()
    } else {
        None
    };

    let mut attempts = Vec::new();
    match request.engine.as_str() {
        "snippingtool" => attempts.push("snippingtool"),
        "windows" | "" if request.source_lang == "auto" => {
            attempts.push("snippingtool");
            attempts.push("windows");
        }
        "windows" | "" => attempts.push("windows"),
        other => anyhow::bail!("未知 OCR 引擎：{other}"),
    }

    let mut errors = Vec::new();
    for engine in attempts {
        let result = match engine {
            "snippingtool" => recognize_png_snippingtool_oneocr(png).await,
            "windows" => recognize_png_windows_ocr(png, &request.source_lang).await,
            _ => unreachable!(),
        };
        match result {
            Ok(text) if !text.trim().is_empty() => {
                return Ok(OcrPipelineResult {
                    engine: engine.to_string(),
                    text: text.trim().to_string(),
                    preprocessed_png,
                });
            }
            Ok(_) => errors.push(format!("{engine}: 未识别到文本")),
            Err(err) => errors.push(format!("{engine}: {err}")),
        }
    }

    Err(anyhow!(errors.join(" / ")))
}
