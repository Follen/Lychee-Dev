//! Main-client editor textContent, without changing the submitted HTML.
use base64::Engine;
use html5gum::{DefaultEmitter, Token, Tokenizer};
use regex::Regex;
use std::sync::OnceLock;

pub(super) fn text(content: &str) -> String {
    let decoded = if content.contains('<') {
        None
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(content)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    };
    static MARKERS: OnceLock<Regex> = OnceLock::new();
    let markers = MARKERS.get_or_init(|| {
        Regex::new(r#"(?i:\[\[wa:data="[^"]*"\]\])|\[\[?img:key=(?:"[^"]+"|[^\]]+)\]\]?"#).unwrap()
    });
    let html = markers.replace_all(decoded.as_deref().unwrap_or(content), "");
    let mut emitter = DefaultEmitter::default();
    emitter.naively_switch_states(true);
    let mut hidden = Vec::new();
    let mut out = String::new();
    for token in Tokenizer::new_with_emitter(html.as_ref(), emitter) {
        match token.expect("string tokenizer has no I/O failures") {
            Token::StartTag(tag)
                if [b"script".as_slice(), b"style", b"iframe", b"object"]
                    .contains(&tag.name.as_ref()) =>
            {
                hidden.push(tag.name)
            }
            Token::EndTag(tag) => {
                if let Some(i) = hidden.iter().position(|name| name == &tag.name) {
                    hidden.truncate(i);
                }
            }
            Token::String(s) if hidden.is_empty() => {
                out.push_str(&String::from_utf8_lossy(&s.value))
            }
            _ => {}
        }
    }
    out.replace('\u{a0}', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_text_keeps_entities_and_quoted_attributes_without_embedded_imports() {
        let html = r#"<p title="a > b">作者&nbsp;&amp;&eacute;&#x1F41D;</p><script>if(a<b) "private";</script><style>hidden</style><iframe>hidden</iframe><object><b>hidden</b></object>[[wa:data="private-wa"]][img:key="private-image"]<p>后文 &copy without semicolon</p>"#;
        assert_eq!(text(html), "作者 &é🐝后文 © without semicolon");
        assert_eq!(
            text(&base64::engine::general_purpose::STANDARD.encode(html)),
            text(html)
        );
        assert_eq!(text("plain &lt; text"), "plain < text");
    }
}
