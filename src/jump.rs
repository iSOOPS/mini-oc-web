use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// Build a deep-link URL of the form `{base_url}/{b64dir}/session/{sid}`.
pub fn build_jump_url(base_url: &str, directory: &str, session_id: &str) -> String {
    let encoded_dir = utf8_percent_encode(directory, FRAGMENT);
    let b64 = URL_SAFE_NO_PAD.encode(encoded_dir.to_string().as_bytes());
    format!("{}/{}/session/{}", base_url.trim_end_matches('/'), b64, session_id)
}

/// URL-safe base64 (no padding) encoding of the device pcname. Used as the
/// first URL segment on `oc.isoops.com/{b64pc}/...` so nginx can dispatch to
/// the matching rathole upstream.
#[must_use]
pub fn pcname_b64(pcname: &str) -> String {
    URL_SAFE_NO_PAD.encode(pcname.as_bytes())
}

/// Append opencode serve's `auth_token` query parameter — base64 of
/// `user:password` — to a jump URL so the browser navigation to the device
/// web UI is pre-authorized (no native Basic Auth popup).
///
/// The device server checks `?auth_token=` before the `Authorization`
/// header (opencode `credentialFromRequest`, verified against 1.18.30 in
/// the field). The token is standard base64, percent-encoded because
/// `+`/`/`/`=` are unsafe in a query value (form-decoding would corrupt
/// the token). Empty username AND password append nothing — used by the
/// credential-free health probe path.
#[must_use]
pub fn append_auth_token(url: &str, username: &str, password: &str) -> String {
    use base64::engine::general_purpose::STANDARD;
    if username.is_empty() && password.is_empty() {
        return url.to_string();
    }
    let token = STANDARD.encode(format!("{}:{}", username, password));
    let sep = if url.contains('?') { '&' } else { '?' };
    format!(
        "{}{}auth_token={}",
        url,
        sep,
        urlencoding::encode(&token)
    )
}
