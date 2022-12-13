
rabbit/up:
	docker run -d --name rabbitmq -p 127.0.0.1:5672:5672 rabbitmq:3.11

rabbit/list_queues:
	docker exec rabbitmq /bin/bash -c "rabbitmqctl list_queues"

rabbit/list_exchanges:
	docker exec rabbitmq /bin/bash -c "rabbitmqctl list_exchanges"

rabbit/list_channels:
	docker exec rabbitmq /bin/bash -c "rabbitmqctl list_channels"

rabbit/list_consumers:
	docker exec rabbitmq /bin/bash -c "rabbitmqctl list_consumers"

rabbit/list_all: rabbit/list_channels rabbit/list_exchanges rabbit/list_queues rabbit/list_consumers

serve:
	fuser -k 8089/tcp || true && RUST_LOG=debug cargo run --bin unwrap_qr_server

worker:
	RUST_LOG=debug cargo run --bin unwrap_qr_worker