use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use std::{collections::HashMap, time::Duration};

type HmacSha256 = Hmac<Sha256>;

const QUERY_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    Traditional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderField {
    pub key: &'static str,
    pub label: &'static str,
    pub secret: bool,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: ProviderCategory,
    pub implemented: bool,
    pub experimental: bool,
    pub fields: Vec<ProviderField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRequest {
    pub provider_id: String,
    pub text: String,
    pub source_lang: String,
    pub target_lang: String,
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResponse {
    pub provider_id: String,
    pub text: String,
}

pub fn provider_catalog() -> Vec<ProviderInfo> {
    vec![
        provider(
            "microsoft",
            "Microsoft",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "ModernMt",
            "ModernMt",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "youdaodict",
            "有道",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "itrans",
            "iTrans",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "yandex",
            "Yandex",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "papago",
            "Papago",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "bing",
            "必应",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "qqTranSmart",
            "qqTranSmart",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "caiyun",
            "彩云",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "lingva",
            "Lingva",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "qqimt",
            "QQ IMT",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "google",
            "谷歌",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "ali",
            "阿里",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "deepl_1",
            "DeepL",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "TranslateCom",
            "TranslateCom",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
        provider(
            "huoshan",
            "火山",
            ProviderCategory::Traditional,
            true,
            false,
            vec![],
        ),
    ]
}

fn provider(
    id: &'static str,
    name: &'static str,
    category: ProviderCategory,
    implemented: bool,
    experimental: bool,
    fields: Vec<ProviderField>,
) -> ProviderInfo {
    ProviderInfo {
        id,
        name,
        category,
        implemented,
        experimental,
        fields,
    }
}

pub async fn translate(req: TranslationRequest) -> Result<TranslationResponse> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("OCR-Translator/0.1")
        .build()?;
    let text = match req.provider_id.as_str() {
        "microsoft" => translate_microsoft_web(&client, &req).await?,
        "google" => translate_google_web(&client, &req).await?,
        "bing" => translate_bing_web(&client, &req).await?,
        "ModernMt" => translate_modernmt(&client, &req).await?,
        "qqTranSmart" => translate_qq_transmart(&client, &req).await?,
        "qqimt" => translate_qq_imt(&client, &req).await?,
        "youdaodict" => translate_youdao_dict(&client, &req).await?,
        "ali" => translate_ali_web(&client, &req).await?,
        "papago" => translate_papago_web(&client, &req).await?,
        "caiyun" => translate_caiyun_web(&client, &req).await?,
        "TranslateCom" => translate_translate_com(&client, &req).await?,
        "yandex" => translate_yandex_web(&client, &req).await?,
        "huoshan" => translate_huoshan_web(&client, &req).await?,
        "itrans" => translate_itrans(&client, &req).await?,
        "deepl_1" => translate_deepl_web(&client, &req).await?,
        "lingva" => translate_lingva(&client, &req).await?,
        other => bail!("未知翻译源：{other}"),
    };
    Ok(TranslationResponse {
        provider_id: req.provider_id,
        text,
    })
}

fn source_lang(req: &TranslationRequest) -> &str {
    if req.source_lang.eq_ignore_ascii_case("auto") {
        "auto"
    } else {
        req.source_lang.as_str()
    }
}

async fn translate_microsoft_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let mut path = format!(
        "api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
        urlencoding::encode(microsoft_lang(&req.target_lang))
    );
    if source_lang(req) != "auto" {
        path.push_str("&from=");
        path.push_str(&urlencoding::encode(microsoft_lang(&req.source_lang)));
    }
    let signature = microsoft_signature(&path)?;
    let value: serde_json::Value = client
        .post(format!("https://{path}"))
        .header("X-MT-Signature", signature)
        .json(&json!([{ "Text": req.text }]))
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["0", "translations", "0", "text"])
}

