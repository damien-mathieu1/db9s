# Regenerates docs/demo.gif (README hero). Needs the seeded stack:
#   docker compose up -d && cargo build && vhs docs/demo.tape
Output docs/demo.gif
Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 640
Set Padding 20
Set TypingSpeed 45ms
Set Framerate 24

Hide
# empty HOME so saved user connections never end up in the GIF
Type "export HOME=$(mktemp -d) && clear && ./target/debug/db9s --postgres postgres://db9s:db9s@localhost:55432/db9s --mongo mongodb://localhost:57017 --rabbitmq http://db9s:db9s@localhost:55672 --redis redis://localhost:56379"
Enter
Sleep 3s
Show
Sleep 1500ms

# postgres: open, preview tables
Enter
Sleep 900ms
Enter
Sleep 1300ms
Type "j"
Sleep 1300ms

# inline edit with a movable cursor, then discard
Tab
Sleep 400ms
Type "j"
Sleep 700ms
Type "e"
Sleep 700ms
Type "j"
Enter
Sleep 500ms
Left 6
Sleep 300ms
Type "M. "
Sleep 600ms
Enter
Sleep 1300ms
Escape
Sleep 600ms

# raw SQL
Type "/"
Sleep 300ms
Type "select name, email from users where active"
Sleep 500ms
Enter
Sleep 1800ms

# mongo
Escape
Escape
Escape
Sleep 600ms
Type "j"
Sleep 400ms
Enter
Sleep 900ms
Type "jj"
Sleep 400ms
Enter
Sleep 1300ms
Type "j"
Sleep 1500ms

# rabbitmq
Escape
Escape
Escape
Sleep 600ms
Type "j"
Sleep 400ms
Enter
Sleep 900ms
Enter
Sleep 1300ms
Type "j"
Sleep 1500ms

# redis
Escape
Escape
Escape
Sleep 600ms
Type "j"
Sleep 400ms
Enter
Sleep 900ms
Enter
Sleep 1300ms
Type "jj"
Sleep 1200ms
Type "jjj"
Sleep 2500ms
