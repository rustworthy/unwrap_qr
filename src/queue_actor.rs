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

use crate::errors::Error;
use crate::{ensure_channel, ensure_consumer, ensure_queue};

pub type TaskID = ShortString;
pub type RabbitMessage = Vec<u8>;

pub trait QueueHandler: 'static + Unpin + Clone {
    fn source_queue_name(&self) -> String;
    fn target_queue_name(&self) -> String;
    fn handle(&self, id: TaskID, incoming: RabbitMessage) -> Result<Option<RabbitMessage>, Error>;
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

    pub fn process_delivery(&self, item: &Delivery) -> Result<(String, Vec<u8>), Error> {
        log::debug!(
            "Starting to process delivery with properties: {:?}",
            item.properties
        );
        let correlation_id = item.properties.correlation_id().to_owned().ok_or_else(|| {
            Error::Common("No correlation ID found in msg: no address for response :(".to_string())
        })?;

        let handling_result = self
            .handler
            .handle(correlation_id.clone(), item.data.clone())
            .map_err(|e| Error::Common(format!("Error processing message: {}", e)))?
            .unwrap_or_default();

        Ok((correlation_id.to_string(), handling_result))
    }

    pub fn send_message(&self, corr_id: String, ctx: &mut Context<Self>, msg: Vec<u8>) {
        log::debug!("Sending msg with id {}", &corr_id);
        let queue_name = String::from(self.handler.target_queue_name());
        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                &msg,
                BasicProperties::default().with_correlation_id(corr_id.into()),
            )
            .await
            .expect("Failed to send msg");
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

        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_ack(item.delivery_tag, BasicAckOptions::default())
                .await
                .expect("Stream handler failed to acknowledge message receipt")
        }));

        log::debug!("Stream handler started processing message...");
        let (corr_id, res) = match self.process_delivery(&item) {
            Err(e) => {
                log::warn!("Error occurred when proccessing delivery: {}", e);
                return;
            }
            Ok(res) => res,
        };

        log::debug!(
            "Sending processing results to {}",
            &self.handler.target_queue_name()
        );
        self.send_message(corr_id, ctx, res);
    }
}
