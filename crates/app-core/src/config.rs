use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub source_lang: String,
    pub target_lang: String,
    pub ocr_engine: String,
    pub translator: String,
    pub hotkey: String,
    pub provider_settings: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    pub app: AppBehaviorConfig,
    pub overlay: OverlayConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppBehaviorConfig {
    #[serde(default)]
    pub close_to_tray: bool,
    #[serde(default = "default_ask_before_close")]
    pub ask_before_close: bool,
    #[serde(default)]
    pub auto_elevate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayConfig {
    #[serde(default = "default_result_mode")]
    pub result_mode: String,
    pub width: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub screen_margin: i32,
    #[serde(default = "default_overlay_max_height")]
    pub max_height: u32,
    pub opacity: f32,
    pub font_size: u32,
    pub no_drag_ms: u64,
    pub double_click_close: bool,
    #[serde(default)]
    pub show_source: bool,
    #[serde(default = "default_overlay_draggable")]
    pub draggable: bool,
    #[serde(default = "default_source_background")]
    pub source_background: String,
    #[serde(default = "default_translation_background")]
    pub translation_background: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            source_lang: "auto".to_string(),
            target_lang: "zh-CN".to_string(),
            ocr_engine: "snippingtool".to_string(),
            translator: "bing".to_string(),
            hotkey: "MouseX1".to_string(),
            provider_settings: HashMap::new(),
            app: AppBehaviorConfig::default(),
            overlay: OverlayConfig::default(),
        }
    }
}

impl Default for AppBehaviorConfig {
    fn default() -> Self {
        Self {
            close_to_tray: false,
            ask_before_close: default_ask_before_close(),
            auto_elevate: false,
        }
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            result_mode: default_result_mode(),
            width: 320,
            offset_x: 0,
            offset_y: 0,
            screen_margin: 12,
            max_height: default_overlay_max_height(),
            opacity: 0.55,
            font_size: 18,
            no_drag_ms: 500,
            double_click_close: true,
            show_source: false,
            draggable: true,
            source_background: default_source_background(),
            translation_background: default_translation_background(),
        }
    }
}

fn default_overlay_draggable() -> bool {
    true
}

fn default_result_mode() -> String {
    "text_overlay".to_string()
}

fn default_overlay_max_height() -> u32 {
    620
}

fn default_ask_before_close() -> bool {
    true
}

fn default_source_background() -> String {
    "#2858a5".to_string()
}

fn default_translation_background() -> String {
    "#127858".to_string()
}

pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("无法定位用户配置目录")?
        .join("OCR-Translator");
    Ok(dir)
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.json"))
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            if let Some(old_path) = legacy_config_path()? {
                let text = fs::read_to_string(&old_path)
                    .with_context(|| format!("读取旧配置失败：{}", old_path.display()))?;
                let mut cfg: Self = serde_json::from_str(&text)
                    .with_context(|| format!("解析旧配置失败：{}", old_path.display()))?;
                cfg.normalize();
                cfg.save()?;
                return Ok(cfg);
            }
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败：{}", path.display()))?;
        let missing_overlay_fields = !text.contains("\"show_source\"")
            || !text.contains("\"draggable\"")
            || !text.contains("\"max_height\"")
            || !text.contains("\"result_mode\"")
            || !text.contains("\"source_background\"")
            || !text.contains("\"translation_background\"");
        let missing_app_fields = !text.contains("\"app\"")
            || !text.contains("\"close_to_tray\"")
            || !text.contains("\"ask_before_close\"")
            || !text.contains("\"auto_elevate\"");
        let mut cfg: Self = serde_json::from_str(&text)
            .with_context(|| format!("解析配置失败：{}", path.display()))?;
        let before = cfg.clone();
        cfg.normalize();
        if missing_overlay_fields || missing_app_fields || cfg.normalized_differs_from(&before) {
            cfg.save()?;
        }
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let dir = config_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("创建配置目录失败：{}", dir.display()))?;
        let path = config_path()?;
        let text = serde_json::to_string_pretty(self)?;
        fs::write(&path, text).with_context(|| format!("写入配置失败：{}", path.display()))?;
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.ocr_engine = "snippingtool".to_string();
        if self.hotkey.trim().is_empty() {
            self.hotkey = "MouseX1".to_string();
        }
        if !matches!(
            self.overlay.result_mode.as_str(),
            "text_overlay" | "image_replace"
        ) {
            self.overlay.result_mode = default_result_mode();
        }
        self.overlay.width = self.overlay.width.clamp(180, 900);
        self.overlay.max_height = self.overlay.max_height.clamp(120, 1200);
        self.overlay.font_size = self.overlay.font_size.clamp(12, 48);
        self.overlay.opacity = self.overlay.opacity.clamp(0.05, 0.9);
        self.overlay.screen_margin = self.overlay.screen_margin.clamp(0, 120);
        self.overlay.no_drag_ms = self.overlay.no_drag_ms.max(500);
        if !is_hex_color(&self.overlay.source_background) {
            self.overlay.source_background = default_source_background();
        }
        if !is_hex_color(&self.overlay.translation_background) {
            self.overlay.translation_background = default_translation_background();
        }
    }

    fn normalized_differs_from(&self, other: &Self) -> bool {
        self.source_lang != other.source_lang
            || self.target_lang != other.target_lang
            || self.ocr_engine != other.ocr_engine
            || self.translator != other.translator
            || self.hotkey != other.hotkey
            || self.provider_settings != other.provider_settings
            || self.app != other.app
            || self.overlay != other.overlay
    }
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b) || (b'A'..=b'F').contains(b))
}

fn legacy_config_path() -> Result<Option<PathBuf>> {
    let Some(dir) = dirs::config_dir() else {
        return Ok(None);
    };
    let path = dir.join("DN-OCR-Translator").join("config.json");
    Ok(path.exists().then_some(path))
}
