//! C2PA アサーションの読み取り。
//!
//! 目的は診断（なぜ X が「AIで作成」を付けるのか）を人間に説明することであって、
//! 署名の検証ではない。Lightroom のバージョン差で構造が変わっても落ちないよう、
//! 既知のキーを探して見つからなければ黙って省く方針で書いている。

use ciborium::value::Value;

/// IPTC の digitalSourceType 語彙のうち、AI の関与を示すもの。
const AI_SOURCE_TYPES: &[&str] = &[
    "trainedAlgorithmicMedia",
    "compositeWithTrainedAlgorithmicMedia",
    "algorithmicMedia",
];

/// 1 つのアクション。
#[derive(Debug, Clone, Default)]
pub struct Action {
    /// `c2pa.edited` など
    pub name: String,
    pub when: Option<String>,
    /// `Adobe Remove Object 1` のように名前とバージョンを繋いだもの
    pub software_agent: Option<String>,
    pub digital_source_type: Option<String>,
    /// `com.adobe.*` などの注目すべきパラメータ
    pub params: Vec<(String, String)>,
}

impl Action {
    /// このアクションが AI の関与を宣言しているか。
    pub fn declares_ai(&self) -> bool {
        match &self.digital_source_type {
            Some(t) => AI_SOURCE_TYPES.iter().any(|s| t.contains(s)),
            None => false,
        }
    }
}

/// `c2pa.actions.v2` の要約。
#[derive(Debug, Clone, Default)]
pub struct Actions {
    pub actions: Vec<Action>,
    /// `com.adobe.appEnforced` — 書き出し設定に関係なくアプリが強制付与したことを示す
    pub app_enforced: bool,
}

impl Actions {
    /// AI を宣言しているアクションがあるか。X の判定根拠になる。
    pub fn declares_ai(&self) -> bool {
        self.actions.iter().any(|a| a.declares_ai())
    }
}

/// `c2pa.claim.v2` の要約。
#[derive(Debug, Clone, Default)]
pub struct Claim {
    pub generator_name: Option<String>,
    pub generator_version: Option<String>,
    pub spec_version: Option<String>,
}

/// 署名者の情報。証明書チェーンの先頭（リーフ）から取る。
#[derive(Debug, Clone, Default)]
pub struct Signer {
    /// 署名者の CN
    pub subject_cn: Option<String>,
    /// 発行者の CN。トラストリスト外の一時 CA かどうかがここで分かる
    pub issuer_cn: Option<String>,
}

/// CBOR をデコードする。
pub fn decode(cbor: &[u8]) -> Option<Value> {
    ciborium::from_reader(cbor).ok()
}

/// マップから文字列キーで引く。
fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Map(entries) = v else { return None };
    entries
        .iter()
        .find(|(k, _)| matches!(k, Value::Text(t) if t == key))
        .map(|(_, v)| v)
}

/// 文字列として読む。数値や真偽値も表示用に文字列化する。
fn text(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) => Some(t.clone()),
        Value::Integer(i) => Some(format!("{}", i128::from(*i))),
        Value::Bool(b) => Some(b.to_string()),
        Value::Tag(_, inner) => text(inner),
        _ => None,
    }
}

fn get_text(v: &Value, key: &str) -> Option<String> {
    get(v, key).and_then(text)
}

/// 表示する価値のあるパラメータのキー。
const NOTABLE_PARAMS: &[&str] = &[
    "com.adobe.acr.value",
    "com.adobe.firefly.version",
    "com.adobe.genAiId",
    "com.adobe.acr",
];

/// `c2pa.actions.v2` を読む。
pub fn read_actions(cbor: &[u8]) -> Option<Actions> {
    let v = decode(cbor)?;
    let mut out = Actions::default();

    if let Some(Value::Array(items)) = get(&v, "actions") {
        for item in items {
            let mut a = Action {
                name: get_text(item, "action").unwrap_or_default(),
                when: get_text(item, "when"),
                digital_source_type: get_text(item, "digitalSourceType"),
                ..Default::default()
            };

            if let Some(agent) = get(item, "softwareAgent") {
                let name = get_text(agent, "name");
                let version = get_text(agent, "version");
                a.software_agent = match (name, version) {
                    (Some(n), Some(v)) => Some(format!("{n} {v}")),
                    (Some(n), None) => Some(n),
                    _ => None,
                };
            }

            if let Some(params) = get(item, "parameters") {
                for key in NOTABLE_PARAMS {
                    if let Some(val) = get_text(params, key) {
                        a.params.push((key.to_string(), val));
                    }
                }
            }

            out.actions.push(a);
        }
    }

    // templates[].templateParameters["com.adobe.appEnforced"]
    if let Some(Value::Array(templates)) = get(&v, "templates") {
        for t in templates {
            if let Some(tp) = get(t, "templateParameters") {
                if get_text(tp, "com.adobe.appEnforced").as_deref() == Some("true") {
                    out.app_enforced = true;
                }
            }
        }
    }

    Some(out)
}

