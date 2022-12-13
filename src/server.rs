use actix::{Addr, Handler as ActixHandler, Message, System};
use actix_multipart::Multipart;
use actix_web::{
    get, http::header, middleware, post, web::Data, App, Error as ActixError, HttpResponse,
    HttpServer, Responder,
};
use askama::Template;
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt as _;
use indexmap::IndexMap;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use unwrap_qr::{
    queue_actor::{QueueActor, QueueHandler, TaskID},
    ProcessingResult, REQUESTS, RESPONSES,
};
use uuid::Uuid;

type SharedTasks = Arc<Mutex<IndexMap<String, Record>>>;

#[derive(Clone, Debug)]
struct Record {
    task_id: TaskID,
    timestamp: DateTime<Utc>,
    status: ProcessingResult,
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

    fn handle(&self, id: TaskID, incoming: ProcessingResult) -> Option<ProcessingResult> {
        let mut tasks = self.tasks.lock().unwrap();
        let record = tasks.get_mut(&id.to_string()).unwrap();
        record.status = incoming;
        None
    }
}

// ------------------------------------------------------------------------------------------------
pub struct TaskMessage(pub Vec<u8>);

impl Message for TaskMessage {
    type Result = String;
}

impl ActixHandler<TaskMessage> for QueueActor<ServerHandler> {
    type Result = String;
    fn handle(&mut self, msg: TaskMessage, ctx: &mut Self::Context) -> Self::Result {
        let corr_id = Uuid::new_v4().to_string();
        log::debug!("Generated correlation_id: {}. Sending message...", corr_id);
        self.publish_message(
            corr_id.clone().into(),
            ctx,
            ProcessingResult::InProgress(Some(msg.0)),
        );
        corr_id
    }
}
// ------------------------------------------------------------------------------------------------

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

#[get("/tasks")]
async fn list_tasks(tasks: Data<State>) -> impl Responder {
    let tasks: Vec<Record> = tasks.tasks.lock().unwrap().values().cloned().collect();
    let renderer = Tasks { tasks };
    let rendering_results = renderer.render().unwrap(); // handle me gracefully
    HttpResponse::Ok().body(rendering_results)
}

#[post("/tasks")]
async fn handle_upload(
    mut items: Multipart,
    app_data: Data<State>,
) -> Result<HttpResponse, ActixError> {
    while let Some(Ok(mut item)) = items.next().await {
        let mut f = Vec::new();

        while let Some(chunk) = item.next().await {
            let data = chunk.unwrap();
            f.write_all(&data).unwrap()
        }

        // exchange raw input for correlation id; msg's trip starts here;
        let id = app_data.addr.send(TaskMessage(f)).await.unwrap();

        // add a new record, that will be redered on the task list view page;
        let record = Record {
            task_id: id.clone().into(),
            timestamp: Utc::now(),
            status: ProcessingResult::InProgress(None),
        };
        log::debug!("Adding new recored: {:?}", record);
        app_data.tasks.lock().unwrap().insert(id, record);
    }

    Ok(HttpResponse::Found()
        .append_header((header::LOCATION, "/tasks"))
        .finish())
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
            .service(handle_upload)
            .service(list_tasks)
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
