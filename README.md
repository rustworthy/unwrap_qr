### Queue Actor
*src/queue_actor.rs* will hold an abstract handler processing incoming messages and sending new messages to a queue.
The actor will create queues in RabbitMQ and subscribe to topics.

### Worker
*src/worker.rs*, the worker msg hanlder will read from 'requests' queue, decode the delivery data (qr code image) and send the result to the 'response' queue.

### Server
*src/server.rs* will have a web application with shared state. The state in its turn will have a reference to a server msg handler, that can publish raw messages
to a 'requests' queue, and read processing results from 'responses' queue.

### Flow
Procure RabbitMQ container with `make rabbit/up`. Start the server with `make serve`. At *localhost:8089/tasks* upload a few qr-code images. Start the worker with `make worker`. The worker will get the messages from the queue and process them. Upload another image or reload the page.