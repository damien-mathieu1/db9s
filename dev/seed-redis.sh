#!/bin/sh
# One key of every type, mirroring dev/seed.sql. Skipped if already seeded.
set -e
R="redis-cli -h redis"

until $R ping > /dev/null 2>&1; do sleep 1; done
if [ "$($R exists greeting)" = "1" ]; then echo "redis already seeded"; exit 0; fi

$R set greeting "hello from db9s"
$R hset user:1 name "Ada Lovelace" email ada@example.com active true
$R hset user:2 name "Alan Turing" email alan@example.com active true
$R hset user:3 name "Grace Hopper" email grace@example.com active false
$R rpush queue:emails "welcome:ada@example.com" "reset:grace@example.com"
$R sadd tags hardware input display
$R zadd leaderboard 42 ada 37 alan 51 grace
$R xadd events '*' type signup user ada
$R xadd events '*' type order user alan total 199
$R -n 1 set cache:homepage "<h1>cached</h1>" EX 86400

echo "redis seeded"
