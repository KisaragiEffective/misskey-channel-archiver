use crate::MisskeyAuthorizationToken;
use crate::{CanonicalEmojiKey, EmojiName, LocalOnly};

#[test]
fn do_not_leak_token_from_debug_impl() {
    const TOKEN: &str = "sometokenhere";
    let token = MisskeyAuthorizationToken(TOKEN.to_string());
    let debug_str = format!("{token:?}");

    assert!(!debug_str.contains(TOKEN));
}

#[test]
fn deserialize_custom_emoji_key() {
    let key: CanonicalEmojiKey = serde_json::from_str(r#"":sample_emoji@.:""#).unwrap();

    assert!(matches!(
            key,
            CanonicalEmojiKey::Custom {
                name: EmojiName(name),
                host: LocalOnly,
            } if name == "sample_emoji"
        ));
}

mod extract_capped_key_number_value {
    use crate::CanonicalEmojiKey;
    const UNICODE_FE0F: char = '\u{FE0F}';
    const UNICODE_20E3: char = '\u{20E3}';

    #[test]
    fn rejects_empty_string() {
        assert_eq!(CanonicalEmojiKey::extract_capped_key_number_value(""), None);
    }

    #[test]
    fn rejects_simple_ascii_digit() {
        for sub in b'0'..=b'9' {
            let bytes = &[sub];
            let s = std::str::from_utf8(bytes).expect("infallible");
            assert_eq!(CanonicalEmojiKey::extract_capped_key_number_value(s), None);
        }
    }

    #[test]
    fn accepts_capped_key_digit_two_codepoints() {
        for sub in '0'..='9' {
            let s = &[sub, UNICODE_20E3].into_iter().collect::<String>();
            assert_eq!(
                CanonicalEmojiKey::extract_capped_key_number_value(s),
                Some(sub as u8 - '0' as u8)
            );
        }
    }

    #[test]
    fn rejects_excess_characters_after_two_codepoints() {
        for sub in '0'..='9' {
            let s = &[sub, UNICODE_20E3, 'a', 'b', 'c'].into_iter().collect::<String>();
            assert_eq!(
                CanonicalEmojiKey::extract_capped_key_number_value(s),
                None
            );
        }
    }

    #[test]
    fn accepts_capped_key_digit_three_codepoints() {
        for sub in '0'..='9' {
            let s = &[sub, UNICODE_FE0F, UNICODE_20E3].into_iter().collect::<String>();
            assert_eq!(
                CanonicalEmojiKey::extract_capped_key_number_value(s),
                Some(sub as u8 - '0' as u8)
            );
        }
    }

    #[test]
    fn rejects_excess_characters_after_three_codepoints() {
        for sub in '0'..='9' {
            let s = &[sub, UNICODE_FE0F, UNICODE_20E3, 'a', 'b', 'c'].into_iter().collect::<String>();
            assert_eq!(
                CanonicalEmojiKey::extract_capped_key_number_value(s),
                None
            );
        }
    }
}
