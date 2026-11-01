use super::*;
use crate::{
    newbee_official::tests::{client, me, reply},
    test_support::{Reply, Server, TestDir},
};
#[test]
fn a_lost_official_changelog_clear_requires_the_original_file_and_reads_without_repeating_the_edit()
{
    let dir = TestDir::new();
    let request=ChangelogEdit::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin-changelog.edit","file_id":91,"changelog":""}"#).unwrap();
    let read = |id, text| reply(json!({"file_id":id,"changelog":text}));
    let server = Server::new(vec![
        me(71),
        read(91, json!("Original")),
        Reply::Drop,
        read(92, json!("")),
        read(91, Value::Null),
        read(91, json!("External edit after completion")),
        reply(Value::Null),
        read(91, json!("")),
    ]);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.edit_changelog(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(client.edit_changelog(&request).unwrap_err().exit_code, 6);
    let recovered = client.edit_changelog(&request).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    // Mutable edits read current state even after their original receipt ended.
    let fresh = client.edit_changelog(&request).unwrap();
    assert_eq!(fresh["changed"], true);
    assert_eq!(fresh["author_writes"], 1);
    assert_ne!(recovered["operation_id"], fresh["operation_id"]);
    let requests = server.finish();
    assert_eq!(requests.len(), 8);
    let edits = requests
        .iter()
        .filter(|request| request.line == "POST /creator/cli/v1/mod_file/edit_changelog HTTP/1.1")
        .collect::<Vec<_>>();
    assert_eq!(edits.len(), 2);
    for edit in edits {
        assert_eq!(
            input::value(&edit.body).unwrap(),
            json!({"file_id":91,"changelog":""})
        );
    }
}
#[test]
fn official_review_reply_acceptance_and_visibility_are_separate_and_the_original_receipt_is_reused()
{
    let dir = TestDir::new();
    let request=ReviewReply::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.review.reply","review_id":9007199254740993,"content":"!WA:2!PRIVATE_REPLY_IMPORT"}"#).unwrap();
    let plan = request.plan();
    assert_eq!(plan["credential_reads"], false);
    assert!(!plan.to_string().contains("PRIVATE_REPLY_IMPORT"));
    assert!(!dir.0.join("publish").exists());
    let server = Server::new(vec![
        me(71),
        reply(
            json!({"reply_id":991,"content":"!WA:2!PRIVATE_REPLY_IMPORT","token":"PRIVATE_BACKEND_TOKEN"}),
        ),
    ]);
    let mut client = client(&dir, &server);
    let result = client.reply_review(&request).unwrap();
    assert_eq!(result["api_accepted"], true);
    assert_eq!(result["effect_verification"], "not_available");
    assert!(!result.to_string().contains("PRIVATE_"));
    assert_eq!(client.reply_review(&request).unwrap()["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].line,
        "POST /creator/cli/v1/content_review/reply HTTP/1.1"
    );
    assert_eq!(
        input::value(&requests[1].body).unwrap(),
        json!({"review_id":9007199254740993u64,"content":"!WA:2!PRIVATE_REPLY_IMPORT"})
    );
}
#[test]
fn an_uncertain_official_review_reply_remains_pending_without_another_submission() {
    let dir = TestDir::new();
    let request=ReviewReply::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.review.reply","review_id":7,"content":"A reply"}"#).unwrap();
    let server = Server::new(vec![me(71), Reply::Drop]);
    let mut client = client(&dir, &server);
    assert_eq!(
        client.reply_review(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(client.reply_review(&request).unwrap_err().exit_code, 6);
    assert_eq!(server.finish().len(), 2);
}