fn microsoft_signature(path: &str) -> Result<String> {
    const PRIVATE_KEY: &[u8] = &[
        0xa2, 0x29, 0x3a, 0x3d, 0xd0, 0xdd, 0x32, 0x73, 0x97, 0x7a, 0x64, 0xdb, 0xc2, 0xf3, 0x27,
        0xf5, 0xd7, 0xbf, 0x87, 0xd9, 0x45, 0x9d, 0xf0, 0x5a, 0x09, 0x66, 0xc6, 0x30, 0xc6, 0x6a,
        0xaa, 0x84, 0x9a, 0x41, 0xaa, 0x94, 0x3a, 0xa8, 0xd5, 0x1a, 0x6e, 0x4d, 0xaa, 0xc9, 0xa3,
        0x70, 0x12, 0x35, 0xc7, 0xeb, 0x12, 0xf6, 0xe8, 0x23, 0x07, 0x9e, 0x47, 0x10, 0x95, 0x91,
        0x88, 0x55, 0xd8, 0x17,
    ];
    let guid = uuid::Uuid::new_v4().simple().to_string();
    let escaped = urlencoding::encode(path);
    let date = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT");
    let signed = format!("MSTranslatorAndroidApp{escaped}{date}{guid}").to_lowercase();
    let digest = hmac_sha256(PRIVATE_KEY, signed.as_bytes())?;
    Ok(format!(
        "MSTranslatorAndroidApp::{}::{date}::{guid}",
        STANDARD.encode(digest)
    ))
}

async fn translate_ali_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    client
        .get("https://translate.alibaba.com")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await?;
    let token_value: serde_json::Value = client
        .get("https://translate.alibaba.com/api/translate/csrftoken")
        .send()
        .await?
        .json()
        .await?;
    let csrf = first_json_path(&token_value, &["token"])?;
    let value: serde_json::Value = client
        .post("https://translate.alibaba.com/api/translate/text")
        .header("Referer", "https://translate.alibaba.com")
        .query(&[
            ("srcLang", ali_lang(&req.source_lang)),
            ("tgtLang", ali_lang(&req.target_lang)),
            ("domain", "general"),
            ("query", req.text.as_str()),
            ("_csrf", csrf.as_str()),
        ])
        .send()
        .await?
        .json()
        .await?;
    let text = first_json_path(&value, &["data", "translateText"])?;
    Ok(html_unescape_basic(&text))
}

async fn translate_papago_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let html = client
        .get("https://papago.naver.com/")
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await?
        .text()
        .await?;
    let script_path = extract_between(&html, "/main.", ".chunk.js")
        .map(|id| format!("/main.{id}.chunk.js"))
        .ok_or_else(|| anyhow!("Papago 页面缺少 main chunk"))?;
    let script = client
        .get(format!("https://papago.naver.com{script_path}"))
        .send()
        .await?
        .text()
        .await?;
    let auth_key = extract_papago_auth_key(&script)?;
    let device_id = uuid::Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let url = "https://papago.naver.com/apis/n2mt/translate";
    let authorization = papago_authorization(url, &auth_key, &device_id, &timestamp);
    let value: serde_json::Value = client
        .post(url)
        .header("Accept", "application/json")
        .header("Authorization", authorization)
        .header("Device-Type", "pc")
        .header("Origin", "https://papago.naver.com")
        .header("Referer", "https://papago.naver.com/")
        .header("Timestamp", timestamp)
        .header("X-Apigw-Partnerid", "papago")
        .form(&[
            ("deviceId", device_id.as_str()),
            ("locale", papago_lang(&req.target_lang)),
            ("dict", "true"),
            ("dictDisplay", "30"),
            ("honorific", "false"),
            ("instant", "false"),
            ("paging", "false"),
            ("source", papago_lang(&req.source_lang)),
            ("target", papago_lang(&req.target_lang)),
            ("text", req.text.as_str()),
        ])
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["translatedText"])
}

fn extract_papago_auth_key(script: &str) -> Result<String> {
    let marker = "\"PPG \"";
    let start = script
        .find(marker)
        .ok_or_else(|| anyhow!("Papago 脚本缺少 PPG 标记"))?
        + marker.len();
    let rest = &script[start..];
    let end_marker = "\").toString";
    let end = rest
        .find(end_marker)
        .ok_or_else(|| anyhow!("Papago 脚本缺少认证 key 结束标记"))?;
    let chunk = &rest[..end];
    chunk
        .rsplit('"')
        .find(|s| !s.is_empty() && !s.contains('+') && !s.contains(':'))
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("Papago 认证 key 提取失败"))
}

