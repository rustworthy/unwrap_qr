use core::fmt;
use std::sync::{Arc, Mutex};

use actix::Addr;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use unwrap_qr::{
    errors::Error,
    queue_actor::{QueueActor, QueueHandler, RabbitMessage, TaskID},
    QrResponse, REQUESTS, RESPONSES,
};

type Tasks = IndexMap<String, Record>;
type SharedTasks = Arc<Mutex<Tasks>>;

struct Record {
    task_id: TaskID,
    timestamp: DateTime<Utc>,
    status: Status,
}

#[derive(Clone)]
enum Status {
    Pending,
    Done(QrResponse),
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending..."),
            Self::Done(resp) => match resp {
                QrResponse::Success(data) => write!(f, "Done: {}", data),
                QrResponse::Failure(detail) => write!(f, "Error: {}", detail),
            },
        }
    }
}

#[derive(Clone)]
struct State {
    tasks: SharedTasks,
    addr: Addr<QueueActor<ServerHandler>>,
}

#[derive(Clone)]
struct ServerHandler {
    tasks: SharedTasks,
}

impl QueueHandler for ServerHandler {
    fn source_queue_name(&self) -> String {
        RESPONSES.to_string()
    }

    fn target_queue_name(&self) -> String {
        REQUESTS.to_string()
    }
    fn handle(&self, id: TaskID, incoming: RabbitMessage) -> Result<Option<RabbitMessage>, Error> {
        let mut tasks = self.tasks.lock().unwrap();
        let record = tasks.get_mut(&id.to_string()).unwrap();
        record.status = Status::Done(QrResponse::Success(String::from_utf8(incoming).unwrap()));
        Ok(None)
    }
}
fn main() {}
