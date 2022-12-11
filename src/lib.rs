pub mod errors;
pub mod queue_actor;

use actix::{Message, SystemRunner};
use errors::Error;
use futures::Future;
use lapin::{
    options::{BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection, ConnectionProperties,
};

pub const REQUESTS: &str = "requests";
pub const RESPONSES: &str = "responses";

pub fn ensure_channel(sys: &mut SystemRunner) -> Result<Channel, Error> {
    let conn = sys
        .block_on(Connection::connect(
            "amqp://127.0.0.1:5672",
            ConnectionProperties::default(),
        ))
        .map_err(|_| {
            Error::Common("Failed to establish connection to RabbitMQ server".to_string())
        })?;

    sys.block_on(async move { conn.create_channel().await })
        .map_err(|e| Error::Common(e.to_string()))
}

pub fn ensure_queue(
    chan: Channel,
    queue_name: String,
) -> impl Future<Output = Result<lapin::Queue, lapin::Error>> {
    let opts = QueueDeclareOptions {
        auto_delete: true,
        ..Default::default()
    };
    async move {
        chan.queue_declare(&queue_name, opts, FieldTable::default())
            .await
    }
}

pub fn ensure_consumer<'a>(
    chan: Channel,
    queue_name: String,
) -> impl Future<Output = Result<lapin::Consumer, lapin::Error>> + 'a {
    let consumer_tag = format!("{}-consumer", &queue_name);
    async move {
        chan.basic_consume(
            &queue_name,
            consumer_tag.as_str(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
    }
}

pub struct QrRequest {
    pub image: Vec<u8>,
}

impl Message for QrRequest {
    type Result = ();
}

#[derive(Clone)]
pub enum QrResponse {
    Success(String),
    Failure(String),
}

impl Message for QrResponse {
    type Result = ();
}

impl From<Result<String, Error>> for QrResponse {
    fn from(value: Result<String, Error>) -> Self {
        match value {
            Ok(_string) => QrResponse::Success(_string),
            Err(e) => QrResponse::Failure(e.to_string()),
        }
    }
}