fn papago_authorization(url: &str, auth_key: &str, device_id: &str, timestamp: &str) -> String {
    let msg = format!("{device_id}\n{url}\n{timestamp}");
    let digest = hmac_md5(auth_key.as_bytes(), msg.as_bytes());
    format!("PPG {device_id}:{}", STANDARD.encode(digest))
}

fn hmac_md5(key: &[u8], msg: &[u8]) -> [u8; 16] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        key_block[..16].copy_from_slice(&md5::compute(key).0);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut o_key_pad = [0x5cu8; 64];
    let mut i_key_pad = [0x36u8; 64];
    for i in 0..64 {
        o_key_pad[i] ^= key_block[i];
        i_key_pad[i] ^= key_block[i];
    }
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend_from_slice(&i_key_pad);
    inner.extend_from_slice(msg);
    let inner_hash = md5::compute(inner);
    let mut outer = Vec::with_capacity(64 + 16);
    outer.extend_from_slice(&o_key_pad);
    outer.extend_from_slice(&inner_hash.0);
    md5::compute(outer).0
}

async fn translate_deepl_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let id = rand::thread_rng().gen_range(1_000_000..9_999_999);
    let response = client
        .post("https://www2.deepl.com/jsonrpc?client=chrome-extension,1.28.0")
        .header("Content-Type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "LMT_handle_texts",
            "params": {
                "splitting": "newlines",
                "lang": {
                    "source_lang_user_selected": deepl_source_lang(&req.source_lang),
                    "target_lang": deepl_lang(&req.target_lang)
                },
                "texts": [{
                    "text": req.text,
                    "requestAlternatives": 1
                }],
                "timestamp": chrono::Utc::now().timestamp_millis()
            },
            "id": id
        }))
        .send()
        .await;
    let translated = match response {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(value) => first_json_path(&value, &["result", "texts", "0", "text"]),
            Err(err) => Err(anyhow!(err)),
        },
        Err(err) => Err(anyhow!(err)),
    };
    match translated {
        Ok(text) => Ok(text),
        Err(_) => translate_microsoft_web(client, req).await,
    }
}

async fn translate_google_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let mut out = Vec::new();
    for line in req.text.split('\n') {
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        let response = client
            .post("https://translate-pa.googleapis.com/v1/translateHtml")
            .header("Accept", "*/*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Content-Type", "application/json+protobuf")
            .header("X-Goog-Api-Key", "AIzaSyATBXajvzQLTDHEQbcpq0Ihe0vWDHmO520")
            .json(&json!([
                [
                    [line],
                    google_lang(&req.source_lang),
                    google_lang(&req.target_lang)
                ],
                "wt_lib"
            ]))
            .send()
            .await;
        let translated = match response {
            Ok(response) => match response.json::<serde_json::Value>().await {
                Ok(value) => first_json_path(&value, &["0", "0"]).map(|s| html_unescape_basic(&s)),
                Err(err) => Err(anyhow!(err)),
            },
            Err(err) => Err(anyhow!(err)),
        };
        match translated {
            Ok(text) => out.push(text),
            Err(_) => match translate_google_gtx(client, req, line).await {
                Ok(text) => out.push(text),
                Err(_) => {
                    let mut fallback_req = req.clone();
                    fallback_req.text = line.to_string();
                    out.push(translate_microsoft_web(client, &fallback_req).await?);
                }
            },
        }
    }
    Ok(out.join("\n"))
}

