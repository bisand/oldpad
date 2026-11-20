//! Passkey (WebAuthn level 2) verification for one algorithm: ES256 on P-256,
//! which is what Apple, Google and Microsoft passkeys use. Attestation is not
//! checked — we ask for "none" and trust the user's own device. The parsing is
//! ours; the signature check is the `p256` crate.

use crate::cbor::{self, Cbor};
use crate::util::{b64url_decode, b64url_encode, sha256};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde_json::{json, Value};

const FLAG_UP: u8 = 0x01; // user present
const FLAG_UV: u8 = 0x04; // user verified (Face ID, PIN, …)
const FLAG_AT: u8 = 0x40; // attested credential data included

pub struct Rp<'a> {
    pub id: &'a str,     // hostname, e.g. "dash.example.com"
    pub origin: &'a str, // e.g. "https://dash.example.com"
}

pub struct NewCredential {
    pub id: String,
    pub public_key: String,
    pub counter: u32,
}

pub fn registration_options(rp: &Rp, challenge: &str, user_handle: &str, user_name: &str, exclude: &[String]) -> Value {
    json!({
        "challenge": challenge,
        "rp": { "name": "oldpad", "id": rp.id },
        "user": { "id": user_handle, "name": user_name, "displayName": user_name },
        "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }],
        "authenticatorSelection": { "residentKey": "required", "requireResidentKey": true, "userVerification": "required" },
        "attestation": "none",
        "timeout": 120000,
        "excludeCredentials": exclude.iter().map(|id| json!({ "type": "public-key", "id": id })).collect::<Vec<_>>(),
    })
}

pub fn authentication_options(rp: &Rp, challenge: &str) -> Value {
    // No allowCredentials: the phone offers whichever passkey it has for this site.
    json!({ "challenge": challenge, "rpId": rp.id, "userVerification": "required", "timeout": 120000 })
}

fn field<'a>(v: &'a Value, path: &[&str]) -> Result<&'a str, String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p).ok_or_else(|| format!("missing {}", path.join(".")))?;
    }
    cur.as_str().ok_or_else(|| format!("{} is not a string", path.join(".")))
}

fn decode_field(v: &Value, path: &[&str]) -> Result<Vec<u8>, String> {
    b64url_decode(field(v, path)?).ok_or_else(|| format!("{} is not base64url", path.join(".")))
}

fn check_client_data(raw: &[u8], kind: &str, rp: &Rp, challenge: &str) -> Result<(), String> {
    let cd: Value = serde_json::from_slice(raw).map_err(|_| "clientDataJSON is not JSON")?;
    if cd["type"] != kind {
        return Err(format!("expected {kind}"));
    }
    if cd["challenge"].as_str() != Some(challenge) {
        return Err("challenge mismatch".into());
    }
    if cd["origin"].as_str() != Some(rp.origin) {
        return Err(format!("origin {} is not {}", cd["origin"], rp.origin));
    }
    Ok(())
}

fn check_auth_data(auth: &[u8], rp: &Rp) -> Result<(u8, u32), String> {
    if auth.len() < 37 {
        return Err("authenticatorData too short".into());
    }
    if auth[..32] != sha256(rp.id.as_bytes()) {
        return Err("authenticator signed for another site".into());
    }
    let flags = auth[32];
    if flags & FLAG_UP == 0 || flags & FLAG_UV == 0 {
        return Err("user presence and verification are required".into());
    }
    Ok((flags, u32::from_be_bytes(auth[33..37].try_into().unwrap())))
}

/// Converts a COSE EC2 P-256 key to an uncompressed SEC1 point.
fn cose_to_sec1(key: &Cbor) -> Result<Vec<u8>, String> {
    let int = |k| key.get_int(k).and_then(Cbor::as_int);
    if int(1) != Some(2) || int(3) != Some(-7) || int(-1) != Some(1) {
        return Err("only ES256 (P-256) passkeys are supported".into());
    }
    let x = key.get_int(-2).and_then(Cbor::as_bytes).ok_or("missing x")?;
    let y = key.get_int(-3).and_then(Cbor::as_bytes).ok_or("missing y")?;
    if x.len() != 32 || y.len() != 32 {
        return Err("bad coordinate length".into());
    }
    let mut point = vec![0x04];
    point.extend(x);
    point.extend(y);
    VerifyingKey::from_sec1_bytes(&point).map_err(|_| "public key is not on the curve")?;
    Ok(point)
}

