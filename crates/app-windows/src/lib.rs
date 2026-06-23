pub mod cursor;
pub mod geometry;
pub mod hotkey;
pub mod ocr;
pub mod pipeline;
pub mod screen;

pub use cursor::{cursor_position, release_cursor_lock};
pub use geometry::{Point, Rect};
pub use hotkey::{GlobalInputEvent, GlobalInputHook, KeyboardEvent, MouseButton, MouseEvent};
pub use ocr::{
    available_windows_ocr_languages, detect_ocr_engines, install_snippingtool_oneocr_runtime,
    preprocess_png_for_windows_ocr, preview_snippingtool_oneocr_package,
    recognize_png_snippingtool_oneocr, recognize_png_windows_ocr, OcrEngineStatus, OcrLanguageInfo,
    OneOcrPackageInfo,
};
pub use pipeline::{recognize_png_pipeline, OcrPipelineRequest, OcrPipelineResult};
pub use screen::capture_rect_png;
