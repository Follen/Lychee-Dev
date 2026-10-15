//! Kind-specific validation. Deliberate oddities of fixed Go are preserved:
//! checkpoint dispatch precedes general epoch/field checks, identity/reset
//! dispatch precedes the non-receiver binding prohibition, and session request
//! IDs/reload nonces are not tightened beyond the actual released validators.
use super::*;

pub(super) fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(|c| c < ' ' || c == '\u{7f}')
}
fn optional_label(value: &str) -> Result<()> {
    if !value.is_empty() && !label(value) {
        Err(error("bridge.invalid_signal_label"))
    } else {
        Ok(())
    }
}
fn key(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn receiver_fields(s: &Signal) -> bool {
    !s.receiver_nonce.is_empty()
        || !s.attempt_id.is_empty()
        || s.body_bytes != 0
        || !s.body_adler32.is_empty()
        || !s.commit_nonce.is_empty()
        || s.accepted
}
fn bindings_present(s: &Signal) -> bool {
    !s.wake_binding.is_empty() || !s.submit_binding.is_empty() || !s.close_binding.is_empty()
}
fn digests_present(s: &Signal) -> bool {
    s.code_bytes != 0
        || s.report_bytes != 0
        || !s.code_adler32.is_empty()
        || !s.report_adler32.is_empty()
}

pub(super) fn validate(s: &Signal, control: &OperationControl) -> Result<()> {
    if s.schema != "lycheedev.signal.v1" {
        return Err(error("bridge.invalid_signal"));
    }
    if s.kind == "checkpoint" {
        return checkpoint(s, control);
    }
    if !s.work_state.is_empty()
        || !s.code_sha256.is_empty()
        || !s.receipt.is_empty()
        || s.resources_released
    {
        return Err(error("bridge.unexpected_checkpoint_fields"));
    }
    if !s.report_scope.is_empty()
        && (s.report_scope != "character-v1"
            || !matches!(s.kind.as_str(), "ready" | "receiver_ready"))
    {
        return Err(error("bridge.invalid_report_scope"));
    }
    if s.runtime_epoch > LUA_INTEGER_MAX {
        return Err(error("bridge.invalid_runtime_epoch"));
    }
    if !s.receiver_protocol.is_empty()
        && (s.kind != "receiver_ready" || s.receiver_protocol.len() > 32)
    {
        return Err(error("bridge.receiver_protocol_invalid"));
    }
    if s.kind != "receiver_ready" && (s.prior_session_sequence != 0 || s.prior_session_epoch != 0) {
        return Err(error("bridge.receiver_prior_session_invalid"));
    }
    match s.kind.as_str() {
        "receiver_ready"
        | "receiver_staged"
        | "receiver_commit_ready"
        | "receiver_rejected"
        | "receiver_accepted"
        | "receiver_cancelled"
        | "receiver_timeout" => return receiver(s),
        "identity" => return identity(s),
        "reset" => return reset(s),
        "ready" | "loaded" | "reported" | "report_error" | "acknowledged" | "cancelled"
        | "cleared" => (),
        _ => return Err(error("bridge.invalid_signal_kind")),
    }
    if bindings_present(s) {
        return Err(error("bridge.receiver_bindings_unexpected"));
    }
    if s.runtime_epoch != 0 && !matches!(s.kind.as_str(), "ready" | "cleared") {
        return Err(error("bridge.invalid_runtime_epoch"));
    }
    session(s)
}

fn session(s: &Signal) -> Result<()> {
    if receiver_fields(s) || (s.kind != "report_error" && !s.error_code.is_empty()) {
        return Err(error("bridge.unexpected_receiver_fields"));
    }
    if s.kind == "report_error"
        && (!key(&s.error_code, 64)
            || s.report_bytes != 0
            || !s.report_adler32.is_empty()
            || s.input_ready)
    {
        return Err(error("bridge.invalid_report_error_signal"));
    }
    if s.sequence == 0 || s.sequence > LUA_INTEGER_MAX {
        return Err(error("bridge.invalid_signal"));
    }
    if s.kind == "ready" && s.session_nonce.is_empty() {
        return Err(error("bridge.signal_missing_session"));
    }
    optional_label(&s.session_nonce)?;
    if s.release.is_empty() || s.product.is_empty() || s.build.is_empty() {
        return Err(error("bridge.invalid_signal"));
    }
    for value in [&s.release, &s.character, &s.realm, &s.product, &s.build] {
        optional_label(value)?;
    }
    if !s.probe_nonce.is_empty() || !s.actor_state.is_empty() || !s.input_reason.is_empty() {
        return Err(error("bridge.invalid_identity_signal"));
    }
    if s.kind != "ready" && s.request_id.is_empty() {
        return Err(error("bridge.signal_missing_request"));
    }
    if !s.guid.is_empty() && !label(&s.guid) {
        return Err(error("bridge.invalid_signal_guid"));
    }
    if s.kind == "cleared" {
        if !hex(&s.cleanup_nonce, 32)
            || s.guid.is_empty()
            || !s.reload_nonce.is_empty()
            || s.input_ready
            || digests_present(s)
        {
            return Err(error("bridge.invalid_cleanup_signal"));
        }
    } else if !s.cleanup_nonce.is_empty() {
        return Err(error("bridge.unexpected_cleanup_nonce"));
    }
    if (s.kind == "loaded" && s.code_bytes == 0) || (s.kind == "reported" && s.report_bytes == 0) {
        return Err(error("bridge.signal_missing_payload_identity"));
    }
    if s.code_bytes as usize > CODE_LIMIT || s.report_bytes as usize > REPORT_LIMIT {
        return Err(error("bridge.signal_payload_limit"));
    }
    if (s.code_bytes > 0 && !hex(&s.code_adler32, 8))
        || (s.report_bytes > 0 && !hex(&s.report_adler32, 8))
    {
        return Err(error("bridge.invalid_signal_checksum"));
    }
    Ok(())
}

fn receiver(s: &Signal) -> Result<()> {
    if s.release.is_empty()
        || s.product.is_empty()
        || s.build.is_empty()
        || s.sequence == 0
        || s.sequence > LUA_INTEGER_MAX - 1
        || s.runtime_epoch == 0
        || s.runtime_epoch > LUA_INTEGER_MAX - 1
        || !hex(&s.receiver_nonce, 32)
    {
        return Err(error("bridge.receiver_signal_identity"));
    }
    for value in [
        &s.release,
        &s.product,
        &s.build,
        &s.character,
        &s.realm,
        &s.guid,
    ] {
        optional_label(value)?;
    }
    if (!s.session_nonce.is_empty() && (s.kind != "receiver_ready" || !hex(&s.session_nonce, 32)))
        || !s.reload_nonce.is_empty()
        || !s.cleanup_nonce.is_empty()
        || !s.probe_nonce.is_empty()
        || !s.actor_state.is_empty()
        || !s.input_reason.is_empty()
        || digests_present(s)
    {
        return Err(error("bridge.receiver_signal_pollution"));
    }
    if (s.kind != "receiver_ready" && (s.prior_session_sequence != 0 || s.prior_session_epoch != 0))
        || s.prior_session_sequence > LUA_INTEGER_MAX
        || s.prior_session_epoch > LUA_INTEGER_MAX
    {
        return Err(error("bridge.receiver_prior_session_invalid"));
    }
    if !s.error_code.is_empty() && !key(&s.error_code, 64) {
        return Err(error("bridge.receiver_error_code"));
    }
    match s.kind.as_str() {
        "receiver_ready" => {
            if !s.input_ready
                || s.accepted
                || !s.request_id.is_empty()
                || !s.attempt_id.is_empty()
                || s.body_bytes != 0
                || !s.body_adler32.is_empty()
                || !s.commit_nonce.is_empty()
                || !s.error_code.is_empty()
            {
                return Err(error("bridge.receiver_ready_invalid"));
            }
            if (s.session_nonce.is_empty()
                && (s.prior_session_sequence != 0 || s.prior_session_epoch != 0))
                || (!s.session_nonce.is_empty()
                    && s.prior_session_epoch != 0
                    && s.prior_session_epoch != s.runtime_epoch)
            {
                return Err(error("bridge.receiver_prior_session_invalid"));
            }
            receiver_bindings(&s.wake_binding, &s.submit_binding, &s.close_binding)
                .map_err(|_| error("bridge.receiver_bindings_invalid"))?;
        }
        "receiver_staged" | "receiver_commit_ready" | "receiver_accepted" => {
            if bindings_present(s) {
                return Err(error("bridge.receiver_bindings_unexpected"));
            }
            if !key(&s.request_id, 80)
                || !hex(&s.attempt_id, 16)
                || s.body_bytes == 0
                || s.body_bytes > MAX_RECEIVER_WIRE_BYTES
                || !hex(&s.body_adler32, 8)
                || (s.kind != "receiver_accepted" && !s.error_code.is_empty())
            {
                return Err(error("bridge.receiver_stage_receipt_invalid"));
            }
            if s.kind == "receiver_commit_ready" {
                if !s.input_ready || s.accepted || !hex(&s.commit_nonce, 16) {
                    return Err(error("bridge.receiver_challenge_invalid"));
                }
            } else if !s.commit_nonce.is_empty() {
                return Err(error("bridge.receiver_challenge_unexpected"));
            }
            if s.kind == "receiver_staged" && (!s.input_ready || s.accepted) {
                return Err(error("bridge.receiver_staged_invalid"));
            }
            if s.kind == "receiver_accepted" && (s.input_ready || !s.accepted) {
                return Err(error("bridge.receiver_accepted_invalid"));
            }
        }
        "receiver_rejected" | "receiver_cancelled" | "receiver_timeout" => {
            if bindings_present(s) {
                return Err(error("bridge.receiver_bindings_unexpected"));
            }
            if s.input_ready || s.accepted || !s.commit_nonce.is_empty() || s.error_code.is_empty()
            {
                return Err(error("bridge.receiver_rejection_invalid"));
            }
            if !s.attempt_id.is_empty() && !hex(&s.attempt_id, 16) {
                return Err(error("bridge.receiver_attempt_invalid"));
            }
            if !s.body_adler32.is_empty() && !hex(&s.body_adler32, 8) {
                return Err(error("bridge.receiver_digest_invalid"));
            }
        }
        _ => return Err(error("bridge.receiver_signal_kind")),
    }
    Ok(())
}

fn reset(s: &Signal) -> Result<()> {
    if receiver_fields(s) || !s.error_code.is_empty() {
        return Err(error("bridge.unexpected_receiver_fields"));
    }
    if !hex(&s.probe_nonce, 32) {
        return Err(error("bridge.invalid_signal_probe_nonce"));
    }
    if !s.session_nonce.is_empty()
        || !s.request_id.is_empty()
        || !s.reload_nonce.is_empty()
        || !s.cleanup_nonce.is_empty()
        || s.sequence != 0
        || digests_present(s)
        || !s.actor_state.is_empty()
        || !s.input_reason.is_empty()
        || s.input_ready
    {
        return Err(error("bridge.invalid_reset_signal"));
    }
    if !label(&s.character) || !label(&s.realm) || !label(&s.guid) {
        return Err(error("bridge.invalid_reset_actor"));
    }
    Ok(())
}
fn identity(s: &Signal) -> Result<()> {
    if receiver_fields(s) || !s.error_code.is_empty() {
        return Err(error("bridge.unexpected_receiver_fields"));
    }
    if !hex(&s.probe_nonce, 32) {
        return Err(error("bridge.invalid_signal_probe_nonce"));
    }
    if !matches!(
        s.actor_state.as_str(),
        "ok" | "no_actor" | "actor_restricted"
    ) {
        return Err(error("bridge.invalid_signal_actor_state"));
    }
    if !s.session_nonce.is_empty()
        || !s.request_id.is_empty()
        || !s.reload_nonce.is_empty()
        || !s.cleanup_nonce.is_empty()
        || s.sequence != 0
        || digests_present(s)
    {
        return Err(error("bridge.invalid_identity_signal"));
    }
    if s.actor_state == "ok" {
        if !label(&s.character) || !label(&s.realm) {
            return Err(error("bridge.invalid_identity_actor"));
        }
    } else if !s.character.is_empty() || !s.realm.is_empty() || !s.guid.is_empty() {
        return Err(error("bridge.invalid_identity_actor"));
    }
    if !s.guid.is_empty() && !label(&s.guid) {
        return Err(error("bridge.invalid_signal_guid"));
    }
    if (s.input_ready && !s.input_reason.is_empty())
        || (!s.input_reason.is_empty()
            && !(s.input_reason.len() >= 7
                && s.input_reason.len() <= 64
                && s.input_reason.starts_with("input_")
                && s.input_reason
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')))
    {
        return Err(error("bridge.invalid_signal_input_reason"));
    }
    Ok(())
}
fn checkpoint(s: &Signal, control: &OperationControl) -> Result<()> {
    if !hex(&s.probe_nonce, 32)
        || !hex(&s.session_nonce, 32)
        || !hex(&s.code_sha256, 64)
        || !key(&s.request_id, 128)
        || s.guid.is_empty()
        || s.character.is_empty()
        || s.realm.is_empty()
        || s.runtime_epoch == 0
        || s.input_ready
        || s.receipt.len() > 1400
        || !s.report_scope.is_empty()
        || !s.receiver_protocol.is_empty()
        || s.prior_session_sequence != 0
        || s.prior_session_epoch != 0
        || bindings_present(s)
    {
        return Err(error("bridge.invalid_checkpoint"));
    }
    match s.work_state.as_str() {
        "loaded" | "running" => {
            if !s.receipt.is_empty() {
                return Err(error("bridge.invalid_checkpoint_receipt"));
            }
        }
        "reported" | "report_error" | "acknowledged" | "finished" => {
            if s.receipt.is_empty() {
                return Err(error("bridge.checkpoint_receipt_missing"));
            }
        }
        _ => return Err(error("bridge.invalid_checkpoint_state")),
    }
    // A bounded DTO clone matches Go's by-value plain view; never alters the
    // caller's checkpoint or turns it into readiness authority.
    let mut plain = s.clone();
    plain.kind = "ready".into();
    plain.probe_nonce.clear();
    session(&plain)?;
    if !s.receipt.is_empty() {
        let receipt = match parse_signal(s.receipt.as_bytes(), control) {
            Ok(receipt) if receipt.request_id == s.request_id => receipt,
            Ok(_) => return Err(error("bridge.checkpoint_receipt_mismatch")),
            Err(e) => {
                // Original cancellation/deadline always survives; syntax or
                // nested protocol failure maps to Go's checkpoint category.
                control.check()?;
                if e.code == "command.cancelled" || e.code == "command.deadline" {
                    return Err(e);
                }
                return Err(error("bridge.checkpoint_receipt_mismatch"));
            }
        };
        let want = if s.work_state == "finished" {
            "acknowledged"
        } else {
            &s.work_state
        };
        if receipt.kind != want {
            return Err(error("bridge.checkpoint_receipt_kind"));
        }
        for (inner, outer) in [
            (&receipt.session_nonce, &s.session_nonce),
            (&receipt.character, &s.character),
            (&receipt.realm, &s.realm),
            (&receipt.guid, &s.guid),
            (&receipt.release, &s.release),
            (&receipt.product, &s.product),
            (&receipt.build, &s.build),
        ] {
            if !inner.is_empty() && inner != outer {
                return Err(error("bridge.checkpoint_receipt_identity"));
            }
        }
    }
    Ok(())
}

fn binding_key(chord: &str) -> Option<&str> {
    let key = chord
        .strip_prefix("ALT-CTRL-SHIFT-")
        .or_else(|| chord.strip_prefix("ALT-CTRL-"))?;
    if matches!(
        key,
        "[" | "]"
            | "F1"
            | "F2"
            | "F3"
            | "F4"
            | "F5"
            | "F6"
            | "F7"
            | "F8"
            | "F9"
            | "F10"
            | "F11"
            | "F12"
    ) {
        Some(key)
    } else {
        None
    }
}
pub(super) fn receiver_bindings(wake: &str, submit: &str, close: &str) -> Result<()> {
    let chords = [wake, submit, close];
    let mut keys = [""; 3];
    for (i, chord) in chords.iter().enumerate() {
        keys[i] = binding_key(chord).ok_or_else(|| error("desktop.receiver_binding_invalid"))?;
        if chords[..i].contains(chord) {
            return Err(error("desktop.receiver_binding_duplicate"));
        }
    }
    if keys[2] == keys[0] || keys[2] == keys[1] {
        return Err(error("desktop.receiver_close_key_conflict"));
    }
    Ok(())
}
