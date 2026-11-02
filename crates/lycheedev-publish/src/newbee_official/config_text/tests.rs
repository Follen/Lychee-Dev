use super::*;
use crate::{
    newbee_official::tests::{client, me, reply},
    test_support::{Reply, Server, TestDir},
};
use std::fs;
fn state(title: &str, intro: Value, body: &str, format: u64) -> Vec<Reply> {
    vec![
        reply(json!({"id":91,"title":title,"intro":intro,"content_format":format})),
        reply(json!({"id":91,"content":body,"content_format":format})),
    ]
}
fn request(dir: &TestDir, fields: Value) -> ConfigTextUpdate {
    let mut doc = json!({"schema":"lycheedev.publish.v1.newbee.official.config.update","id":91});
    for (key, value) in fields.as_object().unwrap() {
        doc[key] = value.clone();
    }
    ConfigTextUpdate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap()
}
#[test]
fn official_partial_text_edit_clears_explicit_intro_and_preserves_unselected_html_body_and_format()
{
    let dir = TestDir::new();
    let request = request(&dir, json!({"title":"New title","intro":""}));
    let mut replies = vec![me(71)];
    replies.extend(state(
        "Old title",
        json!("Old intro"),
        "<p>Unchanged body</p>",
        1,
    ));
    replies.push(reply(Value::Null));
    replies.extend(state("New title", Value::Null, "<p>Unchanged body</p>", 1));
    replies.extend(state("New title", Value::Null, "<p>Unchanged body</p>", 1));
    let server = Server::new(replies);
    let mut client = client(&dir, &server);
    let result = client.update_config_text(&request).unwrap();
    assert_eq!(result["intro_verification"], "readback");
    assert_eq!(result["api_acceptance"], "acknowledged");
    assert_eq!(client.update_config_text(&request).unwrap()["reused"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests[3].line,
        "POST /creator/cli/v1/share_config/push_text HTTP/1.1"
    );
    assert_eq!(
        input::value(&requests[3].body).unwrap(),
        json!({"id":91,"title":"New title","intro":""})
    );
}
#[test]
fn lost_markdown_update_recovers_its_original_body_and_preserved_title_without_inventing_summary_verification()
 {
    let dir = TestDir::new();
    let text = "# UI\nPRIVATE_CONFIGURATION_BODY";
    fs::write(dir.0.join("body.md"), text).unwrap();
    let request = request(&dir, json!({"file":"body.md"}));
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert!(!plan.to_string().contains("PRIVATE_"));
    assert!(!dir.0.join("publish").exists());
    let mut replies = vec![me(71)];
    replies.extend(state("Original title", json!("Custom summary"), "old", 2));
    replies.push(Reply::Drop);
    replies.extend(state("Original title", json!("Platform summary"), text, 2));
    let server = Server::new(replies);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.update_config_text(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    let result = client.update_config_text(&request).unwrap();
    assert_eq!(result["recovered"], true);
    assert_eq!(result["intro_verification"], "not_available");
    assert_eq!(result["api_acceptance"], "not_observed");
    assert!(!result.to_string().contains("PRIVATE_"));
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        input::value(&requests[3].body).unwrap(),
        json!({"id":91,"content":text})
    );
}
#[test]
fn pending_text_update_keeps_the_original_preserved_title_hash_instead_of_rebasing_on_wrong_readback()
 {
    let dir = TestDir::new();
    fs::write(dir.0.join("body.md"), "new body").unwrap();
    let request = request(&dir, json!({"file":"body.md","intro":"New intro"}));
    let mut replies = vec![me(71)];
    replies.extend(state(
        "Original title",
        json!("Original intro"),
        "old body",
        2,
    ));
    replies.push(reply(Value::Null));
    for _ in 0..2 {
        replies.extend(state(
            "Wrong preserved title",
            json!("New intro"),
            "new body",
            2,
        ));
    }
    replies.extend(state("Original title", json!("New intro"), "new body", 2));
    let server = Server::new(replies);
    let mut client = client(&dir, &server);
    for _ in 0..2 {
        assert_eq!(
            client.update_config_text(&request).unwrap_err().exit_code,
            6
        );
    }
    let result = client.update_config_text(&request).unwrap();
    assert_eq!(result["recovered"], true);
    assert_eq!(result["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 10);
    assert_eq!(requests.iter().filter(|request|request.line=="POST /creator/cli/v1/share_config/push_text HTTP/1.1").count(),1);
}
#[test]
fn html_body_updates_are_rejected_before_submission_and_local_text_admission_needs_no_profile() {
    let dir = TestDir::new();
    fs::write(dir.0.join("body.md"), "Markdown").unwrap();
    let request = request(&dir, json!({"file":"body.md"}));
    let mut replies = vec![me(71)];
    replies.extend(state("Title", json!("Intro"), "<p>HTML</p>", 1));
    let server = Server::new(replies);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.update_config_text(&request).unwrap_err().exit_code,
        3
    );
    assert_eq!(server.finish().len(), 3);
    fs::write(dir.0.join("body.md"), [0xff]).unwrap();
    assert_eq!(
        request
            .plan(&OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
    for fields in [json!({}), json!({"intro":null}), json!({"title":"  "})] {
        let mut doc =
            json!({"schema":"lycheedev.publish.v1.newbee.official.config.update","id":91});
        for (key, value) in fields.as_object().unwrap() {
            doc[key] = value.clone();
        }
        assert_eq!(
            ConfigTextUpdate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0)
                .err()
                .unwrap()
                .exit_code,
            4
        );
    }
}
#[test]
fn an_unobservable_derived_summary_after_a_lost_response_does_not_become_false_success() {
    let dir = TestDir::new();
    fs::write(dir.0.join("body.md"), "same body").unwrap();
    let request = request(&dir, json!({"file":"body.md"}));
    let mut replies = vec![me(71)];
    replies.extend(state("Title", json!("Original intro"), "same body", 2));
    replies.push(Reply::Drop);
    replies.extend(state("Title", json!("Original intro"), "same body", 2));
    let server = Server::new(replies);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.update_config_text(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(
        client.update_config_text(&request).unwrap_err().exit_code,
        6
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.ends_with("/share_config/push_text HTTP/1.1"))
            .count(),
        1
    );
}
