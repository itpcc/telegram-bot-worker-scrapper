use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGUser {
    pub id: u64,
    pub is_bot: bool,
    pub first_name: String,
    pub last_name: Option<String>,
    pub username: String,
    pub language_code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGChat {
    pub id: u64,
    pub username: String,
    pub first_name: String,
    #[serde(rename = "type")]
    pub chat_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGMessageInfo {
    pub message_id: u64,
    pub from: TGUser,
    pub chat: TGChat,
    pub date: u64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGMessgae {
    pub update_id: u64,
    pub message: TGMessageInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TGDekaNumber {
    pub deka_serial: String,
    pub deka_year: u32,
    pub with_long_note: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TGDekaSearch {
    pub search_words: Vec<String>,
    pub search_law: Option<String>,
    pub search_law_no: Option<String>,
    pub case_from: Option<u32>,
    pub case_to: Option<u32>,
    pub with_long_note: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum TGDeka {
    Number(TGDekaNumber),
    Search(TGDekaSearch),
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct MessagePayload {
    pub message: TGMessgae,
    pub info: TGDeka,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct DekaMetadata {
    pub law: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct DekaInfo {
    pub deka_no: String,
    pub short_note: String,
    pub long_note: Option<String>,
    pub metadata: DekaMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGResponseOkay {
    pub from: String,
    pub message: TGMessgae,
    pub result: Option<Vec<DekaInfo>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct TGResponseErr {
    pub from: String,
    pub message: TGMessgae,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum TGResponse {
    Okay(TGResponseOkay),
    Err(TGResponseErr),
}
