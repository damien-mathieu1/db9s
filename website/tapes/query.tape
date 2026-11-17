Output website/static/gif/query.gif
Source website/tapes/_header.tape
Hide
Type "export HOME=$(mktemp -d) && clear && ./target/debug/db9s --postgres postgres://db9s:db9s@localhost:55432/db9s"
Enter
Sleep 3s
Show
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Type ":users"
Enter
Sleep 1200ms
Type "/"
Sleep 400ms
Type "select name, email from users where active"
Sleep 500ms
Enter
Sleep 2s
Type "/"
Sleep 400ms
Type "select name, price from products where price < 100"
Enter
Sleep 2s
# up walks the history, kept across sessions
Type "/"
Sleep 500ms
Up
Sleep 900ms
Up
Sleep 900ms
Enter
Sleep 2s
