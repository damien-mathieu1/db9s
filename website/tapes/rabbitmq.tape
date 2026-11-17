Output website/static/gif/rabbitmq.gif
Source website/tapes/_header.tape
Hide
Type "export HOME=$(mktemp -d) && clear && ./target/debug/db9s --rabbitmq http://db9s:db9s@localhost:55672"
Enter
Sleep 3s
Show
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1500ms
Tab
Sleep 600ms
# / publishes to the queue; messages are peeked, never consumed
Type "/"
Sleep 300ms
Type `{"to": "ada@example.com", "template": "welcome"}`
Enter
Sleep 1200ms
Type "/"
Type `{"to": "grace@example.com", "template": "reset"}`
Enter
Sleep 1200ms
Type "r"
Sleep 2500ms
