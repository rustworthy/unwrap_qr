use actix::SystemRunner;
use lapin::{
    options::{BasicConsumeOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, Connection, ConnectionProperties, Consumer, Queue,
};

use crate::errors::Error;

#[derive(Clone)]
struct Clonable;

pub fn ensure_channel(sys: &mut SystemRunner) -> Result<Channel, Error> {
    let conn = sys
        .block_on(Connection::connect(
            "amqp://127.0.0.1:5672",
            ConnectionProperties::default(),
        ))
        .map_err(|e| Error::Common(e.to_string()))?;

    sys.block_on(conn.create_channel())
        .map_err(|e| Error::Common(e.to_string()))
}

pub fn ensure_queue<'a>(
    sys: &mut SystemRunner,
    chan: &'a Channel,
    queue_name: &'a str,
) -> Result<Queue, Error> {
    sys.block_on(async {
        let opts = QueueDeclareOptions {
            auto_delete: true,
            ..Default::default()
        };
        chan.queue_declare(queue_name, opts, FieldTable::default())
            .await
    })
    .map_err(|e| Error::Common(e.to_string()))
}

pub fn ensure_consumer<'a>(
    sys: &mut SystemRunner,
    chan: &'a Channel,
    queue_name: &'a str,
) -> Result<Consumer, Error> {
    sys.block_on(async {
        chan.basic_consume(
            queue_name,
            &format!("{}-consumer", queue_name),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
    })
    .map_err(|e| Error::Common(e.to_string()))
}
