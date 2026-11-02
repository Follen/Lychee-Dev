//! Public author results keep metadata, summarize raw imports, and remove auth.
use regex::Regex;
use reqwest::Url;
use serde_json::{Value, json};
use std::sync::OnceLock;

fn redact_text(text: &str) -> String {
    static WA: OnceLock<Regex> = OnceLock::new();
    static BEARER: OnceLock<Regex> = OnceLock::new();
    static JWT: OnceLock<Regex> = OnceLock::new();
    let wa =
        WA.get_or_init(|| Regex::new(r#"!WA:\d+![^\s"']+"#).expect("fixed WA redaction pattern"));
    let bearer = BEARER.get_or_init(|| {
        Regex::new(r"(?i)\bBearer[ \t]+[A-Za-z0-9._~+/=-]+")
            .expect("fixed Bearer redaction pattern")
    });
    let jwt = JWT.get_or_init(|| {
        Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]+\b")
            .expect("fixed JWT redaction pattern")
    });
    let text = wa.replace_all(text, "[REDACTED_WA]");
    let text = bearer.replace_all(&text, "Bearer [REDACTED]");
    jwt.replace_all(&text, "[REDACTED_JWT]").into_owned()
}

pub(crate) fn public(value: Value, public_text: &[&str]) -> Value {
    match value {
        Value::Object(object)=>Value::Object(object.into_iter().map(|(key,value)|{
            let normalized=key.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect::<String>();
            let secret=["token","accesstoken","refreshtoken","resourcetoken","jwt","jwttoken","cookie","setcookie","authorization","authentication","clientno","clientid","clientsecret","deviceid","deviceproof","cred","credential","signature","xamzcredential","xamzsignature","xamzsecuritytoken","signedurl","uploadurl","presigneduri","presignedurl","apikey","authkey","password","secret","pkey","userpkey","phone","phonenum","phonenumber","mobile","smscode","verificationcode","logincode","oauthcode","wxcode","cookies","hkey","nonce","phoneencrypted","oauthstate","oauthcallback","oauthnext","oauthuuid","xxhhtokenid"];
            let raw=["content","wastr","twastr","rawwtf","wtfzip","importstring","contenttext","codetext","base64","logobase64","screenshotbase64s","encoded"];
            let value=if secret.contains(&normalized.as_str()) {json!("[REDACTED]")}
                else if raw.contains(&normalized.as_str())&&!public_text.contains(&key.as_str()) {
                    match value {
                        Value::String(text)=>json!({"length":text.chars().count(),"sha256":lycheedev_core::storage::sha256(text.as_bytes())}),
                        other=>{let bytes=serde_json::to_vec(&other).expect("public JSON value");json!({"bytes":bytes.len(),"sha256":lycheedev_core::storage::sha256(&bytes)})}
                    }
                } else {public(value,public_text)};
            (key,value)
        }).collect()),
        Value::Array(values)=>Value::Array(values.into_iter().map(|v|public(v,public_text)).collect()),
        Value::String(text)=>{
            let signed=Url::parse(&text).ok().filter(|url|matches!(url.scheme(),"https"|"http"))
                .is_some_and(|url|url.query_pairs().any(|(name,_)|{
                    let name=name.to_ascii_lowercase();name.starts_with("x-amz-")||["token","signature","sig","q-signature","authorization","security-token"].contains(&name.as_str())
                }));
            if signed {json!("[REDACTED_URL]")}else{Value::String(redact_text(&text))}
        }
        other=>other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_descriptions_keep_text_but_remove_inline_imports_and_bearer_values() {
        let value = public(
            json!({"description":"Fix text: !WA:2!private-import next sentence; Bearer credential-private end."}),
            &[],
        );
        assert_eq!(
            value["description"],
            "Fix text: [REDACTED_WA] next sentence; Bearer [REDACTED] end."
        );
    }
    #[test]
    fn business_metadata_and_exact_ids_survive_while_raw_imports_and_auth_do_not() {
        let source = json!({"id":9007199254740993u64,"name":"Author title","data":[{"Access_Token":"credential-private","phone":"18812345678","t_wa_str":"!WA:2!private","url":"https://oss.invalid/key?X-Amz-Signature=private"}],"content":"Public author description"});
        let output = public(source, &["content"]);
        let wire = output.to_string();
        for secret in [
            "credential-private",
            "18812345678",
            "!WA:2!private",
            "X-Amz-Signature=private",
        ] {
            assert!(!wire.contains(secret));
        }
        assert_eq!(output["id"].as_u64(), Some(9007199254740993));
        assert_eq!(output["name"], "Author title");
        assert_eq!(output["content"], "Public author description");
        assert_eq!(output["data"][0]["t_wa_str"]["length"], 13);
        assert!(output["data"][0]["t_wa_str"]["sha256"].is_string());
    }
}
