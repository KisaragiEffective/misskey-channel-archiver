#![deny(clippy::all)]
#![warn(clippy::pedantic, clippy::nursery)]
#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Debug, Formatter, Write};
use std::num::NonZeroUsize;
use std::str::FromStr;

use std::time::Duration;
use chrono::{DateTime, Utc};
use clap::Parser;
use lazy_regex::Lazy;

use reqwest::{Client, Method, Request, RequestBuilder};
use url::Url;
use serde::{Serialize, Deserialize, Deserializer, Serializer};

use tokio::time::sleep;

#[derive(Eq, PartialEq, Clone, Serialize, Deserialize)]
struct NoteId(String);

impl FromStr for NoteId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

#[derive(Eq, PartialEq, Clone, Deserialize, Serialize)]
struct ChannelId(String);

impl FromStr for ChannelId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

#[derive(Eq, PartialEq, Clone, Hash, Deserialize, Serialize)]
struct UserId(String);

impl FromStr for UserId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}
#[derive(Eq, PartialEq, Clone, Serialize)]
struct MisskeyAuthorizationToken(String);

impl MisskeyAuthorizationToken {
    fn leak(self) -> String {
        self.0
    }
}

impl Debug for MisskeyAuthorizationToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MisskeyAuthorizationToken").field("value", &"*****").finish()
    }
}

impl FromStr for MisskeyAuthorizationToken {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

#[derive(Eq, PartialEq, Parser)]
enum Args {
    Archive {
        #[clap(long)]
        /// どこから遡るか。ない場合は実行時点の最新のノートから。
        before: Option<NoteId>,
        #[clap(long)]
        /// どこまで遡るか。ない場合は実行時点の最古のノートまで。
        after: Option<NoteId>,
        #[clap(long)]
        host: String,
        #[clap(long)]
        token: MisskeyAuthorizationToken,
        #[clap(long)]
        channel_id: ChannelId,
        #[clap(long, long = "cool-down")]
        /// リクエストの間隔をミリ秒で指定。
        cool_down_millisecond: Option<NonZeroUsize>,
    },
    FetchUser {
        #[clap(long)]
        user: Vec<UserId>,
        #[clap(long)]
        host: String,
        #[clap(long)]
        token: MisskeyAuthorizationToken,
        #[clap(long, long = "cool-down")]
        /// リクエストの間隔をミリ秒で指定。
        cool_down_millisecond: Option<NonZeroUsize>,
    },
}

#[derive(Serialize)]
struct ChannelTimelineCommand {
    #[serde(rename = "channelId")]
    channel_id: ChannelId,
    limit: NonZeroUsize,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sinceId")]
    note_after: Option<NoteId>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "untilId")]
    note_before: Option<NoteId>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sinceDate")]
    date_after: Option<UnixDateTime>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "untilDate")]
    date_before: Option<UnixDateTime>,
}

#[derive(Serialize)]
struct WithTokenRef<'a, T> {
    #[serde(rename = "i")]
    token: &'a MisskeyAuthorizationToken,
    #[serde(flatten)]
    body: T,
}

impl ChannelTimelineCommand {
    async fn send(self, http_client: &Client, host: String, misskey_token: &MisskeyAuthorizationToken) -> Result<Vec<Note>, Box<dyn Error + Send + Sync>> {
        let wtr = WithTokenRef {
            token: misskey_token,
            body: self,
        };
        eprintln!("{}", serde_json::to_string(&wtr).unwrap());
        let x = http_client.request(Method::POST, format!("https://{host}/api/channels/timeline"))
            .json(&wtr)
            .send()
            .await?;
        let status = x.status();
        let text = x.text().await?;

        let json = match serde_path_to_error::deserialize(&mut serde_json::de::Deserializer::from_str(&text)) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("ERROR: deserialize failed.");
                eprintln!("raw: {text}", text = text);
                eprintln!("status: {status}");
                panic!("{e:?}");
            }
        };
        Ok(json)
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Debug, Serialize)]
struct UnixDateTime(u32);

