//! The DSSE envelope a managed state travels in (ADR-0016 decision 2).
//!
//! ```json
//! { "payloadType": "application/vnd.fastskill.managed-state+json",
//!   "payload": "<base64 of the state's exact bytes>",
//!   "signatures": [ { "keyid": "<pinned key id>", "sig": "<base64 Ed25519 signature>" } ] }
//! ```
//!
//! Each signature covers the payload type and the payload's exact bytes through DSSE's
//! pre-authentication encoding ([`pae`]); no JSON is canonicalized. A state is accepted when
//! at least one signature names a pinned key and verifies against it. Signatures naming other
//! keys are skipped, so a source can sign with a new key before every machine pins it.

use super::config::PinnedKey;
use super::ManagedError;
use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;

/// The payload type of a managed state.
pub const PAYLOAD_TYPE: &str = "application/vnd.fastskill.managed-state+json";

/// The largest envelope FastSkill reads.
pub const MAX_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;

/// What a verified envelope carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The payload's exact bytes.
    pub payload: Vec<u8>,
    /// The pinned key whose signature verified.
    pub key_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    payload_type: String,
    payload: String,
    signatures: Vec<Signature>,
}

#[derive(Deserialize)]
struct Signature {
    #[serde(default)]
    keyid: String,
    sig: String,
}

/// DSSE's pre-authentication encoding: `DSSEv1 <len(type)> <type> <len(body)> <body>`, lengths
/// in ASCII decimal.
pub fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "DSSEv1 {} {payload_type} {} ",
        payload_type.len(),
        payload.len()
    )
    .into_bytes();
    out.extend_from_slice(payload);
    out
}

/// Standard or URL-safe base64, padded or not: DSSE leaves the choice to the signer.
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let config =
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
    GeneralPurpose::new(&alphabet::STANDARD, config)
        .decode(value)
        .or_else(|_| GeneralPurpose::new(&alphabet::URL_SAFE, config).decode(value))
        .ok()
}

/// Decode a base64 value that must decode, naming `what` when it doesn't.
pub(crate) fn decode_required(value: &str, what: &str) -> Result<Vec<u8>, String> {
    decode_base64(value.trim()).ok_or_else(|| format!("{what} is not valid base64"))
}

/// Check `envelope` against `keys` and return its payload.
pub fn verify_envelope(envelope: &[u8], keys: &[PinnedKey]) -> Result<Verified, ManagedError> {
    if envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(ManagedError::Envelope(format!(
            "it is larger than {MAX_ENVELOPE_BYTES} bytes"
        )));
    }
    let envelope: Envelope = serde_json::from_slice(envelope)
        .map_err(|error| ManagedError::Envelope(format!("not a DSSE envelope: {error}")))?;
    if envelope.payload_type != PAYLOAD_TYPE {
        return Err(ManagedError::Envelope(format!(
            "payload type is {:?}, expected {PAYLOAD_TYPE:?}",
            envelope.payload_type
        )));
    }
    let payload =
        decode_required(&envelope.payload, "the payload").map_err(ManagedError::Envelope)?;
    if keys.is_empty() {
        return Err(ManagedError::Untrusted("no keys are pinned".to_string()));
    }
    let message = pae(PAYLOAD_TYPE, &payload);
    let mut named_pinned = false;
    for signature in &envelope.signatures {
        let Some(key) = keys.iter().find(|key| key.id == signature.keyid) else {
            continue;
        };
        named_pinned = true;
        let Some(sig) = decode_base64(signature.sig.trim()) else {
            continue;
        };
        if UnparsedPublicKey::new(&ED25519, key.public_key)
            .verify(&message, &sig)
            .is_ok()
        {
            return Ok(Verified {
                payload,
                key_id: key.id.clone(),
            });
        }
    }
    let ids: Vec<&str> = keys.iter().map(|key| key.id.as_str()).collect();
    Err(ManagedError::Untrusted(if named_pinned {
        format!(
            "a signature names a pinned key ({}) but doesn't verify",
            ids.join(", ")
        )
    } else {
        format!(
            "no signature names a pinned key (pinned: {})",
            ids.join(", ")
        )
    }))
}
