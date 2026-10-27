#!/bin/sh
# Declares demo queues and publishes sample messages, mirroring dev/seed.sql.
# Messages are only published when a queue is first created, so re-running `up` won't duplicate them.
set -e
API=http://db9s:db9s@rabbitmq:15672/api

until curl -sf "$API/overview" > /dev/null; do sleep 2; done

publish() {
  curl -sf -X POST -H 'content-type: application/json' \
    -d "{\"properties\":{\"content_type\":\"application/json\"},\"routing_key\":\"$1\",\"payload\":\"$2\",\"payload_encoding\":\"string\"}" \
    "$API/exchanges/%2F/amq.default/publish" > /dev/null
}

declare_queue() {
  curl -s -o /dev/null -w '%{http_code}' -X PUT -H 'content-type: application/json' \
    -d '{"durable":true}' "$API/queues/%2F/$1"
}

if [ "$(declare_queue orders)" = "201" ]; then
  publish orders '{\"order_id\": 1, \"user\": \"ada@example.com\", \"product\": \"Keyboard\", \"total\": 49.9}'
  publish orders '{\"order_id\": 2, \"user\": \"alan@example.com\", \"product\": \"Monitor\", \"total\": 199.0}'
  publish orders '{\"order_id\": 3, \"user\": \"grace@example.com\", \"product\": \"Mouse\", \"total\": 19.5}'
fi

if [ "$(declare_queue emails)" = "201" ]; then
  publish emails '{\"to\": \"ada@example.com\", \"template\": \"order_confirmation\"}'
  publish emails '{\"to\": \"grace@example.com\", \"template\": \"password_reset\"}'
fi

echo "rabbitmq seeded"
