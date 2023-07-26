#![deny(clippy::all)]
#![warn(clippy::pedantic, clippy::nursery)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Debug, Formatter, Write};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::Client;
use serde::{Serialize, Deserialize, Deserializer, Serializer};
use serde::de::{Error as _};
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

#[derive(Eq, PartialEq, Clone, Deserialize, Serialize)]
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
struct Args {
    #[clap(long)]
    before: Option<NoteId>,
    #[clap(long)]
    host: String,
    #[clap(long)]
    token: MisskeyAuthorizationToken,
    #[clap(long)]
    channel_id: ChannelId,
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
    async fn send(self, HTTP_CLIENT: &Client, host: String, misskey_token: &MisskeyAuthorizationToken) -> Result<Vec<Note>, Box<dyn Error + Send + Sync>> {
        let wtr = WithTokenRef {
            token: misskey_token,
            body: self,
        };
        eprintln!("{}", serde_json::to_string(&wtr).unwrap());
        let x = HTTP_CLIENT.post(format!("https://{host}/api/channels/timeline")).json(&wtr).build()?;

        let x = HTTP_CLIENT.execute(x).await?;

        let status = x.status();
        let text = x.text().await?;

        let json = match serde_json::from_str(&text) {
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
    text: MisskeyFlavoredMarkdown,
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
    // NOTE: Userのディスプレイネームはあとで取得する
    id: UserId,
}

#[derive(Eq, PartialEq, Hash)]
struct CanonicalEmojiKey {
    name: EmojiName,
    host: LocalOnly,
}

impl<'de> Deserialize<'de> for CanonicalEmojiKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        let pattern = regex_lite::Regex::new(r#"^:([a-z0-9_]+)@\.:$"#).expect("this pattern should be valid");

        let raw = String::deserialize(deserializer)?;
        let m = pattern.captures(&raw).ok_or(serde::de::Error::custom("should be match"))?;
        let name_range = m.get(1).expect("should be match").range();
        // TODO: おそらくこの再アロケーションは避けられる
        let name = EmojiName(raw[name_range].to_owned());

        Ok(CanonicalEmojiKey {
            name,
            host: LocalOnly,
        })
    }
}

impl Serialize for CanonicalEmojiKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: Serializer {
        let s = format!(":{name}@.:", name = &self.name.0);
        serializer.serialize_str(&s)
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

    let mut last_note = None;

    let client = Client::builder().gzip(true).deflate(true).brotli(true).build().expect("panic");
    loop {
        let send = ChannelTimelineCommand {
            channel_id: arg.channel_id.clone(),
            limit: 60.try_into().unwrap(),
            note_after: None,
            note_before: None,
            date_after: None,
            date_before: None,
        };

        let result = send.send(&client, arg.host.clone(), &arg.token).await?;

        if result.is_empty() {
            break
        }

        last_note = result.iter().min_by_key(|x| x.created_at).map(|x| x.id.clone());
        println!(r#"{{ "kind": "log", "message": "proceeded by {last_note}"}}"#, last_note = last_note.expect("must be Some").0);
        println!("{}", serde_json::to_string(&result)?);

        sleep(Duration::new(10, 0)).await;
    }

    Ok(())
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