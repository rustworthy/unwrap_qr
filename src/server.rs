use core::fmt;
use std::sync::{Arc, Mutex};

use actix::{Addr, System};
use actix_multipart::Multipart;
use actix_web::{
    body::MessageBody, middleware, web, web::Data, App, Error as ActixError, HttpResponse,
    HttpServer, Responder,
};
use askama::Template;
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt as _;
use indexmap::IndexMap;
use unwrap_qr::{
    errors::Error,
    queue_actor::{QueueActor, QueueHandler, RabbitMessage, SendMsg, TaskID},
    QrResponse, REQUESTS, RESPONSES,
};

type Tasks_ = IndexMap<String, Record>;
type SharedTasks = Arc<Mutex<Tasks_>>;

#[derive(Clone)]
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

#[derive(Template)]
#[template(path = "tasks.html")]
struct Tasks {
    tasks: Vec<Record>,
}

#[derive(Clone)]
struct State {
    tasks: SharedTasks,
    addr: Addr<QueueActor<ServerHandler>>,
}

async fn index_handler(app_data: Data<State>) -> impl Responder {
    let id = app_data
        .addr
        .send(SendMsg(
            "hello from index page".try_into_bytes().unwrap().to_vec(),
        ))
        .await;
    log::debug!("{:#?}", id);
    log::debug!("Send a check msg, recevied back");
    HttpResponse::Ok().body("QR Parsing Service")
}

async fn list_tasks(tasks: Data<State>) -> impl Responder {
    let tasks: Vec<Record> = tasks.tasks.lock().unwrap().values().cloned().collect();
    let renderer = Tasks { tasks };
    let rendering_results = renderer.render().unwrap(); // handle me gracefully
    HttpResponse::Ok().body(rendering_results)
}

async fn handle_upload(
    mut files: Multipart,
    tasks: Data<State>,
) -> Result<HttpResponse, ActixError> {
    while let Some(item) = files.next().await {
        let mut field = item?;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            println!("-- CHUNK: \n{:?}", std::str::from_utf8(&chunk?));
        }
    }
    Ok(HttpResponse::Ok().into())
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut sys_runner = System::new("unwrap_qr_server");
    let tasks = Arc::new(Mutex::new(IndexMap::new()));

    let handler = ServerHandler {
        tasks: tasks.clone(),
    };
    let addr = match QueueActor::new(handler, &mut sys_runner) {
        Err(e) => panic!("Failed to initiate a queue actor for SERVER: {}", e),
        Ok(addr) => addr,
    };
    // -----------------------------------------------------------------------------
    let state = State {
        tasks: tasks.clone(),
        addr,
    };
    let data = Data::new(state);

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(data.clone())
            .route("/", web::get().to(index_handler))
            .route("/tasks", web::post().to(handle_upload))
            .route("/tasks", web::get().to(list_tasks))
    });

    let awaitable_server = server
        .bind("127.0.0.1:8089")
        .expect("Failed to bind address for web server")
        .run();

    log::debug!("Launching application server");
    tokio::spawn(awaitable_server);

    // ----------------------------------------------------------------------------
    if let Err(e) = sys_runner.run() {
        panic!("Failed to launch system runner for WORKER: {}", e)
    }
}