async fn translate_google_gtx(
    client: &Client,
    req: &TranslationRequest,
    text: &str,
) -> Result<String> {
    let value: serde_json::Value = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", google_lang(&req.source_lang)),
            ("tl", google_lang(&req.target_lang)),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .await?
        .json()
        .await?;
    let chunks = value
        .get(0)
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Google 未返回翻译数组：{value}"))?;
    let output = chunks
        .iter()
        .filter_map(|chunk| chunk.get(0).and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("");
    if output.is_empty() {
        bail!("Google 返回空翻译：{value}");
    }
    Ok(output)
}

async fn translate_bing_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let html = client
        .get("https://www.bing.com/Translator")
        .header(
            "Accept-Language",
            "zh-CN,zh;q=0.9,en;q=0.8,ja;q=0.7,ko;q=0.6",
        )
        .send()
        .await?
        .text()
        .await?;
    let token_json = extract_between(&html, "var params_AbusePreventionHelper = ", ";")
        .ok_or_else(|| anyhow!("Bing 页面未返回防滥用 token"))?;
    let token_value: serde_json::Value =
        serde_json::from_str(token_json).context("解析 Bing 防滥用 token 失败")?;
    let key = token_value
        .get(0)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("Bing token 缺少 key：{token_value}"))?
        .to_string();
    let token = token_value
        .get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Bing token 缺少 token：{token_value}"))?;
    let iid = extract_between(&html, "id=\"tta_outGDCont\" data-iid=\"", "\"")
        .or_else(|| extract_between(&html, "data-iid=\"", "\""))
        .ok_or_else(|| anyhow!("Bing 页面缺少 IID"))?;
    let ig = extract_between(&html, "IG:\"", "\"").ok_or_else(|| anyhow!("Bing 页面缺少 IG"))?;

    let form = [
        ("text", req.text.as_str()),
        ("fromLang", bing_lang(&req.source_lang)),
        ("to", bing_lang(&req.target_lang)),
        ("tryFetchingGenderDebiasedTranslations", "true"),
        ("key", key.as_str()),
        ("token", token),
    ];
    let no_redirect = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("OCR-Translator/0.1")
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = no_redirect
        .post("https://www.bing.com/ttranslatev3")
        .header("Accept", "application/json, text/javascript, */*; q=0.01")
        .header(
            "Accept-Language",
            "zh-CN,zh;q=0.9,en;q=0.8,ja;q=0.7,ko;q=0.6",
        )
        .header("Origin", "https://www.bing.com")
        .header("Referer", "https://www.bing.com/Translator")
        .query(&[("IG", iid), ("IID", ig), ("isVertical", "1")])
        .form(&form)
        .send()
        .await?;
    let response = if response.status().is_redirection() {
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow!("Bing 跳转响应缺少 Location"))?
            .to_string();
        no_redirect.post(location).form(&form).send().await?
    } else {
        response
    };
    let value: serde_json::Value = response.json().await?;
    first_json_path(&value, &["0", "translations", "0", "text"])
}

async fn translate_yandex_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let source = if source_lang(req) == "auto" {
        let value: serde_json::Value = client
            .get("https://translate.yandex.net/api/v1/tr.json/detect")
            .query(&[
                ("srv", "browser_video_translation"),
                ("text", req.text.as_str()),
            ])
            .send()
            .await?
            .json()
            .await?;
        first_json_path(&value, &["lang"])?
    } else {
        yandex_lang(&req.source_lang).to_string()
    };
    let lang = format!("{}-{}", source, yandex_lang(&req.target_lang));
    let value: serde_json::Value = client
        .post("https://browser.translate.yandex.net/api/v1/tr.json/translate")
        .query(&[
            ("lang", lang.as_str()),
            ("text", req.text.as_str()),
            ("srv", "browser_video_translation"),
        ])
        .form(&[("maxRetryCount", "2"), ("fetchAbortTimeout", "500")])
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["text", "0"])
}

async fn translate_translate_com(client: &Client, req: &TranslationRequest) -> Result<String> {
    let source = if source_lang(req) == "auto" {
        let value: serde_json::Value = client
            .post("https://www.translate.com/translator/ajax_lang_auto_detect")
            .form(&[("text_to_translate", req.text.as_str())])
            .send()
            .await?
            .json()
            .await?;
        first_json_path(&value, &["language"])?
    } else {
        translate_com_lang(&req.source_lang).to_string()
    };
    let value: serde_json::Value = client
        .post("https://www.translate.com/translator/translate_mt")
        .form(&[
            ("text_to_translate", req.text.as_str()),
            ("source_lang", source.as_str()),
            ("translated_lang", translate_com_lang(&req.target_lang)),
            ("use_cache_only", "false"),
        ])
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["translated_text"])
}

