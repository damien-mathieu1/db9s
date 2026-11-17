Output website/static/gif/editing.gif
Source website/tapes/_header.tape
Hide
Type `export HOME=$(mktemp -d) && sqlite3 $HOME/shop.db "CREATE TABLE books (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, author TEXT, year INTEGER); INSERT INTO books (title, author, year) VALUES ('Dune', 'Frank Herbert', 1965), ('Neuromancer', 'William Gibson', 1984), ('Hyperion', 'Dan Simmons', 1989), ('Foundation', 'Isaac Asimov', 1951);" && clear && ./target/debug/db9s --sqlite sqlite://$HOME/shop.db`
Enter
Sleep 3s
Show
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1200ms
Tab
Sleep 600ms
# e: edit field by field, ctrl+s saves
Type "e"
Sleep 600ms
Type "j"
Sleep 400ms
Enter
Sleep 300ms
Type " Messiah"
Sleep 500ms
Enter
Sleep 600ms
Ctrl+S
Sleep 1500ms
# a: insert, fields left null get the database default (the id here)
Type "a"
Sleep 600ms
Type "j"
Enter
Type "Snow Crash"
Enter
Sleep 300ms
Type "j"
Enter
Type "Neal Stephenson"
Enter
Type "j"
Enter
Type "1992"
Enter
Sleep 700ms
Ctrl+S
Sleep 1500ms
# d: the confirm shows the statement it will run
Type "jj"
Sleep 400ms
Type "d"
Sleep 2500ms
Type "n"
Sleep 1500ms
