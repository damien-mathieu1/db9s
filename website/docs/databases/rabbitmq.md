---
description: "RabbitMQ in db9s: peek messages, publish, purge, through the management API or plain AMQP."
---

# RabbitMQ

Databases are **vhosts**, containers are **queues**, rows are **messages**. Two
ways to connect, picked from the URL scheme:

| URL | Uses | Lists |
|---|---|---|
| `http(s)://user:pass@host:15672` | the management API (plugin `rabbitmq_management`) | every vhost and queue |
| `amqp(s)://user:pass@host:5672/<vhost>?queues=a,b` | plain AMQP, no plugin needed | the vhost in the URL, the queues in `queues=` (AMQP can't list them) |

```bash
db9s --rabbitmq http://guest:guest@localhost:15672
db9s --rabbitmq 'amqp://guest:guest@localhost:5672/%2F?queues=orders,emails'
```

![rabbitmq](/gif/rabbitmq.gif)

- Messages are **peeked**, not consumed: they stay in the queue, but RabbitMQ
  marks them as redelivered.
- `/` publishes a message to the queue, `d` purges the whole queue (one message
  can't be removed on its own), `e`/`E` are off: messages are read-only.
- The [desktop app](../desktop-app.md) charts ready and unacked messages, the
  publish rate and consumers live.