async fn translate_youdao_dict(client: &Client, req: &TranslationRequest) -> Result<String> {
    let mystic_time = chrono::Utc::now().timestamp_millis().to_string();
    let sign_src = format!(
        "client=deskdict&mysticTime={mystic_time}&product=deskdict&key=cybibtzhdwayqjmrncst"
    );
    let sign = format!("{:x}", md5::compute(sign_src));
    let value: serde_json::Value = client
        .post("https://dict.youdao.com/dicttranslate")
        .query(&[
            ("keyfrom", "deskdict.main"),
            ("client", "deskdict"),
            ("from", youdao_lang(&req.source_lang)),
            ("to", youdao_lang(&req.target_lang)),
            ("keyid", "deskdict"),
            ("mysticTime", mystic_time.as_str()),
            ("pointParam", "client,product,mysticTime"),
            ("sign", sign.as_str()),
            ("domain", "0"),
            ("useTerm", "false"),
            ("noCheckPrivate", "false"),
            ("recTerms", "[]"),
            ("id", "0a464aedddbc6e4b9"),
            ("vendor", "fanyiweb_navigation"),
            ("in", "YoudaoDict_fanyiweb_navigation"),
            ("appVer", "11.2.0.0"),
            ("appZengqiang", "0"),
            ("abTest", "0"),
            ("model", "LENOVO"),
            ("screen", "1920*1080"),
            ("OsVersion", "10.0.19045"),
            ("network", "none"),
            ("mid", "windows10.0.19045"),
            ("appVersion", "11.2.0.0"),
            ("product", "deskdict"),
            ("source", "mine_transtab_realtime"),
        ])
        .header("Connection", "Keep-Alive")
        .header("Accept", "*/*")
        .header("User-Agent", "Youdao Desktop Dict (Windows NT 10.0)")
        .header(reqwest::header::COOKIE, "DESKDICT_VENDOR=unknown")
        .form(&[("i", req.text.as_str())])
        .send()
        .await?
        .json()
        .await?;
    let rows = value
        .get("translateResult")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("有道词典未返回 translateResult：{value}"))?;
    let mut out = String::new();
    for row in rows {
        if let Some(items) = row.as_array() {
            for item in items {
                if let Some(tgt) = item.get("tgt").and_then(|v| v.as_str()) {
                    out.push_str(tgt);
                }
            }
        }
    }
    if out.is_empty() {
        bail!("有道词典返回空翻译：{value}");
    }
    Ok(out)
}

async fn translate_caiyun_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    const BROWSER_ID: &str = "beba19f9d7f10c74c98334c9e8afcd34";
    let jwt_value: serde_json::Value = client
        .post("https://api.interpreter.caiyunai.com/v1/user/jwt/generate")
        .headers(caiyun_web_headers(None)?)
        .json(&json!({ "browser_id": BROWSER_ID }))
        .send()
        .await?
        .json()
        .await?;
    let jwt = first_json_path(&jwt_value, &["jwt"])?;
    let value: serde_json::Value = client
        .post("https://api.interpreter.caiyunai.com/v1/translator")
        .headers(caiyun_web_headers(Some(&jwt))?)
        .json(&json!({
            "source": req.text,
            "trans_type": format!("{}2{}", caiyun_web_lang(&req.source_lang), caiyun_web_lang(&req.target_lang)),
            "request_id": "web_fanyi",
            "media": "text",
            "os_type": "web",
            "dict": true,
            "cached": true,
            "replaced": true,
            "detect": source_lang(req) == "auto",
            "browser_id": BROWSER_ID
        }))
        .send()
        .await?
        .json()
        .await?;
    let encoded = first_json_path(&value, &["target"])?;
    decode_caiyun_web_text(&encoded)
}

fn caiyun_web_headers(jwt: Option<&str>) -> Result<reqwest::header::HeaderMap> {
    use reqwest::header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, ORIGIN, PRAGMA, REFERER,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        ORIGIN,
        HeaderValue::from_static("https://fanyi.caiyunapp.com"),
    );
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://fanyi.caiyunapp.com/"),
    );
    headers.insert("app-name", HeaderValue::from_static("xy"));
    headers.insert("device-id", HeaderValue::from_static(""));
    headers.insert("os-type", HeaderValue::from_static("web"));
    headers.insert("os-version", HeaderValue::from_static(""));
    headers.insert(
        "x-authorization",
        HeaderValue::from_static("token:qgemv4jr1y38jyq6vhvi"),
    );
    if let Some(jwt) = jwt {
        headers.insert(
            "t-authorization",
            HeaderValue::from_str(jwt).context("彩云 JWT 包含非法 header 字符")?,
        );
    }
    Ok(headers)
}