#[derive(Deserialize, Serialize)]
struct Note {
    id: NoteId,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    user: PartialUser,
    /// 本文。RNなら[`None`]。QRNなら引用先の文。
    text: Option<MisskeyFlavoredMarkdown>,
    /// CWの折りたたみ時に表示されるテキスト
    #[serde(rename = "cw")]
    spoiler_disclaimer_text: Option<String>,
    // visibility
    #[serde(rename = "replyId")]
    reply_to: Option<NoteId>,
    #[serde(rename = "renoteId")]
    renote_on: Option<NoteId>,
    #[serde(rename = "renoteCount")]
    renote_count: usize,
    #[serde(rename = "repliesCount")]
    reply_count: usize,
    reactions: HashMap<CanonicalEmojiKey, NonZeroUsize>,
}

#[derive(Deserialize, Serialize)]
struct PartialUser {
    // NOTE: その他のプロパティを捨てているのは下流側の正規化が面倒になるため
    id: UserId,
}

#[derive(Eq, PartialEq, Hash)]
enum CanonicalEmojiKey {
    SingleCodepointPunctuation(char),
    BoxedSingleDigit {
        digit: u8,
    },
    Unicode {
        utf8: String,
    },
    Custom {
        name: EmojiName,
        host: LocalOnly,
    },
    Uncategorized(String),
}

impl<'de> Deserialize<'de> for CanonicalEmojiKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let raw = String::deserialize(deserializer)?;
        // ヒント: もしこれがエラーに見えているならIntelliJがおかしい
        static PAT: Lazy<lazy_regex::Regex> = lazy_regex::lazy_regex!(r#"^:([a-z0-9_-]+)@\.:$"#);

        if let Some(captures) = PAT.captures(&raw) {
            let m = captures;
            let name_range = m.get(1).expect("should be match").range();
            // TODO: おそらくこの再アロケーションは避けられる
            let name = EmojiName(raw[name_range].to_owned());

            Ok(Self::Custom {
                name,
                host: LocalOnly,
            })
        } else if let Some(emoji) = emojis::iter().find(|x| x.as_str() == &raw) {
            // 絵文字は単にUnicodeの「文字」であることもある
            Ok(Self::Unicode {
                utf8: emoji.to_string()
            })
        } else if raw.chars().next().expect("must not be empty").is_ascii_digit() && raw.chars().nth(1).expect("ow") == '\u{20e3}' {
            Ok(Self::BoxedSingleDigit {
                // Unicodeでは0-9は一列に並んでいるのでオフセットは引き算するだけで求められる
                digit: u8::try_from(raw.chars().next().expect("1") as u32 - '0' as u32).expect("oops"),
            })
        } else {
            Ok(Self::Uncategorized(raw))
        }
    }
}

impl Serialize for CanonicalEmojiKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        match self {
            Self::Unicode { utf8 } => {
                serializer.serialize_str(utf8)
            }
            Self::Custom { name, .. } => {
                let s = format!(":{}@.:", name.0);
                serializer.serialize_str(&s)
            }
            Self::SingleCodepointPunctuation(c) => {
                serializer.serialize_char(*c)
            }
            Self::BoxedSingleDigit { digit } => {
                serializer.serialize_str(&format!("{digit}\u{20e3}"))
            }
            Self::Uncategorized(s) => {
                serializer.serialize_str(&s)
            }
        }
    }
}

#[derive(Eq, PartialEq, Hash)]
struct EmojiName(String);

#[derive(Eq, PartialEq, Hash)]
struct LocalOnly;

#[derive(Deserialize, Serialize)]
struct MisskeyFlavoredMarkdown(String);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>>{
    let arg = Args::parse();
    let client = Client::builder().gzip(true).deflate(true).brotli(true)
        .use_rustls_tls()
        .build()
        .expect("panic");

