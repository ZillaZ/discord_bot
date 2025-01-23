use serde::{Deserialize, Serialize};
use serde_json::Value;
use websocket::ws::dataframe::DataFrame;
use websocket::OwnedMessage;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::{thread, time::Duration};
use rand::Rng;

#[derive(Debug, Deserialize, Serialize)]
struct Identify {
    token: String,
    properties: IdentifyConnectionProperties,
    compress: Option<bool>,
    large_threshold: Option<i32>,
    shard: Option<[i32; 2]>,
    presence: Option<GatewayPresenceUpdate>,
    intents: i32
}

impl Default for Identify {
    fn default() -> Self {
        Self {
            token: "".into(),
            properties: IdentifyConnectionProperties::default(),
            compress: Some(false),
            large_threshold: Some(250),
            shard: Some([0, 1]),
            presence: None,
            intents: (1 + (1 << 9) + (1 << 15))
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct GatewayPresenceUpdate {
    since: Option<i32>,
    activities: Value,
    status: String,
    afk: bool
}

impl Default for GatewayPresenceUpdate {
    fn default() -> Self {
        Self {
            since: None,
            activities: Value::Null,
            status: "online".into(),
            afk: false
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct IdentifyConnectionProperties {
    os: String,
    browser: String,
    device: String
}

impl Default for IdentifyConnectionProperties {
    fn default() -> Self {
        Self {
            os: "linux".into(),
            browser: "disco".into(),
            device: "disco".into()
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct HelloPayload {
    heartbeat_interval: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct WsMessage {
    op: i32,
    s: Option<i64>,
    t: Option<String>,
    d: Option<WsPayload>
}

impl WsMessage {
    pub fn new_identify(token: String) -> Self {
        let mut ident = Identify::default();
        ident.token = token;
        Self {
            op: 2,
            s: None,
            t: None,
            d: Some(WsPayload::Identify(ident))
        }
    }

    pub fn new_heartbeat(s: Option<i64>) -> Self {
        Self {
            s,
            op: 1,
            t: None,
            d: None
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum WsPayload {
    Hello(HelloPayload),
    Identify(Identify),
    MessageCreate(MessageCreate)
}

fn start_env() {
    let _ = dotenvy::dotenv();
}

fn main() {
    ws_worker();
}

fn ws_worker() {
    start_env();
    let api_key = std::env::var("API_KEY").expect("No API_KEY environment variable");
    let mut client = websocket::ClientBuilder::new("wss://gateway.discord.gg/?v=10&encoding=json")
    .unwrap()
    .connect(None)
    .unwrap();

    let mut context = Context::default();

    client.set_nonblocking(true).unwrap();

    let (heartbeat_sender, heatbeat_receiver) = channel();
    let mut s : Option<i64> = None;

    if let Ok(message) = client.recv_message() {
        let payload = message.take_payload();
        let Ok(hello) = serde_json::from_slice::<WsMessage>(&payload) else {
            panic!("First message was not Hello (opcode 10)")
        };
        let WsPayload::Hello(payload) = hello.d.unwrap() else {
            panic!("Not hello")
        };
        let time = payload.heartbeat_interval;
        spawn_heartbeat_worker(heartbeat_sender, time);
        let identify_message = WsMessage::new_identify(api_key);
        let message = serde_json::to_string(&identify_message).unwrap();
        client.send_message(&OwnedMessage::Text(message)).unwrap();
    }

    if let Ok(ready) = client.recv_message() {
        let payload = ready.take_payload();
        let obj = serde_json::from_slice::<Value>(&payload).unwrap();
        if obj["op"] != 0 {
            panic!("Not ready message");
        }
        s = obj["s"].as_i64();
    }
    loop {
        if let Ok(_) = heatbeat_receiver.try_recv() {
            let heartbeat = WsMessage::new_heartbeat(s);
            let message = serde_json::to_string(&heartbeat).unwrap();
            client.send_message(&OwnedMessage::Text(message)).unwrap();
        }

        if let Ok(message) = client.recv_message() {
            let payload = message.take_payload();
            let value = serde_json::from_slice::<Value>(&payload).unwrap();
            context.parse_message(value);
        }
    }
}

#[derive(Debug)]
struct Message {
    content: String,
    nick: String,
    user: Option<User>,
    message_id: String,
    channel_id: String
}

impl From<MessageCreate> for Message {
    fn from(value: MessageCreate) -> Self {
        Self {
            content: value.content,
            nick: value.member.as_ref().unwrap().nick.as_ref().unwrap().to_string(),
            user: value.member.as_ref().unwrap().user.clone(),
            message_id: value.id,
            channel_id: value.channel_id
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct User {
    username: String,
    id: String
}

#[derive(Debug, Deserialize, Serialize)]
struct GuildMember {
    user: Option<User>,
    nick: Option<String>
}

#[derive(Debug, Deserialize, Serialize)]
struct MentionUser {
    id: String,
    username: String,
    member: GuildMember
}

#[derive(Debug, Deserialize, Serialize)]
struct MessageCreate {
    id: String,
    content: String,
    channel_id: String,
    guild_id: Option<String>,
    member: Option<GuildMember>,
    mentions: Vec<MentionUser>
}

struct Context {
    messages: HashMap<String, Vec<Message>>
}

impl Default for Context {
    fn default() -> Self {
        Self {
            messages: HashMap::new()
        }
    }
}

impl Context {
    pub fn parse_message(&mut self, message: Value) {       
        let Some(data) = message["t"].as_str() else {
            return;
        };
        match data {
            "MESSAGE_CREATE" => {
                let message = serde_json::from_value::<WsMessage>(message).unwrap();
                let WsPayload::MessageCreate(message) = message.d.unwrap() else {
                    panic!("Not message create")
                };
                println!("{:?}", message);
                let message = Message::from(message);
                let messages = self.messages.entry(message.channel_id.clone()).or_insert(vec![]);
                messages.push(message);
                println!("{:?}", self.messages);
            },
            value => println!("{value}")
        };
    }
}

fn heartbeat_worker(sender: &Sender<()>, time: u64) {
    thread::sleep(Duration::from_millis(time as u64));
    sender.send(()).unwrap();
}

fn spawn_heartbeat_worker(sender: Sender<()>, time: u64) {
    thread::spawn(move || {
        let mut rng= rand::thread_rng();
        let jitter = rng.gen_range(0..time);
        thread::sleep(Duration::from_millis(jitter));
        sender.send(()).unwrap();
        loop {
            heartbeat_worker(&sender, time);
        }
    });
}