/// Verifies a navigator.credentials.create() result (JSON with base64url fields).
pub fn verify_registration(rp: &Rp, challenge: &str, cred: &Value) -> Result<NewCredential, String> {
    check_client_data(&decode_field(cred, &["response", "clientDataJSON"])?, "webauthn.create", rp, challenge)?;

    let att_bytes = decode_field(cred, &["response", "attestationObject"])?;
    let (att, _) = cbor::decode(&att_bytes)?;
    let auth = att.get_text("authData").and_then(Cbor::as_bytes).ok_or("missing authData")?;
    let (flags, counter) = check_auth_data(auth, rp)?;
    if flags & FLAG_AT == 0 {
        return Err("no credential in authenticatorData".into());
    }

    // aaguid(16) | credIdLen(2) | credId | COSE key
    let rest = auth.get(37..).ok_or("truncated")?;
    let id_len = u16::from_be_bytes(rest.get(16..18).ok_or("truncated")?.try_into().unwrap()) as usize;
    let cred_id = rest.get(18..18 + id_len).ok_or("truncated credential id")?;
    let (key, _) = cbor::decode(&rest[18 + id_len..])?;

    let id = b64url_encode(cred_id);
    if b64url_decode(field(cred, &["rawId"])?).as_deref() != Some(cred_id) {
        return Err("rawId does not match the credential".into());
    }
    Ok(NewCredential { id, public_key: b64url_encode(&cose_to_sec1(&key)?), counter })
}

