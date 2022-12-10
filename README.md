### Queue Actor
*src/queue_actor.rs* will hold an abstract handler processing incoming messages and sending new messages to a queue.
The actor will create queues in RabbitMQ and subscribe to topics.

### Worker
*src/worler* will read from 'requests' queue, decode the delivery data (qr code image) and send the result to the 'response' queue.

### Server