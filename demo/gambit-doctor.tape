Output static/gambit-doctor.gif

Set Shell bash
Set FontSize 21
Set Width 1200
Set Height 720
Set TypingSpeed 35ms
Set Theme "Catppuccin Mocha"
Set Padding 24
Set Margin 20
Set MarginFill "#181825"
Set BorderRadius 10
Set WindowBar Colorful
Set CursorBlink false

Hide
Type `export PATH="$PWD/target/release:$PATH"; export PS1='❯ '; unlink target/demo.gambit 2>/dev/null || true`
Enter
Sleep 500ms
Show
Ctrl+L

Type "gambit --version"
Sleep 300ms
Enter
Sleep 1s

Type "gambit index examples/valid.pgn --output target/demo.gambit"
Sleep 300ms
Enter
Sleep 5s

Ctrl+L
Type "gambit index --update examples/valid.pgn \"
Enter
Type "  --output target/demo.gambit"
Sleep 300ms
Enter
Sleep 4s

Ctrl+L
Type "gambit info --check target/demo.gambit"
Sleep 300ms
Enter
Sleep 4s
