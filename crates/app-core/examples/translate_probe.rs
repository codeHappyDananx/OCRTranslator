use app_core::{translate, TranslationRequest};
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let provider_id = args.next().unwrap_or_else(|| "bing".to_string());
    let text = args
        .next()
        .unwrap_or_else(|| "Cooldown reduction\nQuest reward".to_string());
    let result = translate(TranslationRequest {
        provider_id,
        text,
        source_lang: "en".to_string(),
        target_lang: "zh-CN".to_string(),
        settings: HashMap::new(),
    })
    .await;

    match result {
        Ok(response) => {
            println!("{}", response.text);
        }
        Err(err) => {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
    }
}
