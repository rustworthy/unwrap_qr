use actix::{fut::wrap_future, Actor, Addr, AsyncContext, Context, StreamHandler, SystemRunner};
use lapin::message::Delivery;
use lapin::options::BasicAckOptions;
use lapin::Channel;
use serde::{Deserialize, Serialize};

use crate::errors::Error;
use crate::utilz;

type TaskID = String;

pub trait QueueHandler: 'static + Unpin {
    type Incoming: for<'de> Deserialize<'de>;
    type Outcoming: Serialize;

    fn source_queue_name(&self) -> &str;
    fn target_queue_name(&self) -> &str;
    fn handle(
        &self,
        id: &TaskID,
        incoming: Self::Incoming,
    ) -> Result<Option<Self::Outcoming>, Error>;
}

pub struct QueueActor<T: QueueHandler> {
    handler: T,
    channel: Channel,
}

impl<T: QueueHandler> Actor for QueueActor<T> {
    type Context = Context<Self>;
    fn started(&mut self, _: &mut Self::Context) {}
}

impl<T: QueueHandler> StreamHandler<Result<Delivery, lapin::Error>> for QueueActor<T> {
    fn handle(&mut self, item: Result<Delivery, lapin::Error>, ctx: &mut Self::Context) {
        let item = item.expect("Error unpacking the message");
        log::debug!("Message received!");

        let chan = self.channel.clone();
        ctx.spawn(wrap_future(async move {
            chan.basic_ack(item.delivery_tag, BasicAckOptions::default())
                .await
                .expect("Failed to ack")
        }));
        let correlation_id = item.properties.correlation_id().to_owned();
        if correlation_id.is_none() {
            log::warn!("No correlation ID found in msg: no address for response :(");
            return;
        }
        let incoming = &item.data;
    }
}

impl<T: QueueHandler> QueueActor<T> {
    pub fn new(handler: T, mut sys: &mut SystemRunner) -> Result<Addr<Self>, Error> {
        let channel = utilz::ensure_channel(&mut sys)?;
        // let chan = channel.clone();
        let _target_queue = utilz::ensure_queue(&mut sys, &channel, handler.target_queue_name())?;
        let source_queue = utilz::ensure_queue(&mut sys, &channel, handler.source_queue_name())?;

        let consumer = utilz::ensure_consumer(&mut sys, &channel, source_queue.name().as_str())?;

        // QueueActor is behaving as an Actor:
        let addr = Self::create(move |ctx| {
            ctx.add_stream(consumer);
            Self { handler, channel }
        });
        Ok(addr)
    }
}