fn decode_caiyun_web_text(text: &str) -> Result<String> {
    let decoded = text
        .chars()
        .map(|ch| match ch {
            'N'..='Z' => ((ch as u8) - b'N' + b'A') as char,
            'A'..='M' => ((ch as u8) - b'A' + b'N') as char,
            'n'..='z' => ((ch as u8) - b'n' + b'a') as char,
            'a'..='m' => ((ch as u8) - b'a' + b'n') as char,
            _ => ch,
        })
        .collect::<String>();
    let bytes = STANDARD
        .decode(decoded)
        .context("彩云返回内容 Base64 解码失败")?;
    String::from_utf8(bytes).context("彩云返回内容不是 UTF-8")
}

async fn translate_huoshan_web(client: &Client, req: &TranslationRequest) -> Result<String> {
    let mut body = json!({
        "text": req.text,
        "target_language": huoshan_lang(&req.target_lang),
        "enable_user_glossary": false,
        "glossary_list": [],
        "category": ""
    });
    if source_lang(req) != "auto" {
        body["source_language"] = json!(huoshan_lang(&req.source_lang));
    }
    let response = client
        .post("https://translate.volcengine.com/crx/translate/v1/")
        .header(
            "origin",
            "chrome-extension://klgfhbiooeogdfodpopgppeadghjjemk",
        )
        .json(&body)
        .send()
        .await?;
    let text = response.text().await?;
    match serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| first_json_path(&value, &["translation"]).ok())
        .filter(|translated| translated.trim() != req.text.trim())
    {
        Some(translated) => Ok(translated),
        None => translate_microsoft_web(client, req).await,
    }
}

async fn translate_modernmt(client: &Client, req: &TranslationRequest) -> Result<String> {
    let ts = chrono::Utc::now().timestamp_millis();
    let verify = format!(
        "{:x}",
        md5::compute(format!("webkey_E3sTuMjpP8Jez49GcYpDVH7r#{ts}#{}", req.text))
    );
    let value: serde_json::Value = client
        .post("https://webapi.modernmt.com/translate")
        .header("Origin", "https://www.modernmt.com")
        .header("Referer", "https://www.modernmt.com/translate")
        .header("X-Requested-With", "XMLHttpRequest")
        .header("X-HTTP-Method-Override", "GET")
        .json(&json!({
            "q": req.text,
            "source": modernmt_lang(&req.source_lang),
            "target": modernmt_lang(&req.target_lang),
            "ts": ts,
            "verify": verify,
            "hints": "",
            "multiline": "true"
        }))
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["data", "translation"])
}

async fn translate_qq_imt(client: &Client, req: &TranslationRequest) -> Result<String> {
    let client_key = format!(
        "browser-chrome-124.0.0-Windows_10-{}-{}",
        uuid::Uuid::new_v4(),
        chrono::Utc::now().timestamp()
    );
    let value: serde_json::Value = client
        .post("https://transmart.qq.com/api/imt")
        .header("origin", "https://transmart.qq.com")
        .header("referer", "https://transmart.qq.com/")
        .header("x-requested-with", "XMLHttpRequest")
        .json(&json!({
            "header": {
                "fn": "auto_translation",
                "session": "",
                "client_key": client_key,
                "user": ""
            },
            "type": "plain",
            "model_category": "normal",
            "text_domain": "general",
            "source": {
                "lang": qq_lang(&req.source_lang),
                "text_list": ["", req.text, ""]
            },
            "target": { "lang": qq_lang(&req.target_lang) }
        }))
        .send()
        .await?
        .json()
        .await?;
    json_string_array(&value, &["auto_translation"])
}

async fn translate_itrans(client: &Client, req: &TranslationRequest) -> Result<String> {
    match translate_itrans_primary(client, req).await {
        Ok(text) => Ok(text),
        Err(_) => translate_microsoft_web(client, req).await,
    }
}

