use std::{cmp::Ordering, collections::VecDeque};

use serenity::{all::{Context, EventHandler, GatewayIntents, Message}, async_trait};
use serde::{Deserialize, Serialize};
use database::definitions;
use crossbeam::channel::{Sender, Receiver};

pub mod database;

#[derive(Debug, Deserialize, Serialize)]
struct Validation {
    user_id: Option<u64>,
    reason: Option<String>
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Role {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant
}

#[derive(Deserialize, Serialize)]
struct AIMessage {
    content: Option<String>,
    role: String,
}

impl AIMessage {
    pub fn new(content: Option<String>, role: String) -> Self {
        Self {
            content,
            role
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ResponseFormat {
    r#type: String
}

impl Default for ResponseFormat {
    fn default() -> Self {
        Self {
            r#type: "json_object".into()
        }
    }
}

#[derive(Deserialize, Serialize)]
struct FireworksPayload {
    model: String,
    messages: Vec<AIMessage>,
    response_format: Option<ResponseFormat>,
    max_tokens: u64,
    top_p: u8,
    top_k: u8,
    presence_penalty: u8,
    frequency_penalty: u8,
    temperature: f32,
}

impl Default for FireworksPayload {
    fn default() -> Self {
        let model = std::env::var("MODEL").unwrap();
        Self {
            model,
            messages: Vec::new(),
            response_format: Some(ResponseFormat::default()),
            max_tokens: 4096,
            top_p: 1,
            top_k: 40,
            presence_penalty: 0,
            frequency_penalty: 0,
            temperature: 0.6
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Choice {
    index: usize,
    message: AIMessage
}

#[derive(Deserialize, Serialize)]
struct AIResponse {
    choices: Vec<Choice>,
    created: i32,
    id: String,
    model: String,
    object: String,
}

#[derive(Clone, Debug, Eq, Deserialize, Serialize)]
pub struct PartialMessage {
    id: u64,
    channel_id: u64,
    author_id: u64,
    content: String,
    status: String,
    timestamp: i64
}

impl From<Message> for PartialMessage {
    fn from(message: Message) -> Self {
        Self {
            id: message.id.get(),
            channel_id: message.channel_id.get(),
            author_id: message.author.id.get(),
            content: message.content.clone(),
            status: "not_validated".into(),
            timestamp: message.timestamp.unix_timestamp()
        }
    }
}

impl PartialMessage {
    pub fn new(id: u64, channel_id: u64, author_id: u64, content: String, status: String, timestamp: i64) -> Self {
        Self {
            id,
            channel_id,
            author_id,
            content,
            status,
            timestamp
        }
    }
}

impl PartialEq for PartialMessage {
    fn eq(&self, other: &Self) -> bool {
        self.channel_id == other.channel_id && self.id == other.id
    }
    fn ne(&self, other: &Self) -> bool {
        self.channel_id != other.channel_id || self.id != other.id
    }
}

impl PartialOrd for PartialMessage {
    fn ge(&self, other: &Self) -> bool {
        self.timestamp >= other.timestamp
    }
    fn gt(&self, other: &Self) -> bool {
        self.timestamp > other.timestamp
    }
    fn le(&self, other: &Self) -> bool {
        self.timestamp <= other.timestamp
    }
    fn lt(&self, other: &Self) -> bool {
        self.timestamp < other.timestamp
    }
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.timestamp == other.timestamp {
            Ordering::Equal
        }else if self.timestamp < other.timestamp {
            Ordering::Less
        }else{
            Ordering::Greater
        }.into()
    }
}

impl Ord for PartialMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.timestamp == other.timestamp {
            Ordering::Equal
        }else if self.timestamp < other.timestamp {
            Ordering::Greater
        }else{
            Ordering::Less
        }
    }

    fn max(self, other: Self) -> Self
    where
        Self: Sized, {
        if self.timestamp < other.timestamp {
            return self;
        }
        other
    }

    fn min(self, other: Self) -> Self
    where
        Self: Sized, {
       if self.timestamp < other.timestamp {
            return other;
        }
        self
    }

    fn clamp(self, min: Self, max: Self) -> Self
    where
        Self: Sized, {
        if self.timestamp < min.timestamp {
            return min;
        }
        if self.timestamp > max.timestamp {
            return max;
        }
        self
    }
}

struct Handler {
    id: u64,
    database_connection: (Sender<definitions::DatabaseMessage>, Receiver<Vec<PartialMessage>>),
    web_client: reqwest::Client
}

impl Handler {
    async fn ai_request(&self, messages: Vec<PartialMessage>) {
        let channel_id = messages[0].channel_id;
        let api_key = std::env::var("FIREWORKS_API_KEY").unwrap();
        let system_prompt = std::env::var("SYSTEM_PROMPT").unwrap();
        let system_prompt = AIMessage::new(Some(system_prompt), "system".into());
        let mut ai_messages = messages.iter().map(|x| {
            let content = format!("AUTHOR: {}\nCONTENT: {}\nVALIDATION_STATUS: {}", x.author_id, x.content, x.status);
            AIMessage::new(Some(content), "user".into())
        }).collect::<VecDeque<AIMessage>>();
        ai_messages.push_front(system_prompt);
        let mut payload = FireworksPayload::default();
        payload.messages = ai_messages.into();
        let response = self.web_client.post("https://api.fireworks.ai/inference/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;
        if let Ok(response) = response {
            if response.status().as_u16() != 200 {
                panic!("{:?}", response);
            }
            let body : AIResponse = response.json().await.unwrap();
            let choice = &body.choices[0];
            let content = choice.message.content.clone().unwrap();
            let validation = serde_json::from_str::<Validation>(&content).unwrap();
            if let Some(reason) = validation.reason {
                let (sender, _) = &self.database_connection;
                let _ = sender.send(definitions::DatabaseMessage::ValidateEntries(channel_id));
                println!("{reason}");
            }else{
                println!("this message is fine");
            }
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        let id = std::env::var("APPLICATION_ID").unwrap().parse::<u64>().unwrap();
        let (sender, receiver) = definitions::Database::new();
        Self {
            id,
            database_connection: (sender, receiver),
            web_client: reqwest::Client::new()
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.id.get() == self.id {
            return;
        }

        let (sender, receiver) = &self.database_connection;
        let message = PartialMessage::from(msg);
        let _ = sender.send(definitions::DatabaseMessage::InsertMessage(message));
        let _ = sender.send(definitions::DatabaseMessage::GetLatest(20));
        let messages = receiver.recv().unwrap();
        self.ai_request(messages).await;
    }
}



fn start_env() {
    dotenvy::dotenv().unwrap();
}

#[tokio::main]
async fn main() {
    start_env();
    let token = std::env::var("API_KEY").unwrap();
    let mut client = serenity::Client::builder(token, GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT)
        .event_handler(Handler::default())
        .await
        .unwrap();
    client.start().await.unwrap();
}
