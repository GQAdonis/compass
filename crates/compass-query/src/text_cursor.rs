//! Compact, checksummed continuation tokens shared by the paged text surfaces.
//!
//! A cursor is re-printed at the end of every page, so its spelling is part of
//! the budget that page spends on its answer. The form is a short field list -
//! version first, then the caller's fields - closed by a truncated payload
//! digest:
//!
//! ```text
//! <version>~<field>~<field>.<32 hex chars>
//! ```

use sha2::{Digest, Sha256};

/// Field separator of the cursor wire form.
///
/// `~` is not special to a shell, and the fields only ever carry fixed
/// operation or section names, hex digest prefixes, and decimal integers, so
/// the form needs no escaping.
pub const CURSOR_FIELD_SEPARATOR: char = '~';

/// Hex characters of the payload checksum kept in a cursor.
///
/// The checksum proves the token was not altered on its way back to the tool. A
/// 128-bit prefix of the SHA-256 digest is far beyond the collision a caller
/// could reach by accident, and the token closes every page, so the full
/// 64-character digest only spends page budget.
pub const CURSOR_CHECKSUM_CHARS: usize = 32;

/// Longest field a caller may store.
const MAX_CURSOR_FIELD_CHARS: usize = 128;

/// Why one token is not a readable cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorTokenError {
    /// The token carries no checksum field.
    MissingChecksum,
    /// The checksum is not this form's checksum, or does not cover the payload.
    ChecksumMismatch,
    /// The token is intact but was written by another wire version.
    UnsupportedVersion,
    /// The payload is not a field list this form could have written.
    Malformed,
}

/// Encode `fields` behind one wire version and the payload checksum.
pub fn encode_cursor_token(version: u8, fields: &[&str]) -> String {
    let mut payload = version.to_string();
    for field in fields {
        payload.push(CURSOR_FIELD_SEPARATOR);
        payload.push_str(field);
    }
    let checksum = hex_digest(payload.as_bytes());
    format!(
        "{payload}.{}",
        checksum
            .chars()
            .take(CURSOR_CHECKSUM_CHARS)
            .collect::<String>()
    )
}

/// Split a token into its fields without re-interpreting them.
///
/// The caller owns every field's meaning - a cursor names its own section, page
/// and digest prefixes - so this only proves that the token is intact, readable
/// as this wire form, and carries `field_count` fields.
pub fn decode_cursor_token(
    token: &str,
    version: u8,
    field_count: usize,
) -> Result<Vec<String>, CursorTokenError> {
    let (payload, checksum) = token
        .split_once('.')
        .ok_or(CursorTokenError::MissingChecksum)?;
    if checksum.len() != CURSOR_CHECKSUM_CHARS
        || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CursorTokenError::ChecksumMismatch);
    }
    if !hex_digest(payload.as_bytes()).starts_with(checksum) {
        return Err(CursorTokenError::ChecksumMismatch);
    }
    let mut fields = payload.split(CURSOR_FIELD_SEPARATOR);
    let token_version = fields
        .next()
        .and_then(|value| value.parse::<u8>().ok())
        .ok_or(CursorTokenError::Malformed)?;
    if token_version != version {
        return Err(CursorTokenError::UnsupportedVersion);
    }
    let fields = fields.collect::<Vec<_>>();
    if fields.len() != field_count || !fields.iter().all(|field| is_cursor_field(field)) {
        return Err(CursorTokenError::Malformed);
    }
    Ok(fields.into_iter().map(str::to_owned).collect())
}

/// Whether a field could have been written by this encoder.
fn is_cursor_field(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CURSOR_FIELD_CHARS
        && value.chars().all(|character| character.is_ascii_graphic())
}

/// Whether a field carries a hex digest prefix.
pub fn is_cursor_digest(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_chars
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_and_stay_short() {
        let token = encode_cursor_token(2, &["callers", "0123456789abcdef", "2"]);
        assert!(
            token.len() <= 64,
            "a cursor closes every page, so it stays short: {token}"
        );
        assert_eq!(
            decode_cursor_token(&token, 2, 3),
            Ok(vec![
                "callers".to_owned(),
                "0123456789abcdef".to_owned(),
                "2".to_owned()
            ])
        );
    }

    #[test]
    fn a_changed_token_fails_closed() {
        let token = encode_cursor_token(2, &["callers", "0123456789abcdef", "2"]);
        let tampered = format!("{}x", &token[..token.len() - 1]);
        assert_eq!(
            decode_cursor_token(&tampered, 2, 3),
            Err(CursorTokenError::ChecksumMismatch)
        );
        let truncated = token.split_once('.').map(|(payload, _)| payload.to_owned());
        assert_eq!(
            truncated
                .as_deref()
                .map(|value| decode_cursor_token(value, 2, 3)),
            Some(Err(CursorTokenError::MissingChecksum))
        );
    }

    #[test]
    fn another_version_and_field_count_are_named() {
        let token = encode_cursor_token(1, &["callers", "0123456789abcdef", "2"]);
        assert_eq!(
            decode_cursor_token(&token, 2, 3),
            Err(CursorTokenError::UnsupportedVersion)
        );
        assert_eq!(
            decode_cursor_token(&token, 1, 2),
            Err(CursorTokenError::Malformed)
        );
    }
}