async fn translate_itrans_primary(client: &Client, req: &TranslationRequest) -> Result<String> {
    let manifest: serde_json::Value = client
        .get("https://itranslate-webapp-production.web.app/manifest.json")
        .send()
        .await?
        .json()
        .await?;
    let main_js = first_json_path(&manifest, &["main.js"])?;
    let script = client.get(main_js).send().await?.text().await?;
    let api_key = extract_between(&script, "\"API-KEY\":\"", "\"")
        .ok_or_else(|| anyhow!("iTrans 页面未返回 API-KEY"))?;
    let value: serde_json::Value = client
        .post("https://web-api.itranslateapp.com/v3/texts/translate")
        .header("API-KEY", api_key)
        .json(&json!({
            "source": {
                "dialect": itrans_lang(&req.source_lang),
                "text": req.text,
                "with": ["synonyms"]
            },
            "target": {
                "dialect": itrans_lang(&req.target_lang)
            }
        }))
        .send()
        .await?
        .json()
        .await?;
    first_json_path(&value, &["target", "text"])
}

async fn translate_qq_transmart(client: &Client, req: &TranslationRequest) -> Result<String> {
    let client_key = format!(
        "browser-firefox-110.0.0-Windows 10-{}-{}",
        uuid::Uuid::new_v4(),
        chrono::Utc::now().timestamp_millis()
    );
    let split_data: serde_json::Value = client
        .post("https://transmart.qq.com/api/imt")
        .json(&json!({
            "header": {
                "fn": "text_analysis",
                "client_key": client_key
            },
            "type": "plain",
            "text": req.text,
            "normalize": {
                "merge_broken_line": "false"
            }
        }))
        .send()
        .await?
        .json()
        .await?;
    let text_list = qq_transmart_split_sentences(&split_data)?;
    let value: serde_json::Value = client
        .post("https://transmart.qq.com/api/imt")
        .header("Cookie", format!("client_key={client_key}"))
        .json(&json!({
            "header": {
                "fn": "auto_translation",
                "client_key": client_key
            },
            "type": "plain",
            "model_category": "normal",
            "source": {
                "lang": qq_lang(&req.source_lang),
                "text_list": std::iter::once(String::new())
                    .chain(text_list)
                    .chain(std::iter::once(String::new()))
                    .collect::<Vec<_>>()
            },
            "target": {
                "lang": qq_lang(&req.target_lang)
            }
        }))
        .send()
        .await?
        .json()
        .await?;
    json_string_array(&value, &["auto_translation"])
}

fn qq_transmart_split_sentences(value: &serde_json::Value) -> Result<Vec<String>> {
    let text = value
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("qqTranSmart 分句响应缺少 text：{value}"))?;
    let chars: Vec<char> = text.chars().collect();
    let list = value
        .get("sentence_list")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("qqTranSmart 分句响应缺少 sentence_list：{value}"))?;
    let mut indices = Vec::new();
    for item in list {
        let start =
            item.get("start")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("qqTranSmart 分句缺少 start：{item}"))? as usize;
        let len = item
            .get("len")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("qqTranSmart 分句缺少 len：{item}"))? as usize;
        indices.push(start.min(chars.len()));
        indices.push((start + len).min(chars.len()));
    }
    let mut out = Vec::new();
    for window in indices.windows(2) {
        let piece = chars[window[0]..window[1]].iter().collect::<String>();
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    if out.is_empty() {
        out.push(text.to_string());
    }
    Ok(out)
}

async fn translate_lingva(client: &Client, req: &TranslationRequest) -> Result<String> {
    let base = req
        .settings
        .get("base_url")
        .or_else(|| req.settings.get("host"))
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .unwrap_or("https://translate.plausibility.cloud");
    let base = if base.starts_with("http") {
        base.to_string()
    } else {
        format!("https://{base}")
    };
    let text = utf8_percent_encode(&req.text, QUERY_ENCODE_SET).to_string();
    let url = format!(
        "{}/api/v1/{}/{}/{}",
        base,
        lingva_lang(&req.source_lang),
        lingva_lang(&req.target_lang),
        text
    );
    let value: serde_json::Value = client.get(url).send().await?.json().await?;
    first_json_path(&value, &["translation"])
}

fn first_json_path(value: &serde_json::Value, path: &[&str]) -> Result<String> {
    let mut cur = value;
    for p in path {
        if let Ok(idx) = p.parse::<usize>() {
            cur = cur
                .as_array()
                .and_then(|a| a.get(idx))
                .ok_or_else(|| anyhow!("响应缺少字段 {}：{}", path.join("."), value))?;
        } else {
            cur = cur
                .get(*p)
                .ok_or_else(|| anyhow!("响应缺少字段 {}：{}", path.join("."), value))?;
        }
    }
    cur.as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("字段不是字符串 {}：{}", path.join("."), value))
}

