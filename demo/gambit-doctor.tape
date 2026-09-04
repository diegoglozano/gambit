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
Type `export PATH="$PWD/target/release:$PATH"; export PS1='❯ '`
Enter
Sleep 500ms
Show
Ctrl+L

Type "gambit --version"
Sleep 300ms
Enter
Sleep 1s

Type "gambit stats examples/valid.pgn | sed -n '1,7p'"
Sleep 300ms
Enter
Sleep 4s

Ctrl+L
Type "gambit query examples/valid.pgn \"
Enter
Type "  --position 'rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2' \"
Enter
Type "  --format count"
Sleep 300ms
Enter
Sleep 3s

Ctrl+L
Type "gambit doctor examples/invalid.pgn"
Sleep 300ms
Enter
Sleep 4s
