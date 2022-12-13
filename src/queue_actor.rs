use actix::{
    fut::wrap_future, Actor as ActixActor, Addr, AsyncContext, Context,
    StreamHandler as ActixStreamHandler, SystemRunner,
};
use lapin::{
    message::Delivery,
    options::{BasicAckOptions, BasicPublishOptions},
    types::ShortString,
    BasicProperties, Channel, Error as LapinErr,
};

use crate::{ensure_channel, ensure_consumer, ensure_queue};
use crate::{errors::Error, ProcessingResult};

pub type TaskID = ShortString;
pub type RabbitMessage = Vec<u8>;

pub trait QueueHandler: 'static + Unpin + Clone {
    fn source_queue_name(&self) -> String;
    fn target_queue_name(&self) -> String;
    fn handle(&self, id: TaskID, incoming: ProcessingResult) -> Option<ProcessingResult>;
}

pub struct QueueActor<T: QueueHandler> {
    handler: T,
    channel: Channel,
}

impl<T: QueueHandler> QueueActor<T> {
    pub fn new(handler: T, mut sys: &mut SystemRunner) -> Result<Addr<Self>, Error> {
        let channel = ensure_channel(&mut sys)?;
        log::debug!("Channel created");

        let _target_queue = sys
            .block_on(ensure_queue(channel.clone(), handler.target_queue_name()))
            .map_err(|e| Error::Common(e.to_string()))?;
        log::debug!("Target queue procured");

        let source_queue = sys
            .block_on(ensure_queue(channel.clone(), handler.source_queue_name()))
            .map_err(|e| Error::Common(e.to_string()))?;
        log::debug!("Source queue procured");

        let consumer = sys
            .block_on(ensure_consumer(
                channel.clone(),
                source_queue.name().to_string(),
            ))
            .map_err(|e| Error::Common(e.to_string()))?;
        log::debug!("Consumer created");

        // QueueActor is behaving as an Actor:
        let addr = Self::create(move |ctx| {
            ctx.add_stream(consumer);
            Self {
                handler: handler.clone(),
                channel,
            }
        });
        log::debug!("Added stream handler");
        Ok(addr)
    }

    pub fn publish_message(
        &self,
        corr_id: ShortString,
        ctx: &mut Context<Self>,
        msg: ProcessingResult,
    ) {
        log::debug!("Publishing msg {:?} with id {}", msg, &corr_id);
        let queue_name = String::from(self.handler.target_queue_name());
        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                serde_json::to_string(&msg).unwrap().as_bytes(),
                BasicProperties::default().with_correlation_id(corr_id.into()),
            )
            .await
            .expect("Failed to publish msg");
        }));
    }
}

// ---------------------------------------------------------------------------------------

impl<T: QueueHandler> ActixActor for QueueActor<T> {
    type Context = Context<Self>;
    fn started(&mut self, _: &mut Self::Context) {}
}

impl<T: QueueHandler> ActixStreamHandler<Result<Delivery, LapinErr>> for QueueActor<T> {
    fn handle(&mut self, item: Result<Delivery, LapinErr>, ctx: &mut Self::Context) {
        let item = item.expect("Error unpacking the message");

        log::debug!(
            "Stream handler received a message from {} queue!",
            self.handler.source_queue_name()
        );

        let correlation_id = item.properties.correlation_id().to_owned().ok_or_else(|| {
            log::error!("No correlation ID found in msg: no address for response :(");
            return;
        });
        let corr_id = correlation_id.unwrap();

        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_ack(item.delivery_tag, BasicAckOptions::default())
                .await
                .expect("Stream handler failed to acknowledge message receipt")
        }));

        log::debug!("Stream handler started processing message...");
        let to_process = serde_json::from_slice::<ProcessingResult>(item.data.as_slice()).unwrap();

        if let Some(msg) = self.handler.handle(corr_id.clone(), to_process) {
            self.publish_message(corr_id, ctx, msg);
        }
    }
}