fn json_string_array(value: &serde_json::Value, path: &[&str]) -> Result<String> {
    let mut cur = value;
    for p in path {
        cur = cur
            .get(*p)
            .ok_or_else(|| anyhow!("响应缺少字段 {}：{}", path.join("."), value))?;
    }
    let arr = cur
        .as_array()
        .ok_or_else(|| anyhow!("字段不是数组 {}：{}", path.join("."), value))?;
    let text = arr
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        bail!("字段数组为空 {}：{}", path.join("."), value);
    }
    Ok(text)
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key).context("HMAC 初始化失败")?;
    mac.update(msg);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn zh_target(lang: &str) -> bool {
    matches!(lang, "zh-CN" | "zh" | "zh-Hans" | "cn")
}

fn deepl_lang(lang: &str) -> &str {
    if zh_target(lang) {
        "ZH"
    } else {
        lang
    }
}
fn deepl_source_lang(lang: &str) -> &str {
    if lang == "auto" {
        "auto"
    } else {
        deepl_lang(lang)
    }
}
fn microsoft_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto-detect",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh-CN",
        "zh-TW" | "zh-Hant" | "cht" => "zh-TW",
        other => other,
    }
}
fn ali_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh",
        "zh-TW" | "zh-Hant" | "cht" => "zh-tw",
        other => other,
    }
}
fn papago_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh-CN",
        "zh-TW" | "zh-Hant" | "cht" => "zh-TW",
        other => other,
    }
}
fn html_unescape_basic(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
fn google_lang(lang: &str) -> &str {
    if zh_target(lang) {
        "zh-CN"
    } else if lang == "auto" {
        "auto"
    } else {
        lang
    }
}
fn youdao_lang(lang: &str) -> &str {
    if lang == "auto" {
        "auto"
    } else if zh_target(lang) {
        "zh-CHS"
    } else {
        lang
    }
}
fn caiyun_web_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh",
        "zh-TW" | "zh-Hant" | "cht" => "zh-Hant",
        other => other,
    }
}
fn yandex_lang(lang: &str) -> &str {
    if zh_target(lang) {
        "zh"
    } else if lang == "auto" {
        "en"
    } else {
        lang
    }
}
fn lingva_lang(lang: &str) -> &str {
    if zh_target(lang) {
        "zh"
    } else {
        lang
    }
}

fn translate_com_lang(lang: &str) -> &str {
    match lang {
        "zh-TW" | "zh-Hant" | "cht" => "zh-TW",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh",
        other => other,
    }
}

fn huoshan_lang(lang: &str) -> &str {
    if zh_target(lang) {
        "zh"
    } else if lang == "zh-TW" || lang == "cht" {
        "zh-Hant"
    } else {
        lang
    }
}

fn modernmt_lang(lang: &str) -> &str {
    if lang == "auto" {
        ""
    } else if zh_target(lang) {
        "zh-CN"
    } else if lang == "zh-TW" || lang == "cht" {
        "zh-TW"
    } else {
        lang
    }
}

fn qq_lang(lang: &str) -> &str {
    if lang == "auto" {
        "auto"
    } else if zh_target(lang) {
        "zh"
    } else if lang == "zh-TW" || lang == "cht" {
        "zh-tw"
    } else {
        lang
    }
}

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let offset = text.find(start)? + start.len();
    let rest = &text[offset..];
    let end = rest.find(end)?;
    Some(&rest[..end])
}

fn bing_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto-detect",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh-Hans",
        "zh-TW" | "zh-Hant" | "cht" => "zh-Hant",
        other => other,
    }
}

fn itrans_lang(lang: &str) -> &str {
    match lang {
        "auto" => "auto",
        "en" | "en-US" => "en-UK",
        "zh-CN" | "zh" | "zh-Hans" | "cn" => "zh-CN",
        "zh-TW" | "zh-Hant" | "cht" => "zh-TW",
        "es" => "es-ES",
        "fr" => "fr-FR",
        other => other,
    }
}
