use serenity::{all::{Context, EventHandler, GatewayIntents, Message}, async_trait};
use sqlite::{Connection, ConnectionThreadSafe};


struct DbConnection {
    database_connection: ConnectionThreadSafe
}

impl Default for DbConnection {
    fn default() -> Self {
        Self {
            database_connection: Connection::open_thread_safe("database.db").unwrap()
        }
    }
}

impl DbConnection {
    pub fn new_message(&self, message: Message) {
        let query = "insert into Messages values (:channel_id, :message_id, :user_id, :content, :timestamp)";
        let mut statement = self.database_connection.prepare(query).unwrap();
        statement.bind_iter::<_, (_, sqlite::Value)>([(":channel_id", (message.channel_id.get() as i64).into()),
                                                      (":message_id", (message.id.get() as i64).into()),
                                                      (":user_id", (message.author.id.get() as i64).into()),
                                                      (":content", message.content.into()),
                                                      (":timestamp", message.timestamp.unix_timestamp().into())])
            .unwrap();
        statement.iter().count();
    }
}

struct Handler {
    id: u64,
    database_connection: DbConnection
}

impl Default for Handler {
    fn default() -> Self {
        let id = std::env::var("APPLICATION_ID").unwrap().parse::<u64>().unwrap();
        Self {
            id: id,
            database_connection: DbConnection::default()
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.id.get() == self.id {
            return;
        }
        self.database_connection.new_message(msg);
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
