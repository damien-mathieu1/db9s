# Regenerates docs/*.png. Needs the seeded stack: docker compose up -d && cargo build
Output "/tmp/db9s-demo.gif"
Set Shell "bash"
Set FontSize 16
Set Width 1400
Set Height 760
Set Padding 16

Hide
# empty HOME so saved user connections never end up in the screenshots
Type "export HOME=$(mktemp -d) && clear && ./target/debug/db9s --postgres postgres://db9s:db9s@localhost:55432/db9s --mongo mongodb://localhost:57017 --rabbitmq http://db9s:db9s@localhost:55672"
Enter
Sleep 3s
Show

Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Type "j"
Sleep 1s
Tab
Sleep 500ms
Type "j"
Sleep 1s

Type "e"
Sleep 500ms
Type "j"
Enter
Backspace 20
Type "Alan Mathison Turing"
Sleep 300ms
Enter
Type "j"
Sleep 1s
Screenshot docs/edit.png

# discard the edit, back to connections, open rabbitmq
Escape
Sleep 300ms
Escape
Escape
Escape
Sleep 500ms
Type "jj"
Enter
Sleep 1s
Enter
Sleep 1s
Type "j"
Sleep 1s
Tab
Sleep 500ms
Screenshot docs/rabbitmq.png
