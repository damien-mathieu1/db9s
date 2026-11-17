Output website/static/gif/sort-page-export.gif
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
Sleep 1500ms
Type ":sort ye"
Sleep 600ms
Tab
Sleep 300ms
Type " desc"
Sleep 400ms
Enter
Sleep 2s
Type ":sort ti"
Tab
Enter
Sleep 2s
Type ":export c"
Sleep 400ms
Tab
Enter
Sleep 2500ms
