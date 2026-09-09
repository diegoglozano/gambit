# Synthetic failure/handshake scenarios, independent of a real engine search.
printf '%s' "$$" > "$0.pid"
while IFS= read -r command; do
  printf '%s\n' "$command" >> "$0.commands"
  case "$command" in
    uci)
      [ "$scenario" = startup-crash ] && exit 7
      [ "$scenario" = startup-hang ] && continue
      printf '%s\n' 'id name Stockfish 17.1 fixture' 'uciok'
      ;;
    isready) printf '%s\n' readyok ;;
    'go nodes 100'|'go nodes 100000')
      case "$scenario" in
        recorded)
          while IFS= read -r line; do printf '%s\n' "$line"; done < "$0.recorded"
          ;;
        crash) exit 9 ;;
        hang) continue ;;
        malformed) printf '%s\n' 'info score cp invalid' ;;
        mismatch) printf '%s\n' 'info score cp 25 pv d2d4' 'bestmove e2e4' ;;
        illegal-best) printf '%s\n' 'info score cp 25 pv e2e5' 'bestmove e2e5' ;;
        illegal-pv) printf '%s\n' 'info score cp 25 pv d2d4 d7d4' 'bestmove d2d4' ;;
        false-terminal) printf '%s\n' 'info score mate 0' 'bestmove (none)' ;;
        flood)
          while :; do printf '%s\n' 'info string ignored'; done
          ;;
        *)
          printf '%s\n' \
            'info depth 1 score cp 17 nodes 20 pv e2e4 e7e5' \
            'info depth 2 score cp 25 nodes 100 pv d2d4 d7d5' \
            'info depth 3 score cp 50 lowerbound nodes 120 pv e2e4' \
            'bestmove d2d4 ponder d7d5'
          ;;
      esac
      ;;
    quit) exit 0 ;;
  esac
done
