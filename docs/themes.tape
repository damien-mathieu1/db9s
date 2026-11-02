# Regenerates docs/themes/*.png (README theme gallery). Needs the seeded stack:
#   docker compose up -d && cargo build && vhs docs/themes.tape
Output "/tmp/db9s-themes.gif"
Set Shell "bash"
Set FontSize 14
Set Width 1000
Set Height 440
Set Padding 12

Hide
# empty HOME so saved user connections never end up in the screenshots
Type "export HOME=$(mktemp -d) && clear && ./target/debug/db9s --postgres postgres://db9s:db9s@localhost:55432/db9s"
Enter
Sleep 3s
Enter
Sleep 1s
Enter
Sleep 1s
Type "j"
Sleep 1s
Tab
Sleep 300ms
Show
Type ":theme default"
Enter
Sleep 800ms
Screenshot docs/themes/default.png
Type ":theme catppuccin-mocha"
Enter
Sleep 800ms
Screenshot docs/themes/catppuccin-mocha.png
Type ":theme dracula"
Enter
Sleep 800ms
Screenshot docs/themes/dracula.png
Type ":theme gruvbox"
Enter
Sleep 800ms
Screenshot docs/themes/gruvbox.png
Type ":theme nord"
Enter
Sleep 800ms
Screenshot docs/themes/nord.png
Type ":theme tokyo-night"
Enter
Sleep 800ms
Screenshot docs/themes/tokyo-night.png
