Output static/gambit-doctor.gif

Set Shell bash
Set FontSize 21
Set Width 1200
Set Height 580
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

Type "gambit doctor examples/valid.pgn"
Sleep 300ms
Enter
Sleep 2s

Type "gambit doctor examples/invalid.pgn"
Sleep 300ms
Enter
Sleep 4s
