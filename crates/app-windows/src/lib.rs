pub mod cursor;
pub mod geometry;
pub mod hotkey;
pub mod native_selection;
pub mod ocr;
pub mod overlay_window;
pub mod pipeline;
pub mod screen;

pub use cursor::{cursor_position, left_mouse_down, release_cursor_lock, right_mouse_down};
pub use geometry::{Point, Rect};
pub use hotkey::{GlobalInputEvent, GlobalInputHook, KeyboardEvent, MouseButton, MouseEvent};
pub use native_selection::{close_native_selection_windows, select_rect_native};
pub use ocr::{
    available_windows_ocr_languages, detect_ocr_engines, install_snippingtool_oneocr_runtime,
    preprocess_png_for_windows_ocr, preview_snippingtool_oneocr_package,
    recognize_png_snippingtool_oneocr, recognize_png_windows_ocr, OcrEngineStatus, OcrLanguageInfo,
    OneOcrPackageInfo,
};
pub use overlay_window::{start_native_window_resize, NativeResizeDirection};
pub use pipeline::{recognize_png_pipeline, OcrPipelineRequest, OcrPipelineResult, OcrTextLine};
pub use screen::{capture_rect_png, virtual_screen_rect};
