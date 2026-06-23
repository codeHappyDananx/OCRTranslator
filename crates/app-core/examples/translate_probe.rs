use anyhow::Result;
use app_core::{translate, TranslationRequest};
use std::{collections::HashMap, env, io::Read};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let provider = args.next().unwrap_or_else(|| "bing".to_string());
    let source_lang = args.next().unwrap_or_else(|| "auto".to_string());
    let target_lang = args.next().unwrap_or_else(|| "zh-CN".to_string());
    let text = if let Some(text) = args.next() {
        text
    } else {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
    };
    let result = translate(TranslationRequest {
        provider_id: provider,
        text,
        source_lang,
        target_lang,
        settings: HashMap::new(),
    })
    .await?;
    println!("{}", result.text.trim());
    Ok(())
}