/// `c2pa.claim.v2` を読む。
pub fn read_claim(cbor: &[u8]) -> Option<Claim> {
    let v = decode(cbor)?;
    let info = get(&v, "claim_generator_info")?;
    // 配列で来る場合と単一マップで来る場合がある
    let first = match info {
        Value::Array(items) => items.first()?,
        other => other,
    };
    Some(Claim {
        generator_name: get_text(first, "name"),
        generator_version: get_text(first, "version"),
        spec_version: get_text(first, "specVersion"),
    })
}

/// `c2pa.signature`（COSE_Sign1）から署名者を読む。
///
/// 保護ヘッダの `x5chain`（ラベル 33）に証明書チェーンが DER で入っている。
/// リーフ証明書の Issuer CN と Subject CN だけを取る。
pub fn read_signer(cbor: &[u8]) -> Option<Signer> {
    let v = decode(cbor)?;
    // COSE_Sign1 = Tag(18, [protected: bstr, unprotected, payload, signature])
    let arr = match &v {
        Value::Tag(_, inner) => match inner.as_ref() {
            Value::Array(a) => a,
            _ => return None,
        },
        Value::Array(a) => a,
        _ => return None,
    };

    let Value::Bytes(protected) = arr.first()? else {
        return None;
    };
    let header: Value = ciborium::from_reader(protected.as_slice()).ok()?;

    // ラベル 33 = x5chain
    let Value::Map(entries) = &header else {
        return None;
    };
    let chain = entries
        .iter()
        .find(|(k, _)| matches!(k, Value::Integer(i) if i128::from(*i) == 33))
        .map(|(_, v)| v)?;

    let leaf = match chain {
        Value::Array(certs) => match certs.first()? {
            Value::Bytes(b) => b,
            _ => return None,
        },
        Value::Bytes(b) => b,
        _ => return None,
    };

    let cns = common_names(leaf);
    Some(Signer {
        // TBSCertificate では issuer が subject より前に来る
        issuer_cn: cns.first().cloned(),
        subject_cn: cns.get(1).cloned(),
    })
}

/// DER 証明書から commonName を出現順に取り出す。
///
/// 完全な X.509 パーサは要らない。commonName の OID（2.5.4.3）は DER 上で
/// `06 03 55 04 03` という固定バイト列になるので、その直後の DirectoryString を読む。
fn common_names(der: &[u8]) -> Vec<String> {
    const OID_CN: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03];
    let mut out = Vec::new();
    let mut i = 0usize;

    while i + OID_CN.len() < der.len() {
        if &der[i..i + OID_CN.len()] != OID_CN {
            i += 1;
            continue;
        }
        let mut j = i + OID_CN.len();
        if j + 2 > der.len() {
            break;
        }
        // DirectoryString: UTF8String(0x0C) / PrintableString(0x13) / IA5String(0x16) など
        let tag = der[j];
        if !matches!(tag, 0x0C | 0x13 | 0x14 | 0x16 | 0x1E) {
            i = j;
            continue;
        }
        j += 1;
        // 短形式の長さのみ扱う（CN が 127 バイトを超えることは実質ない）
        let len = der[j] as usize;
        if der[j] & 0x80 != 0 || j + 1 + len > der.len() {
            i = j;
            continue;
        }
        j += 1;
        if let Ok(s) = std::str::from_utf8(&der[j..j + len]) {
            out.push(s.to_string());
        }
        i = j + len;
    }

    out
}

/// CBOR を JSON テキストに素通しする（`--json` 用）。
///
/// バイト列は 16 進文字列にする。既知フィールドの抽出に失敗しても生の内容を出せるように
/// しておくためのもので、往復変換は意図していない。
pub fn to_json(v: &Value) -> String {
    let mut s = String::new();
    write_json(v, &mut s);
    s
}

fn write_json(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Integer(i) => out.push_str(&format!("{}", i128::from(*i))),
        Value::Float(f) => out.push_str(&format!("{f}")),
        Value::Text(t) => write_json_string(t, out),
        Value::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            write_json_string(&hex, out);
        }
        Value::Tag(_, inner) => write_json(inner, out),
        Value::Array(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Map(entries) => {
            out.push('{');
            for (n, (k, val)) in entries.iter().enumerate() {
                if n > 0 {
                    out.push(',');
                }
                write_json_string(&text(k).unwrap_or_default(), out);
                out.push(':');
                write_json(val, out);
            }
            out.push('}');
        }
        _ => out.push_str("null"),
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
