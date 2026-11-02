use super::*;

fn previous_input(e: &SlotEnvelope, at: i64) -> Record {
    let mut i = identity(&e.runtime);
    i.owner = e.owner.clone();
    i.fence = e.fence;
    let mut input = sample(&i, at);
    input.schema = PREVIOUS_SCHEMA.into();
    record(Kind::Input, &input, &i.runtime, 1)
}

#[test]
fn previous_two_key_capability_requires_actual_matching_input_but_allows_stale() {
    let mut i = identity(&"1".repeat(32));
    i.schema = PREVIOUS_SCHEMA.into();
    i.input_state = PREVIOUS_SCHEMA.into();
    for mode in [
        "two_key",
        "stale",
        "three_key",
        "empty",
        "foreign",
        "unavailable",
    ] {
        let mut input = sample(&i, if mode == "stale" { 1_000 } else { 10_000 });
        input.schema = PREVIOUS_SCHEMA.into();
        if mode == "foreign" {
            input.guid = "other-character".into();
        }
        if mode == "unavailable" {
            input.input_blocked = Some(true);
            input.reason = "input_binding_unavailable".into();
            input.bindings = Some(InputBindings::default());
        }
        let mut raw = serde_json::to_value(input).unwrap();
        if mode == "three_key" {
            raw["bindings"] = serde_json::json!({
                "wake":"ALT-CTRL-F12", "submit":"ALT-CTRL-SHIFT-F12", "close":"ALT-CTRL-["
            });
        }
        let records = if mode == "empty" {
            vec![]
        } else {
            vec![record(Kind::Input, &raw, &i.runtime, 1)]
        };
        let mut core = make_core(vec![Ok(records)]);
        assert_eq!(
            activation_input_capability(&mut core.reader, &i, 10_000, &control()).unwrap(),
            matches!(mode, "two_key" | "stale"),
            "{mode}"
        );
        assert!(core.witness.is_none());
        assert_eq!(events(&core), ["read:5"]);
    }
}

#[test]
fn previous_input_authorizes_only_fresh_reload_and_one_use_native_witness() {
    for mode in ["reload", "invoke", "stale", "escape"] {
        let e = envelope(if mode == "invoke" {
            "prepare"
        } else {
            "reload"
        });
        let mut r = previous_input(&e, if mode == "stale" { 9_000 } else { 10_000 });
        if mode == "escape" {
            let mut raw: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
            raw["escapeLimit"] = 3.into();
            r = record(Kind::Input, &raw, &e.runtime, 1);
        }
        let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r.clone()]), Ok(vec![r])]);
        let c = control();
        let observation = core.observe_input(&c, &e, 0, PREVIOUS_SCHEMA);
        if mode != "reload" {
            assert!(observation.is_err(), "{mode}");
            assert!(!events(&core).iter().any(|e| e == "send"));
            continue;
        }
        let a = InputAction {
            kind: InputKind::Reload,
            envelope: e,
            observation: Some(observation.unwrap()),
            capability: PREVIOUS_SCHEMA.into(),
            after_millis: 0,
        };
        let sent = core.input(&c, &a);
        assert!(sent.error.is_none(), "{sent:?}");
        assert_eq!(sent.outcome.disposition, InputDisposition::Submitted);
        assert_eq!(sent.outcome.messages_queued, 6);
        assert_eq!(core.input(&c, &a).outcome.messages_queued, 0);
        assert!(!events(&core).iter().any(|e| e == "acquire"));
    }
}

#[test]
fn previous_identity_is_discovery_only_and_cannot_supply_receipts_or_retirement() {
    let mut i = identity(&"2".repeat(32));
    i.schema = PREVIOUS_SCHEMA.into();
    i.input_state = PREVIOUS_SCHEMA.into();
    let r = record(Kind::Identity, &i, &i.runtime, 1);
    assert_eq!(decode_identity(&r.contract().unwrap(), "3.2.0").unwrap(), i);
    let receipt = crate::mailbox::Receipt {
        identity: i.clone(),
        ..Default::default()
    };
    assert!(receipt.validate().is_err());
    let old = identity(&"1".repeat(32));
    let mut core = make_core(replacement_records(&i, 10_000, 11_100));
    assert!(
        core.observe_runtime_replacement(&control(), &old)
            .unwrap()
            .is_none()
    );
    assert!(!events(&core).iter().any(|e| e.starts_with("wait:")));
}

#[test]
fn previous_input_decoder_rejects_every_non_reload_action() {
    let mut e = envelope("reload");
    let record = previous_input(&e, 10_000);
    for action in [
        "reload",
        "invoke",
        "bind",
        "prepare",
        "commit",
        "confirm",
        "release",
        "unbind",
        "escape",
        "reload_fallback",
    ] {
        e.action = action.into();
        assert_eq!(
            decode_input(&record.contract().unwrap(), &e.expected(), 0, 10_000).is_ok(),
            action == "reload",
            "{action}"
        );
    }
    let mut i = identity(&"2".repeat(32));
    i.schema = PREVIOUS_SCHEMA.into();
    assert!(
        i.validate().is_err(),
        "old identity schema must also advertise the old input name"
    );
}
