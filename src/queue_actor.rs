use actix::{
    fut::wrap_future, Actor, Addr, AsyncContext, Context, Handler, Message, StreamHandler,
    SystemRunner,
};
use lapin::{
    message::Delivery,
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::{FieldTable, ShortString},
    BasicProperties, Channel, Error as LapinErr,
};
use uuid::Uuid;

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

    pub fn process_delivery(&self, item: &Delivery) -> Result<Vec<u8>, Error> {
        let correlation_id = item.properties.correlation_id().to_owned().ok_or_else(|| {
            Error::Common("No correlation ID found in msg: no address for response :(".to_string())
        })?;

        let handling_result = self
            .handler
            .handle(correlation_id, item.data.clone())
            .map_err(|e| Error::Common(format!("Error processing message: {}", e)))?;

        Ok(handling_result.unwrap_or_default())
    }

    pub fn send_message(&self, ctx: &mut Context<Self>, msg: Vec<u8>) {
        let queue_name = String::from(self.handler.target_queue_name());
        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_publish(
                "",
                &queue_name,
                BasicPublishOptions::default(),
                &msg,
                BasicProperties::default(),
            )
            .await
            .expect("Failed to send msg");
        }));
    }
}

impl<T: QueueHandler> Actor for QueueActor<T> {
    type Context = Context<Self>;
    fn started(&mut self, _: &mut Self::Context) {}
}

impl<T: QueueHandler> StreamHandler<Result<Delivery, LapinErr>> for QueueActor<T> {
    fn handle(&mut self, item: Result<Delivery, LapinErr>, ctx: &mut Self::Context) {
        let item = item.expect("Error unpacking the message");

        log::debug!(
            "Message received from {}!",
            self.handler.source_queue_name()
        );
        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_ack(item.delivery_tag, BasicAckOptions::default())
                .await
                .expect("Failed to ack")
        }));

        log::debug!("Processing message");
        let handling_result = match self.process_delivery(&item) {
            Err(e) => {
                log::warn!("{}", e);
                return;
            }
            Ok(res) => res,
        };

        log::debug!(
            "Sending processing results to {}",
            &self.handler.target_queue_name()
        );
        self.send_message(ctx, handling_result);
    }
}

pub struct SendMsg(Vec<u8>);

impl Message for SendMsg {
    type Result = String;
}

impl<T: QueueHandler> Handler<SendMsg> for QueueActor<T> {
    type Result = String;
    fn handle(&mut self, msg: SendMsg, ctx: &mut Self::Context) -> Self::Result {
        let corr_id = Uuid::new_v4().to_string();
        self.send_message(ctx, msg.0);
        corr_id
    }
}
