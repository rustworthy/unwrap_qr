use actix::System;
use queens_rock::Scanner;
use unwrap_qr::errors::Error;
use unwrap_qr::queue_actor::{QueueActor, QueueHandler, RabbitMessage, TaskID};
use unwrap_qr::{ProcessingResult, REQUESTS, RESPONSES};

#[derive(Clone)]
struct WorkerHandler;

impl QueueHandler for WorkerHandler {
    fn source_queue_name(&self) -> String {
        REQUESTS.to_string()
    }

    fn target_queue_name(&self) -> String {
        RESPONSES.to_string()
    }

    fn handle(&self, _: TaskID, incoming: ProcessingResult) -> Option<ProcessingResult> {
        if let ProcessingResult::InProgress(Some(data)) = incoming {
            match self.scan(data) {
                Ok(string_befind_qr) => return Some(ProcessingResult::Success(string_befind_qr)),
                Err(e) => return Some(ProcessingResult::Failure(e.to_string())),
            }
        }
        log::error!("Worker expects some raw data to process.");
        None
    }
}

impl WorkerHandler {
    fn scan(&self, incoming: RabbitMessage) -> Result<String, Error> {
        let image = image::load_from_memory(incoming.as_slice())
            .map_err(|e| Error::Common(e.to_string()))?;

        let luma = image.to_luma8().into_vec();
        let code = Scanner::new(
            luma.as_ref(),
            image.width() as usize,
            image.height() as usize,
        )
        .scan()
        .extract(0)
        .ok_or_else(|| Error::Common("Code exctracted from QR bitmap is empty".to_string()))?;
        log::debug!("Extracted code from luma");

        let data = code
            .decode()
            .map_err(|_| Error::Common("Failed to decode".to_string()))?;
        data.try_string().map_err(|_| {
            Error::Common("Failed to build a human readable string from the code".to_string())
        })
    }
}

fn main() {
    env_logger::init();

    let mut sys_runner = System::new("unwrap_qr_worker");

    if let Err(e) = QueueActor::new(WorkerHandler {}, &mut sys_runner) {
        panic!("Failed to initiate a queue actor for WORKER: {}", e)
    }

    if let Err(e) = sys_runner.run() {
        panic!("Failed to launch system runner for WORKER: {}", e)
    }
}