    match arg {
        Args::Archive { before, after, host, token, channel_id, cool_down_millisecond } => {
            let mut last_note = None;

            let mut users = HashSet::with_capacity(100);

            loop {
                let send = ChannelTimelineCommand {
                    channel_id: channel_id.clone(),
                    limit: 60.try_into().unwrap(),
                    note_after: after.clone(),
                    note_before: last_note.clone(),
                    date_after: None,
                    date_before: None,
                };

                let result = send.send(&client, host.clone(), &token).await?;

                if result.is_empty() {
                    break
                }

                last_note = result.iter().min_by_key(|x| x.created_at).map(|x| x.id.clone());
                println!(r#"{{ "kind": "log", "message": "proceeded by {last_note}"}}"#, last_note = last_note.clone().expect("must be Some").0);
                println!("{}", serde_json::to_string(&result)?);
                users.extend(result.into_iter().map(|n| n.user.id));

                let sleep_sec = cool_down_millisecond.map(|x| x.get() / 1000).unwrap_or(0) as u64;
                let sleep_nano = cool_down_millisecond.map(|x| x.get() as u64 - sleep_sec * 1000).unwrap_or(0) as u32 * 1_000_000;
                println!(r#"{{ "kind": "log", "message": "sleep" }}"#);
                sleep(Duration::new(sleep_sec, sleep_nano)).await;
            }
        }
        Args::FetchUser { user, host, token, cool_down_millisecond } => {
            let users = user;
            let mut user_info = Vec::with_capacity(users.len());

            for user_id in users {
                let command = UserDetailCommand {
                    id: user_id
                };

                let result = command.send(&client, host.clone(), &token).await?;

                println!("{}", serde_json::to_string(&result)?);

                user_info.push(result);
                let sleep_sec = cool_down_millisecond.map(|x| x.get() / 1000).unwrap_or(0) as u64;
                let sleep_nano = cool_down_millisecond.map(|x| x.get() as u64 - sleep_sec * 1000).unwrap_or(0) as u32 * 1_000_000;
                println!(r#"{{ "kind": "log", "message": "sleep" }}"#);
                sleep(Duration::new(sleep_sec, sleep_nano)).await;
            }
        }
    }

    Ok(())
}

#[derive(Eq, PartialEq, Serialize)]
struct UserDetailCommand {
    #[serde(rename = "userId")]
    id: UserId,
}

#[derive(Serialize, Deserialize)]
struct DetailedUser {
    id: UserId,
    #[serde(rename = "name")]
    /// スクリーンネームを設定していない場合は[`None`]。その場合、見える文字列はmentionであるべき。
    screen_name: Option<String>,
    #[serde(rename = "username")]
    mention: String,
    #[serde(rename = "isBot")]
    is_bot: bool,
    #[serde(rename = "isCat")]
    is_cat: bool,
    #[serde(rename = "avatarUrl")]
    /// 現在のアイコンのURL
    icon_url: Url,
}

impl UserDetailCommand {
    async fn send(self, http_client: &Client, host: String, misskey_token: &MisskeyAuthorizationToken) -> Result<DetailedUser, Box<dyn Error + Send + Sync>> {
        let wtr = WithTokenRef {
            token: misskey_token,
            body: self,
        };
        eprintln!("{}", serde_json::to_string(&wtr).unwrap());
        let x = http_client.request(Method::POST, format!("https://{host}/api/users/show"))
            .json(&wtr)
            .send()
            .await?;
        let status = x.status();
        let text = x.text().await?;

        let json = match serde_path_to_error::deserialize(&mut serde_json::de::Deserializer::from_str(&text)) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("ERROR: deserialize failed.");
                eprintln!("raw: {text}", text = text);
                eprintln!("status: {status}");
                panic!("{e:?}");
            }
        };
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use crate::MisskeyAuthorizationToken;

    #[test]
    fn do_not_leak_token_from_debug_impl() {
        const TOKEN: &str = "sometokenhere";
        let token = MisskeyAuthorizationToken(TOKEN.to_string());
        let debug_str = format!("{token:?}");

        assert!(!debug_str.contains(TOKEN));
    }
}