/// Verifies a navigator.credentials.get() result against a stored public key.
/// Returns the new signature counter.
pub fn verify_authentication(rp: &Rp, challenge: &str, cred: &Value, public_key: &str, stored_counter: u32) -> Result<u32, String> {
    let client_data = decode_field(cred, &["response", "clientDataJSON"])?;
    check_client_data(&client_data, "webauthn.get", rp, challenge)?;
    let auth = decode_field(cred, &["response", "authenticatorData"])?;
    let (_, counter) = check_auth_data(&auth, rp)?;

    let key = VerifyingKey::from_sec1_bytes(&b64url_decode(public_key).ok_or("stored key corrupt")?)
        .map_err(|_| "stored key corrupt")?;
    let sig = Signature::from_der(&decode_field(cred, &["response", "signature"])?).map_err(|_| "bad signature encoding")?;
    let mut signed = auth.clone();
    signed.extend(sha256(&client_data));
    key.verify(&signed, &sig).map_err(|_| "signature does not verify")?;

    // Synced passkeys (Apple, Google) always report 0. A counter that goes
    // backwards on a key that does count means a cloned authenticator.
    if (counter != 0 || stored_counter != 0) && counter <= stored_counter {
        return Err("signature counter went backwards".into());
    }
    Ok(counter)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::cbor::tests::encode;
    use p256::ecdsa::{signature::Signer, SigningKey};

    const RP: Rp = Rp { id: "dash.example.com", origin: "https://dash.example.com" };

    pub fn client_data(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
        json!({ "type": kind, "challenge": challenge, "origin": origin }).to_string().into_bytes()
    }

    pub fn auth_data_for(rp: &Rp, flags: u8, counter: u32, extra: &[u8]) -> Vec<u8> {
        let mut a = sha256(rp.id.as_bytes()).to_vec();
        a.push(flags);
        a.extend(counter.to_be_bytes());
        a.extend(extra);
        a
    }

    fn register(sk: &SigningKey, challenge: &str) -> Value {
        register_for(&RP, sk, challenge)
    }

    /// What a browser sends back from navigator.credentials.create().
    pub fn register_for(rp: &Rp, sk: &SigningKey, challenge: &str) -> Value {
        let point = sk.verifying_key().to_encoded_point(false);
        let cose = encode(&Cbor::Map(vec![
            (Cbor::Int(1), Cbor::Int(2)),
            (Cbor::Int(3), Cbor::Int(-7)),
            (Cbor::Int(-1), Cbor::Int(1)),
            (Cbor::Int(-2), Cbor::Bytes(point.x().unwrap().to_vec())),
            (Cbor::Int(-3), Cbor::Bytes(point.y().unwrap().to_vec())),
        ]));
        let cred_id = b"credential-1";
        let mut attested = vec![0u8; 16];
        attested.extend((cred_id.len() as u16).to_be_bytes());
        attested.extend(cred_id);
        attested.extend(cose);
        let att = encode(&Cbor::Map(vec![
            (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
            (Cbor::Text("attStmt".into()), Cbor::Map(vec![])),
            (Cbor::Text("authData".into()), Cbor::Bytes(auth_data_for(rp, FLAG_UP | FLAG_UV | FLAG_AT, 0, &attested))),
        ]));
        json!({
            "rawId": b64url_encode(cred_id),
            "response": {
                "clientDataJSON": b64url_encode(&client_data("webauthn.create", challenge, rp.origin)),
                "attestationObject": b64url_encode(&att),
            }
        })
    }

    fn assertion(sk: &SigningKey, challenge: &str, origin: &str, flags: u8, counter: u32) -> Value {
        assertion_for(&RP, sk, challenge, origin, flags, counter)
    }

    /// What a browser sends back from navigator.credentials.get().
    pub fn assertion_for(rp: &Rp, sk: &SigningKey, challenge: &str, origin: &str, flags: u8, counter: u32) -> Value {
        let cd = client_data("webauthn.get", challenge, origin);
        let ad = auth_data_for(rp, flags, counter, &[]);
        let mut signed = ad.clone();
        signed.extend(sha256(&cd));
        let sig: Signature = sk.sign(&signed);
        json!({ "id": b64url_encode(b"credential-1"), "response": {
            "clientDataJSON": b64url_encode(&cd),
            "authenticatorData": b64url_encode(&ad),
            "signature": b64url_encode(sig.to_der().as_bytes()),
        }})
    }

    #[test]
    fn register_then_sign_in() {
        let sk = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let cred = verify_registration(&RP, "chal1", &register(&sk, "chal1")).unwrap();
        assert_eq!(cred.id, b64url_encode(b"credential-1"));

        let ok = assertion(&sk, "chal2", RP.origin, FLAG_UP | FLAG_UV, 0);
        assert_eq!(verify_authentication(&RP, "chal2", &ok, &cred.public_key, 0), Ok(0));
    }

    #[test]
    fn rejects_bad_assertions() {
        let sk = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let other = SigningKey::from_slice(&[9u8; 32]).unwrap();
        let cred = verify_registration(&RP, "c", &register(&sk, "c")).unwrap();
        let uv = FLAG_UP | FLAG_UV;

        let wrong_challenge = assertion(&sk, "old", RP.origin, uv, 0);
        assert!(verify_authentication(&RP, "new", &wrong_challenge, &cred.public_key, 0).is_err());
        let phishing = assertion(&sk, "c2", "https://evil.example", uv, 0);
        assert!(verify_authentication(&RP, "c2", &phishing, &cred.public_key, 0).is_err());
        let no_uv = assertion(&sk, "c3", RP.origin, FLAG_UP, 0);
        assert!(verify_authentication(&RP, "c3", &no_uv, &cred.public_key, 0).is_err());
        let wrong_key = assertion(&other, "c4", RP.origin, uv, 0);
        assert!(verify_authentication(&RP, "c4", &wrong_key, &cred.public_key, 0).is_err());
        let replayed_counter = assertion(&sk, "c5", RP.origin, uv, 5);
        assert!(verify_authentication(&RP, "c5", &replayed_counter, &cred.public_key, 5).is_err());
        assert_eq!(verify_authentication(&RP, "c5", &replayed_counter, &cred.public_key, 4), Ok(5));
    }

    #[test]
    fn rejects_registration_for_other_challenge() {
        let sk = SigningKey::from_slice(&[7u8; 32]).unwrap();
        assert!(verify_registration(&RP, "expected", &register(&sk, "other")).is_err());
    }
}
